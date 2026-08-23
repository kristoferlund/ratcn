//! The pointer gesture machine: what the runtime remembers about a mouse
//! button between its press and its release, and what one raw mouse event
//! turns into because of it.
//!
//! Backends report `Down`, `Up`, and `Moved`; components consume `Click`,
//! `Drag`, and `DragEnd`. [`Gestures::normalize`] bridges the two, and
//! everything it needs to decide with — which button is held, how far it has
//! travelled, who claimed it, what its press landed on — lives here.
//!
//! Nothing here knows the surface. The two questions that need one, "what is
//! under the pointer now" and "is this claim still routable", arrive as
//! arguments, so this whole module is a function of the gestures it holds.
//!
//! # Driving it
//!
//! [`Ratcn`](super::Ratcn) drives one [`Gestures`] for the life of the app.
//! Per raw mouse event: hit-test its cell once and hand the answer to
//! [`normalize`](Gestures::normalize); deliver each event it returns, skipping
//! those [`swallows`](Gestures::swallows) claims, and record a component's
//! capture with [`claim`](Gestures::claim); once the whole batch is delivered,
//! close a release with [`end`](Gestures::end) — the synthesized `Click` or
//! `DragEnd` in the batch still resolves its target through
//! [`capture_for`](Gestures::capture_for).
//!
//! A pointer that leaves the grid ends every gesture with
//! [`forget_all`](Gestures::forget_all). Ground moving under a live gesture —
//! a modal opening, a claim's component going undeclared — calls it off with
//! [`cancel`](Gestures::cancel) or
//! [`cancel_lost_claims`](Gestures::cancel_lost_claims), which keeps the entry
//! so its release is still swallowed and still closes it.

use super::{ChildId, MouseButton, MouseEvent, MouseKind};

/// Every mouse button whose gesture is under way, and what each one is doing.
///
/// One button's gesture runs one course: its press opens it, fixes the cell
/// travel is measured from, and records what it landed on; the component
/// handling that press may claim it; motion that leaves the pressed cell
/// turns into drags and marks the press as moved; and the release closes it,
/// becoming a `Click` when it lands back on the claim or the press target and
/// a `DragEnd` when it moved without landing there.
///
/// Buttons run those courses independently and at once, so gestures are held
/// in press order: the last entry still holding its button is the one motion
/// belongs to.
#[derive(Debug, Default)]
pub(super) struct Gestures {
    tracked: Vec<Gesture>,
}

/// Everything the runtime tracks for one mouse button between its press and
/// its release.
///
/// A button with no entry has no gesture: nothing to route to, nothing to
/// judge a release against, nothing to swallow.
#[derive(Debug)]
struct Gesture {
    button: MouseButton,
    /// The press, kept until [`Gestures::end`] closes the gesture.
    press: Press,
    routing: Routing,
}

/// Where a press landed on the grid, and whether the pointer has left that
/// cell since.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Press {
    pub(crate) column: u16,
    pub(crate) row: u16,
    /// The pointer has left the pressed cell, so this press has emitted at
    /// least one [`MouseKind::Drag`].
    pub(crate) moved: bool,
}

/// Where a gesture's events go.
///
/// The two states are exclusive: a gesture that has been called off keeps no
/// claim and no target, because nothing may act on either again.
#[derive(Debug)]
enum Routing {
    /// A live gesture.
    Active {
        /// The path that claimed it with
        /// [`EventCtx::capture_pointer`](super::EventCtx::capture_pointer).
        /// It outranks geometry for every event until the release.
        capture: Option<Vec<ChildId>>,
        /// What the press hit, `None` for empty space. Its release is judged
        /// against the claim if there is one, otherwise against this — and
        /// empty space compares equal to itself, so a press and a release
        /// that both hit nothing are still a click.
        press_target: Option<Vec<ChildId>>,
    },
    /// The gesture has been called off — its target stopped being declared, or
    /// a modal transition moved the ground under it — but the button is still
    /// held. Every event for it is swallowed until the release that ends it.
    Suppressed,
}

impl Routing {
    /// The path that claimed this gesture, if one did.
    fn capture(&self) -> Option<&[ChildId]> {
        match self {
            Self::Active { capture, .. } => capture.as_deref(),
            Self::Suppressed => None,
        }
    }

    /// Whether a release with `current` under it lands where the press
    /// landed.
    fn releases_on_target(&self, current: Option<&[ChildId]>) -> bool {
        match self {
            Self::Active {
                capture,
                press_target,
            } => current == capture.as_deref().or(press_target.as_deref()),
            Self::Suppressed => false,
        }
    }
}

impl Gestures {
    /// Turn one raw mouse event into the events to dispatch, in order, with
    /// `hit` naming whatever the surface puts under it.
    ///
    /// Backends report `Down`, `Up`, and `Moved`. This synthesizes the
    /// [`Click`](MouseKind::Click), [`Drag`](MouseKind::Drag), and
    /// [`DragEnd`](MouseKind::DragEnd) a backend cannot provide uniformly, and
    /// it is where a press is recorded and where the pointer's travel since
    /// that press is tracked.
    ///
    /// A release follows up as at most one gesture: landing back on the
    /// component the press hit makes it a `Click`, a press that moved
    /// otherwise ends as `DragEnd`, and a press that did not emits nothing. So
    /// a click never fires on a different target, and a claimed drag never
    /// also clicks.
    ///
    /// The empty answer means one thing and arises in one place: motion under
    /// a held button that has not left its pressed cell.
    pub(super) fn normalize(
        &mut self,
        event: MouseEvent,
        hit: Option<&[ChildId]>,
    ) -> Vec<MouseEvent> {
        match event.kind {
            MouseKind::Down(button) => {
                self.begin(button, event.column, event.row, hit);
                vec![event]
            }
            MouseKind::Up(button) => {
                let on_target = self.releases_on_press_target(button, hit);
                let follow = match self.get(button) {
                    // Landing back on the press target wins over having moved:
                    // movement nobody claimed as a drag is drift within the
                    // control, and drift must not eat the click. A claimed
                    // gesture reaches this arm as `false` and ends as
                    // `DragEnd`.
                    Some(_) if on_target => Some(MouseKind::Click(button)),
                    Some(gesture) if gesture.press.moved => Some(MouseKind::DragEnd(button)),
                    _ => None,
                };
                let mut out = vec![event];
                out.extend(follow.map(|kind| MouseEvent { kind, ..event }));
                out
            }
            MouseKind::Moved => match self.tracked.last_mut() {
                Some(gesture) => {
                    let press = &mut gesture.press;
                    if !press.moved && press.column == event.column && press.row == event.row {
                        return Vec::new();
                    }
                    press.moved = true;
                    vec![MouseEvent {
                        kind: MouseKind::Drag(gesture.button),
                        ..event
                    }]
                }
                None => vec![event],
            },
            // Some backends (crossterm) deliver `Drag` natively; it still
            // marks the press as moved so its release ends as a `DragEnd`.
            MouseKind::Drag(button) => {
                if let Some(gesture) = self.get_mut(button) {
                    gesture.press.moved = true;
                }
                vec![event]
            }
            // Scroll (and any already-synthesized kind) pass through.
            _ => vec![event],
        }
    }

    /// Whether a pointer gesture is under way: any button held, or a claim
    /// still outstanding. Both freeze hover.
    pub(super) fn in_flight(&self) -> bool {
        !self.tracked.is_empty()
    }

    /// The press that opened the gesture this event continues, when a claim
    /// is what routes the event there.
    pub(super) fn captured_press(&self, kind: MouseKind) -> Option<Press> {
        let gesture = self.get(continued_button(kind)?)?;
        gesture.routing.capture().is_some().then_some(gesture.press)
    }

    /// Whether this event is being swallowed, because the gesture of the
    /// button it carries has been called off.
    pub(super) fn swallows(&self, kind: MouseKind) -> bool {
        mouse_button(kind).is_some_and(|button| self.suppressed(button))
    }

    /// The path that claimed the gesture this event continues, if this event
    /// continues one and something claimed it.
    ///
    /// A claim outranks geometry until the release, so an event that reaches
    /// here with a claim goes there wherever the pointer is. A `Down` opens a
    /// gesture, and a `Moved` belongs to none at all, so neither of those
    /// consults a claim.
    pub(super) fn capture_for(&self, kind: MouseKind) -> Option<&[ChildId]> {
        self.capture(continued_button(kind)?)
    }

    /// Record the claim a component made on `button`'s gesture with
    /// [`EventCtx::capture_pointer`](super::EventCtx::capture_pointer). A
    /// suppressed gesture takes no claim: its events never reach a component.
    pub(super) fn claim(&mut self, button: MouseButton, path: Vec<ChildId>) {
        if let Some(Routing::Active { capture, .. }) =
            self.get_mut(button).map(|gesture| &mut gesture.routing)
        {
            *capture = Some(path);
        }
    }

    /// Call off `button`'s gesture, keeping it suppressed until its release.
    pub(super) fn suppress(&mut self, button: MouseButton) {
        if let Some(gesture) = self.get_mut(button) {
            gesture.routing = Routing::Suppressed;
        }
    }

    /// Call off every gesture under way. They stay suppressed until their
    /// releases.
    pub(super) fn cancel(&mut self) {
        for gesture in &mut self.tracked {
            gesture.routing = Routing::Suppressed;
        }
    }

    /// Call off every gesture whose claim `routable` rejects. A claim the
    /// surface cannot deliver to would otherwise retarget silently to whatever
    /// occupies its place.
    pub(super) fn cancel_lost_claims(&mut self, routable: impl Fn(&[ChildId]) -> bool) {
        for gesture in &mut self.tracked {
            if let Some(path) = gesture.routing.capture()
                && !routable(path)
            {
                gesture.routing = Routing::Suppressed;
            }
        }
    }

    /// Forget everything tracked for `button`: its gesture ended with this
    /// release.
    pub(super) fn end(&mut self, button: MouseButton) {
        self.tracked.retain(|gesture| gesture.button != button);
    }

    /// Forget every gesture outright, with no suppression to serve out — the
    /// pointer is gone, so no release is coming.
    pub(super) fn forget_all(&mut self) {
        self.tracked.clear();
    }

    /// Does this release complete a click?
    ///
    /// A click is a release on what the press landed on — its claim, or
    /// else what it hit. The pointer may have drifted in between; only the
    /// path has to match, which also catches a redraw sliding a different
    /// component under an unmoved pointer. A claimed gesture that moved is
    /// never a click: claiming it declared the movement meaningful, so it ends
    /// as [`DragEnd`](MouseKind::DragEnd).
    fn releases_on_press_target(&self, button: MouseButton, hit: Option<&[ChildId]>) -> bool {
        let Some(gesture) = self.get(button) else {
            return false;
        };
        if gesture.routing.capture().is_some() && gesture.press.moved {
            return false;
        }
        gesture.routing.releases_on_target(hit)
    }

    /// Record `button`'s press, starting its gesture if it has none. A button
    /// pressed again while held measures travel from the new cell and keeps
    /// its routing. The gesture becomes the most recent, which is the one
    /// motion belongs to.
    fn begin(&mut self, button: MouseButton, column: u16, row: u16, hit: Option<&[ChildId]>) {
        let routing = self
            .tracked
            .iter()
            .position(|gesture| gesture.button == button)
            .map_or_else(
                || Routing::Active {
                    capture: None,
                    press_target: hit.map(<[ChildId]>::to_vec),
                },
                |index| self.tracked.remove(index).routing,
            );
        self.tracked.push(Gesture {
            button,
            press: Press {
                column,
                row,
                moved: false,
            },
            routing,
        });
    }

    fn get(&self, button: MouseButton) -> Option<&Gesture> {
        self.tracked.iter().find(|gesture| gesture.button == button)
    }

    fn get_mut(&mut self, button: MouseButton) -> Option<&mut Gesture> {
        self.tracked
            .iter_mut()
            .find(|gesture| gesture.button == button)
    }

    fn capture(&self, button: MouseButton) -> Option<&[ChildId]> {
        self.get(button)?.routing.capture()
    }

    fn suppressed(&self, button: MouseButton) -> bool {
        self.get(button)
            .is_some_and(|gesture| matches!(gesture.routing, Routing::Suppressed))
    }
}

/// What the tests read back.
#[cfg(test)]
impl Gestures {
    pub(super) fn is_empty(&self) -> bool {
        self.tracked.is_empty()
    }

    pub(super) fn holding(&self) -> bool {
        self.in_flight()
    }

    pub(super) fn is_suppressed(&self, button: MouseButton) -> bool {
        self.suppressed(button)
    }

    pub(super) fn capture_path(&self, button: MouseButton) -> Option<&[ChildId]> {
        self.capture(button)
    }
}

/// The button whose gesture this event continues: the kinds that follow a
/// press, and so may be routed by its claim.
fn continued_button(kind: MouseKind) -> Option<MouseButton> {
    match kind {
        MouseKind::Drag(button)
        | MouseKind::Up(button)
        | MouseKind::Click(button)
        | MouseKind::DragEnd(button) => Some(button),
        _ => None,
    }
}

/// The button this event carries, for the kinds that carry one.
fn mouse_button(kind: MouseKind) -> Option<MouseButton> {
    match kind {
        MouseKind::Down(button)
        | MouseKind::Up(button)
        | MouseKind::Click(button)
        | MouseKind::Drag(button)
        | MouseKind::DragEnd(button) => Some(button),
        MouseKind::Moved | MouseKind::Exited | MouseKind::Scroll(_) => None,
    }
}

#[cfg(test)]
mod tests {
    //! The machine on its own, driven by hand with the hit path supplied at
    //! each step. The engine's trace tests cover the same rules end to end;
    //! these pin them where they are decided.

    use super::*;
    use crate::runtime::Modifiers;

    const LEFT: MouseButton = MouseButton::Left;

    fn at(kind: MouseKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: Modifiers::NONE,
        }
    }

    fn kinds(events: &[MouseEvent]) -> Vec<MouseKind> {
        events.iter().map(|event| event.kind).collect()
    }

    fn id(name: &'static str) -> Vec<ChildId> {
        vec![ChildId::Static(name)]
    }

    #[test]
    fn a_press_opens_a_gesture_and_motion_inside_its_cell_says_nothing() {
        let mut gestures = Gestures::default();
        assert!(!gestures.in_flight());

        assert_eq!(
            kinds(&gestures.normalize(at(MouseKind::Down(LEFT), 2, 3), None)),
            [MouseKind::Down(LEFT)]
        );
        assert!(gestures.holding());

        // The pointer is still in the pressed cell, so there is no drag yet.
        assert!(
            gestures
                .normalize(at(MouseKind::Moved, 2, 3), None)
                .is_empty()
        );
        // A cell away it becomes one, and stays one even back home: leaving
        // the cell once is what the press remembers.
        assert_eq!(
            kinds(&gestures.normalize(at(MouseKind::Moved, 3, 3), None)),
            [MouseKind::Drag(LEFT)]
        );
        assert_eq!(
            kinds(&gestures.normalize(at(MouseKind::Moved, 2, 3), None)),
            [MouseKind::Drag(LEFT)]
        );
    }

    #[test]
    fn a_release_on_the_press_target_is_a_click_however_far_the_pointer_drifted() {
        let mut gestures = Gestures::default();
        let button = id("button");
        let _ = gestures.normalize(at(MouseKind::Down(LEFT), 2, 3), Some(&button));
        let _ = gestures.normalize(at(MouseKind::Moved, 3, 3), Some(&button));

        assert_eq!(
            kinds(&gestures.normalize(at(MouseKind::Up(LEFT), 3, 3), Some(&button))),
            [MouseKind::Up(LEFT), MouseKind::Click(LEFT)]
        );
        gestures.end(LEFT);
        assert!(gestures.is_empty());
    }

    #[test]
    fn a_release_away_from_the_press_target_ends_a_drag_and_otherwise_nothing() {
        let mut gestures = Gestures::default();
        let button = id("button");
        let elsewhere = id("elsewhere");

        let _ = gestures.normalize(at(MouseKind::Down(LEFT), 2, 3), Some(&button));
        let _ = gestures.normalize(at(MouseKind::Moved, 9, 3), Some(&elsewhere));
        assert_eq!(
            kinds(&gestures.normalize(at(MouseKind::Up(LEFT), 9, 3), Some(&elsewhere))),
            [MouseKind::Up(LEFT), MouseKind::DragEnd(LEFT)]
        );
        gestures.end(LEFT);

        // The same release with no motion behind it has no drag to end.
        let _ = gestures.normalize(at(MouseKind::Down(LEFT), 2, 3), Some(&button));
        assert_eq!(
            kinds(&gestures.normalize(at(MouseKind::Up(LEFT), 9, 3), Some(&elsewhere))),
            [MouseKind::Up(LEFT)]
        );
    }

    #[test]
    fn pressing_a_held_button_again_restarts_its_travel_and_keeps_its_routing() {
        let mut gestures = Gestures::default();
        let handle = id("handle");
        let _ = gestures.normalize(at(MouseKind::Down(LEFT), 2, 3), Some(&handle));
        gestures.claim(LEFT, handle.clone());

        // A backend that repeats a press it never released measures travel
        // from the new cell, and the claim and target the first press settled
        // carry over.
        let _ = gestures.normalize(at(MouseKind::Down(LEFT), 5, 3), None);
        assert_eq!(gestures.capture_path(LEFT), Some(handle.as_slice()));
        assert!(
            gestures
                .normalize(at(MouseKind::Moved, 5, 3), Some(&handle))
                .is_empty()
        );
        assert_eq!(
            kinds(&gestures.normalize(at(MouseKind::Up(LEFT), 5, 3), Some(&handle))),
            [MouseKind::Up(LEFT), MouseKind::Click(LEFT)]
        );
    }

    #[test]
    fn a_gesture_called_off_takes_no_claim_no_target_and_no_follow_up() {
        let mut gestures = Gestures::default();
        let button = id("button");
        let _ = gestures.normalize(at(MouseKind::Down(LEFT), 2, 3), Some(&button));
        gestures.cancel();

        assert!(gestures.swallows(MouseKind::Up(LEFT)));
        assert!(!gestures.swallows(MouseKind::Moved));
        gestures.claim(LEFT, button.clone());
        assert_eq!(gestures.capture_path(LEFT), None);
        assert_eq!(
            kinds(&gestures.normalize(at(MouseKind::Up(LEFT), 2, 3), Some(&button))),
            [MouseKind::Up(LEFT)]
        );

        // The release still ends it, which is what the suppression waits for.
        gestures.end(LEFT);
        assert!(gestures.is_empty());
    }
}
