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
//! [`Ratcn`](super::Ratcn) drives one [`Gestures`] for the life of the app,
//! and one raw mouse event is one turn of this loop:
//!
//! 1. Hit-test the raw event's cell once, and hand the answer to
//!    [`normalize`](Gestures::normalize). It opens a gesture on a press,
//!    tracks travel on motion, and returns the events to deliver, in order.
//! 2. Deliver each one, skipping any that [`swallows`](Gestures::swallows)
//!    claims. A component may take the gesture during its `Down`, which the
//!    driver records with [`claim`](Gestures::claim).
//! 3. After the `Down` is delivered, and never before — the claim it may have
//!    made outranks geometry — call
//!    [`record_press_target`](Gestures::record_press_target) with the same hit
//!    path.
//! 4. Once the whole batch is delivered, and only then, close a release with
//!    [`end`](Gestures::end). The synthesized `Click` or `DragEnd` in that
//!    batch still resolves its target through
//!    [`capture_for`](Gestures::capture_for), so a gesture ended at the `Up`
//!    would send its own follow-up somewhere else.
//!
//! Two events break the loop. A pointer that leaves the grid ends every
//! gesture with [`forget_all`](Gestures::forget_all), because no release is
//! coming. Ground moving under a live gesture — a modal opening, a claim's
//! component going undeclared — calls it off with
//! [`cancel`](Gestures::cancel) or
//! [`cancel_lost_claims`](Gestures::cancel_lost_claims), which keeps the
//! entry so its release is still swallowed and still closes it. A modal that
//! has just opened takes the shorter turn
//! [`Ratcn::consume_mouse_without_routing`](super::Ratcn::consume_mouse_without_routing)
//! runs: cancel, normalize, swallow the press, end the release — the surface
//! the event was aimed at is gone, so nothing is delivered.

use super::{ChildId, MouseButton, MouseEvent, MouseKind};

/// Every mouse button whose gesture is under way, and what each one is doing.
///
/// One button's gesture runs one course: its press opens it and fixes the
/// cell travel is measured from; the component handling that press may claim
/// it; what the press landed on — the claim if there was one, otherwise
/// whatever the press hit — is recorded as the target its release is judged
/// against; motion that leaves the pressed cell turns into drags and marks
/// the press as moved; and the release closes it, becoming a `Click` when it
/// lands back on that target and a `DragEnd` when it moved without landing
/// there.
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
    /// The press, while the button is physically down. The release takes it,
    /// leaving the routing the events it synthesizes still need.
    press: Option<Press>,
    routing: Routing,
}

/// Where a press landed on the grid, and whether the pointer has left that
/// cell — the half of a gesture that decides what motion and release
/// normalize into.
#[derive(Debug, Clone, Copy)]
struct Press {
    column: u16,
    row: u16,
    /// The pointer has left the pressed cell, so this press has emitted at
    /// least one [`MouseKind::Drag`]. Whether that makes the release a drag
    /// end is decided by [`Gestures::releases_on_press_target`].
    moved: bool,
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
        /// What the press landed on, for its release to be judged against.
        press_target: PressTarget,
    },
    /// The gesture has been called off — its target stopped being declared, or
    /// a modal transition moved the ground under it — but the button is still
    /// held. Every event for it is swallowed until the release that ends it.
    Suppressed,
}

impl Default for Routing {
    /// A gesture that has just begun: nothing claimed yet, no press recorded
    /// yet.
    fn default() -> Self {
        Self::Active {
            capture: None,
            press_target: PressTarget::Unrecorded,
        }
    }
}

impl Routing {
    /// The path that claimed this gesture, if one did.
    fn capture(&self) -> Option<&[ChildId]> {
        match self {
            Self::Active { capture, .. } => capture.as_deref(),
            Self::Suppressed => None,
        }
    }

    /// Whether a release with `current` under it lands where the press landed
    /// — the click test, with "empty space" comparing equal to itself.
    fn releases_on_target(&self, current: Option<&[ChildId]>) -> bool {
        match self {
            Self::Active { press_target, .. } => press_target.holds(current),
            Self::Suppressed => false,
        }
    }
}

/// Where a press landed, as the thing its release is judged against.
///
/// A press lands somewhere, lands on empty space, or has not been delivered
/// yet, and those are the three states. Empty space is a target like any
/// other: a press and a release that both hit nothing are still the same
/// place, and so still a click — on nothing.
#[derive(Debug)]
enum PressTarget {
    /// The gesture has begun and its press is still on its way to a
    /// component, so there is nothing yet to judge a release against. It
    /// exists to keep [`holds`](Self::holds) total: a release can only be
    /// asked about after its press was delivered or the gesture was
    /// suppressed, and suppression answers first.
    Unrecorded,
    /// The press landed where nothing is declared.
    Nothing,
    /// The press landed on this identity path.
    Path(Vec<ChildId>),
}

impl PressTarget {
    /// What a press that hit `path`, or nothing, landed on.
    fn at(path: Option<Vec<ChildId>>) -> Self {
        path.map_or(Self::Nothing, Self::Path)
    }

    /// Whether a release with `current` under it lands here.
    fn holds(&self, current: Option<&[ChildId]>) -> bool {
        match self {
            Self::Unrecorded => false,
            Self::Nothing => current.is_none(),
            Self::Path(path) => current == Some(path.as_slice()),
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
                self.begin(button, event.column, event.row);
                vec![event]
            }
            MouseKind::Up(button) => {
                let on_target = self.releases_on_press_target(button, hit);
                let press = self
                    .get_mut(button)
                    .and_then(|gesture| gesture.press.take());
                let follow = match press {
                    // Landing back on the press target wins over having moved:
                    // movement nobody claimed as a drag is drift within the
                    // control, and drift must not eat the click. A claimed
                    // gesture reaches this arm as `false` and ends as
                    // `DragEnd`.
                    Some(_) if on_target => Some(MouseKind::Click(button)),
                    Some(press) if press.moved => Some(MouseKind::DragEnd(button)),
                    _ => None,
                };
                let mut out = vec![event];
                out.extend(follow.map(|kind| MouseEvent { kind, ..event }));
                out
            }
            MouseKind::Moved => match self.held_press() {
                Some((button, press)) => {
                    if !press.moved && press.column == event.column && press.row == event.row {
                        return Vec::new();
                    }
                    press.moved = true;
                    vec![MouseEvent {
                        kind: MouseKind::Drag(button),
                        ..event
                    }]
                }
                None => vec![event],
            },
            // Some backends (crossterm) deliver `Drag` natively; it still
            // marks the press as moved so its release ends as a `DragEnd`.
            MouseKind::Drag(button) => {
                if let Some(press) = self
                    .get_mut(button)
                    .and_then(|gesture| gesture.press.as_mut())
                {
                    press.moved = true;
                }
                vec![event]
            }
            // Scroll (and any already-synthesized kind) pass through.
            _ => vec![event],
        }
    }

    /// Whether a pointer gesture is under way: a claimed capture, or any
    /// button still held. Both freeze hover, and they have to agree — the
    /// event path stops writing hover the moment a button goes down, because
    /// motion under a held button normalizes to `Drag` rather than `Moved`,
    /// so a redraw path that kept following the pointer would be the only
    /// thing moving hover mid-gesture.
    pub(super) fn in_flight(&self) -> bool {
        self.holding()
            || self
                .tracked
                .iter()
                .any(|gesture| gesture.routing.capture().is_some())
    }

    /// Whether any button is physically down.
    pub(super) fn holding(&self) -> bool {
        self.tracked.iter().any(|gesture| gesture.press.is_some())
    }

    /// The path that claimed `button`'s gesture, if one did.
    pub(super) fn capture_path(&self, button: MouseButton) -> Option<&[ChildId]> {
        self.get(button)?.routing.capture()
    }

    /// Whether this event is being swallowed, because the gesture of the
    /// button it carries has been called off.
    pub(super) fn swallows(&self, kind: MouseKind) -> bool {
        mouse_button(kind).is_some_and(|button| self.is_suppressed(button))
    }

    /// Whether `button`'s events are being swallowed.
    pub(super) fn is_suppressed(&self, button: MouseButton) -> bool {
        self.get(button)
            .is_some_and(|gesture| matches!(gesture.routing, Routing::Suppressed))
    }

    /// The path that claimed the gesture this event continues, if this event
    /// continues one and something claimed it.
    ///
    /// A claim outranks geometry until the release, so an event that reaches
    /// here with a claim goes there wherever the pointer is. A `Down` opens a
    /// gesture, and a `Moved` belongs to none at all, so neither of those
    /// consults a claim.
    pub(super) fn capture_for(&self, kind: MouseKind) -> Option<&[ChildId]> {
        match kind {
            MouseKind::Drag(button)
            | MouseKind::Up(button)
            | MouseKind::Click(button)
            | MouseKind::DragEnd(button) => self.capture_path(button),
            _ => None,
        }
    }

    /// Record the claim a component made on `button`'s gesture with
    /// [`EventCtx::capture_pointer`](super::EventCtx::capture_pointer).
    pub(super) fn claim(&mut self, button: MouseButton, path: Vec<ChildId>) {
        if let Some((capture, _)) = self.active(button) {
            *capture = Some(path);
        }
    }

    /// Store what this press landed on, for
    /// [`releases_on_press_target`](Self::releases_on_press_target) to judge
    /// its release against. A capture outranks geometry: a component that
    /// claimed the gesture owns it wherever the pointer then goes.
    pub(super) fn record_press_target(&mut self, button: MouseButton, hit: Option<Vec<ChildId>>) {
        let target = self.capture_path(button).map(<[ChildId]>::to_vec).or(hit);
        if let Some((_, press_target)) = self.active(button) {
            *press_target = PressTarget::at(target);
        }
    }

    /// Call off `button`'s gesture, keeping it suppressed until its release.
    ///
    /// A button with no gesture has nothing to call off: this is reached for a
    /// press, and a press has already opened one.
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

    /// Whether no button is being tracked at all: nothing held, nothing
    /// claimed, and no suppression waiting for the release that ends it.
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.tracked.is_empty()
    }

    /// Does this release complete a click?
    ///
    /// A click is a release on the component the press hit, which is what
    /// `hit` reports. The pointer may have moved in between — drifting a
    /// column while pressing a button is still a click, and it is only the hit
    /// path that has to match. That path comparison also catches the reverse
    /// case, where neither the pointer nor the press moved but a redraw slid a
    /// different component under it.
    ///
    /// One thing disqualifies an otherwise matching release: a gesture that a
    /// component claimed with
    /// [`EventCtx::capture_pointer`](super::EventCtx::capture_pointer) *and*
    /// then dragged. Claiming it declares the movement meaningful, so it ends
    /// as [`DragEnd`](MouseKind::DragEnd). A claim that never moved is still a
    /// click, which is what lets one component both drag and be clicked.
    fn releases_on_press_target(&self, button: MouseButton, hit: Option<&[ChildId]>) -> bool {
        let Some(gesture) = self.get(button) else {
            return false;
        };
        if gesture.routing.capture().is_some() && gesture.press.is_some_and(|press| press.moved) {
            return false;
        }
        gesture.routing.releases_on_target(hit)
    }

    /// Record `button`'s press, starting its gesture if it has none. The
    /// gesture becomes the most recent, which is the one motion belongs to.
    fn begin(&mut self, button: MouseButton, column: u16, row: u16) {
        let routing = self
            .tracked
            .iter()
            .position(|gesture| gesture.button == button)
            .map_or_else(Routing::default, |index| self.tracked.remove(index).routing);
        self.tracked.push(Gesture {
            button,
            press: Some(Press {
                column,
                row,
                moved: false,
            }),
            routing,
        });
    }

    /// What is tracked for `button`, while its gesture is under way.
    fn get(&self, button: MouseButton) -> Option<&Gesture> {
        self.tracked.iter().find(|gesture| gesture.button == button)
    }

    fn get_mut(&mut self, button: MouseButton) -> Option<&mut Gesture> {
        self.tracked
            .iter_mut()
            .find(|gesture| gesture.button == button)
    }

    /// The press motion belongs to: the most recent button still held.
    fn held_press(&mut self) -> Option<(MouseButton, &mut Press)> {
        self.tracked
            .iter_mut()
            .rev()
            .find_map(|gesture| Some((gesture.button, gesture.press.as_mut()?)))
    }

    /// The claim and the press target of `button`'s live gesture, to write
    /// into.
    ///
    /// `None` means the button has no gesture, or one that has been called
    /// off, and a caller that finds it so writes nothing: a suppressed gesture
    /// takes no new claim and no new press target, because nothing will ever
    /// act on either again. Both writers may pass over that quietly rather
    /// than guard against it, because the suppression stopped the event
    /// upstream: [`Ratcn::deliver_mouse`](super::Ratcn::deliver_mouse)
    /// swallows every event for a suppressed button before a component can
    /// claim it or a press can land, so there is no fact to record in the
    /// first place.
    fn active(
        &mut self,
        button: MouseButton,
    ) -> Option<(&mut Option<Vec<ChildId>>, &mut PressTarget)> {
        match &mut self.get_mut(button)?.routing {
            Routing::Active {
                capture,
                press_target,
            } => Some((capture, press_target)),
            Routing::Suppressed => None,
        }
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
        gestures.record_press_target(LEFT, Some(button.clone()));
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
        gestures.record_press_target(LEFT, Some(button.clone()));
        let _ = gestures.normalize(at(MouseKind::Moved, 9, 3), Some(&elsewhere));
        assert_eq!(
            kinds(&gestures.normalize(at(MouseKind::Up(LEFT), 9, 3), Some(&elsewhere))),
            [MouseKind::Up(LEFT), MouseKind::DragEnd(LEFT)]
        );
        gestures.end(LEFT);

        // The same release with no motion behind it has no drag to end.
        let _ = gestures.normalize(at(MouseKind::Down(LEFT), 2, 3), Some(&button));
        gestures.record_press_target(LEFT, Some(button.clone()));
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
        gestures.record_press_target(LEFT, Some(handle.clone()));

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
        gestures.record_press_target(LEFT, Some(button.clone()));
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
