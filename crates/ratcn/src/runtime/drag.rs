//! Pointer dragging: turning a press, some movement, and a release into an
//! offset a component can act on.
//!
//! Dragging is fiddly for the same reason every time. The pointer leaves the
//! component's area mid-gesture, so plain hit-testing stops delivering events;
//! the gesture outlives the component instance, because every frame declares a
//! new one; and a release has to be distinguished from an unrelated click.
//!
//! [`EventCtx::drag`](super::EventCtx::drag) handles all three. Give it the
//! mouse event and the offset your app currently stores, and it returns a
//! [`DragPhase`] telling you what stage the gesture is at. It captures the
//! pointer on the press, so movement and release keep arriving no matter where
//! the pointer goes, and it keeps its bookkeeping against the component's
//! identity path rather than the instance.
//!
//! What the offset *means*, how it is clamped, which cells count as a drag
//! handle, and what a drop does are all yours. [`clamp_offset`] and
//! [`offset_rect`] cover the most common bound — keep the dragged box inside a
//! containing area.
//!
//! General rect geometry ([`is_border`](crate::geometry::is_border) and
//! friends) lives in [`geometry`](crate::geometry); this module is dragging
//! only.

use ratatui::layout::{Position, Rect};

use super::{EventCtx, MouseButton, MouseEvent, MouseKind};

/// How far something has been dragged from its origin, in terminal cells.
///
/// Signed on both axes, so it can move in any direction. This is the value your
/// app stores between events — the drag helper is stateless as far as your
/// domain is concerned.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CellOffset {
    /// Columns right of the origin; negative is left.
    pub x: i16,
    /// Rows below the origin; negative is up.
    pub y: i16,
}

impl CellOffset {
    /// An offset of `x` columns and `y` rows.
    #[must_use]
    pub const fn new(x: i16, y: i16) -> Self {
        Self { x, y }
    }
}

/// Options for one call to [`EventCtx::drag`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragOptions {
    offset: CellOffset,
    button: MouseButton,
    can_start: bool,
}

impl DragOptions {
    /// Anchor a gesture that starts now to `offset`.
    ///
    /// Pass the offset your app has stored for this thing. Later
    /// [`DragPhase::Moved`] offsets are this value plus how far the pointer has
    /// travelled since the press, so a second drag continues from where the
    /// first one left off.
    #[must_use]
    pub const fn new(offset: CellOffset) -> Self {
        Self {
            offset,
            button: MouseButton::Left,
            can_start: true,
        }
    }

    /// Match a button other than the default left button.
    #[must_use]
    pub const fn button(mut self, button: MouseButton) -> Self {
        self.button = button;
        self
    }

    /// Gate whether a matching press may start a gesture at all.
    ///
    /// This is how a component restricts dragging to a handle: hit-test the
    /// press yourself and pass the result. It only affects `Down`. A gesture
    /// already underway keeps receiving movement and release even if a later
    /// frame rebuilds the component with `can_start` false, so a drag can never
    /// be stranded half-finished.
    #[must_use]
    pub const fn start_if(mut self, can_start: bool) -> Self {
        self.can_start = can_start;
        self
    }
}

impl Default for DragOptions {
    fn default() -> Self {
        Self::new(CellOffset::default())
    }
}

/// What stage of a drag gesture one mouse event turned out to be.
///
/// Returned by [`EventCtx::drag`]; match on it to decide what to emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragPhase {
    /// The gesture just started: an eligible press of the configured button,
    /// which also captured the pointer.
    Down,
    /// The pointer moved while held. Usually the phase that emits a message.
    Moved {
        /// Where the thing should now sit: the anchor offset plus how far the
        /// pointer has moved since the press. Unclamped — apply your own bound,
        /// such as [`clamp_offset`].
        offset: CellOffset,
        /// Current pointer position, in the coordinate space the component was
        /// declared with.
        position: Position,
    },
    /// The button was released and the gesture's internal state has been
    /// cleared.
    Ended {
        /// Where the release happened, in the coordinate space the component
        /// was declared with — hit-test this to find a drop target.
        position: Position,
        /// False if the pointer never actually moved, which lets you treat the
        /// gesture as a click on the handle rather than as a drag.
        moved: bool,
    },
    /// Not part of a gesture this component is running: a different button, or
    /// movement and release with no press of its own. Fall through to your
    /// other mouse handling.
    Ignored,
}

/// A drag under way: the button holding it, where it started from, and
/// whether it has moved since.
///
/// [`EventCtx::drag`] keeps `Option<ActiveDrag>` at the component's identity
/// path: the transient store inserts a default before the press fills it in, a
/// gesture is never observed in that empty state, and the release removes the
/// transient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveDrag {
    /// Only this button's movement and release continue the gesture.
    button: MouseButton,
    /// The pressed cell and the offset current when it was pressed.
    anchor: AnchorPoint,
    /// The pointer has left the pressed cell, which is what tells a drag from
    /// a click on the same handle.
    moved: bool,
}

impl EventCtx<'_> {
    /// Feed one mouse event into this component's drag gesture.
    ///
    /// Call it from `handle_event` for every mouse event and match the
    /// [`DragPhase`] it returns. An eligible `Down` on the configured button
    /// (left unless [`DragOptions::button`] says otherwise) starts the gesture,
    /// captures the pointer, and anchors to the offset you supplied; subsequent
    /// movement and release then arrive here regardless of where the pointer
    /// is. Anything not part of that gesture comes back as
    /// [`DragPhase::Ignored`].
    ///
    /// [`DragPhase`] positions are in the coordinate space the component was
    /// declared with, matching [`EventCtx::area`](Self::area) and the
    /// [`MouseEvent`] passed in. The anchor a gesture starts from is kept
    /// internally in screen coordinates, so scrolling a
    /// [`viewport`](super::DeclareCtx::viewport) mid-gesture leaves the offset
    /// measuring pointer travel alone.
    ///
    /// The gesture is tracked against the component's identity path, so it
    /// survives the component instance being rebuilt each frame. What it does
    /// not survive is the path itself disappearing: if a successful render no
    /// longer declares it, [`Ratcn`](super::Ratcn) drops the capture and
    /// suppresses the rest of the physical gesture, so no stray `Up` lands
    /// somewhere else. Cleaning up app state that referred to the vanished
    /// thing is still yours to do.
    ///
    /// # Panics
    ///
    /// Panics outside a [`Ratcn`](super::Ratcn) event dispatch, or if this path
    /// already stores a transient of another type — the helper uses
    /// [`EventCtx::transient`] internally, so one path cannot both drag and
    /// keep an unrelated transient.
    pub fn drag(&mut self, mouse: &MouseEvent, options: DragOptions) -> DragPhase {
        let position = Position::new(mouse.column, mouse.row);
        let screen = self.pointer.screen_mouse.unwrap_or(*mouse);
        match mouse.kind {
            MouseKind::Down(button) if button == options.button && options.can_start => {
                self.capture_pointer(button);
                *self.transient::<Option<ActiveDrag>>() = Some(ActiveDrag {
                    button,
                    anchor: AnchorPoint {
                        column: screen.column,
                        row: screen.row,
                        offset: options.offset,
                    },
                    moved: false,
                });
                DragPhase::Down
            }
            MouseKind::Drag(button) => {
                let Some(drag) = self
                    .transient_if_present::<Option<ActiveDrag>>()
                    .and_then(Option::as_mut)
                else {
                    return DragPhase::Ignored;
                };
                if drag.button != button {
                    return DragPhase::Ignored;
                }
                drag.moved = true;
                DragPhase::Moved {
                    offset: drag.anchor.offset_at(screen.column, screen.row),
                    position,
                }
            }
            MouseKind::Up(button) => {
                let Some(drag) = self
                    .transient_if_present::<Option<ActiveDrag>>()
                    .copied()
                    .flatten()
                else {
                    return DragPhase::Ignored;
                };
                if drag.button != button {
                    return DragPhase::Ignored;
                }
                // The gesture is over: drop the stored state and report from
                // the copy taken above.
                self.take_transient::<Option<ActiveDrag>>();
                DragPhase::Ended {
                    position,
                    moved: drag.moved,
                }
            }
            _ => DragPhase::Ignored,
        }
    }
}

/// Where a drag started, and the offset it started from — the arithmetic
/// behind [`DragPhase::Moved`], with no notion of buttons or capture.
///
/// It exists only inside an [`ActiveDrag`], which is what makes it total: an
/// anchor with no drag around it is not a state this module can be in, so
/// [`offset_at`](Self::offset_at) always has an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AnchorPoint {
    column: u16,
    row: u16,
    offset: CellOffset,
}

impl AnchorPoint {
    /// The offset for the pointer now at `(column, row)`, anchored to where the
    /// drag began. The component clamps the result however it likes (see
    /// [`clamp_offset`]).
    fn offset_at(self, column: u16, row: u16) -> CellOffset {
        CellOffset {
            x: axis_delta(self.offset.x, self.column, column),
            y: axis_delta(self.offset.y, self.row, row),
        }
    }
}

/// `anchor_offset + (current_cell - anchor_cell)`, computed in `i32` and
/// saturated into `i16` so a pointer at any terminal coordinate can't overflow.
fn axis_delta(anchor_offset: i16, anchor_cell: u16, current_cell: u16) -> i16 {
    clamp_i16(i32::from(anchor_offset) + i32::from(current_cell) - i32::from(anchor_cell))
}

/// Saturate an `i32` into `i16`. Used after computations that may briefly exceed
/// the range (deltas, clamp bounds).
#[expect(
    clippy::cast_possible_truncation,
    reason = "value is clamped to the i16 range on the line above the cast"
)]
fn clamp_i16(value: i32) -> i16 {
    value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

/// Clamp `offset` so `rect`, shifted by it, stays within `area` — the common
/// "keep the dragged box on screen" bound. Components with other rules (e.g. a
/// resizable pane's min/max size) clamp their own way instead.
#[must_use]
pub fn clamp_offset(area: Rect, rect: Rect, offset: CellOffset) -> CellOffset {
    CellOffset {
        x: clamp_axis(offset.x, rect.x, rect.width, area.x, area.width),
        y: clamp_axis(offset.y, rect.y, rect.height, area.y, area.height),
    }
}

/// Apply a (clamped) `offset` to `rect`, keeping it within `area` — the common
/// "place this box at its base position plus the drag offset" step.
#[must_use]
pub fn offset_rect(area: Rect, rect: Rect, offset: CellOffset) -> Rect {
    let offset = clamp_offset(area, rect, offset);
    Rect {
        x: offset_u16(rect.x, offset.x),
        y: offset_u16(rect.y, offset.y),
        ..rect
    }
}

/// Shift `value` by a signed `offset`, saturating at the `u16` bounds.
#[must_use]
pub(crate) fn offset_u16(value: u16, offset: i16) -> u16 {
    if offset.is_negative() {
        value.saturating_sub(offset.unsigned_abs())
    } else {
        value.saturating_add(offset.unsigned_abs())
    }
}

fn clamp_axis(offset: i16, rect_start: u16, rect_len: u16, area_start: u16, area_len: u16) -> i16 {
    // Range of offsets that keep [rect_start, rect_start+rect_len) inside
    // [area_start, area_start+area_len). `min`/`max` are ordered defensively in
    // case the rect is larger than the area (then the range is inverted).
    let min = i32::from(area_start) - i32::from(rect_start);
    let max =
        i32::from(area_start) + i32::from(area_len) - i32::from(rect_start) - i32::from(rect_len);
    clamp_i16(i32::from(offset).clamp(min.min(max), min.max(max)))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::runtime::component::PointerInputs;
    use crate::runtime::{ChildId, Modifiers};

    fn mouse(kind: MouseKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn captured_drag_reports_click_without_drag_and_cleans_up() {
        let path = vec![ChildId::Static("drag")];
        let mut transients = HashMap::new();
        let mut capture = None;
        let mut ctx = EventCtx::at(
            path.clone(),
            Rect::ZERO,
            &mut transients,
            PointerInputs {
                capture: Some(&mut capture),
                button: Some(MouseButton::Left),
                screen_mouse: None,
            },
        );

        assert_eq!(
            ctx.drag(
                &mouse(MouseKind::Down(MouseButton::Left), 2, 3),
                DragOptions::new(CellOffset::new(4, -1)),
            ),
            DragPhase::Down
        );
        assert_eq!(capture, Some(path.clone()));

        let mut no_capture = None;
        let mut ctx = EventCtx::at(
            path.clone(),
            Rect::ZERO,
            &mut transients,
            PointerInputs {
                capture: Some(&mut no_capture),
                button: None,
                screen_mouse: None,
            },
        );
        assert_eq!(
            ctx.drag(
                &mouse(MouseKind::Up(MouseButton::Left), 2, 3),
                DragOptions::new(CellOffset::new(4, -1)),
            ),
            DragPhase::Ended {
                position: Position::new(2, 3),
                moved: false,
            }
        );
        assert!(transients.is_empty());
    }

    #[test]
    fn captured_drag_tracks_offset_and_position_across_context_replacement() {
        let path = vec![ChildId::Static("drag")];
        let mut transients = HashMap::new();
        let mut capture = None;
        EventCtx::at(
            path.clone(),
            Rect::ZERO,
            &mut transients,
            PointerInputs {
                capture: Some(&mut capture),
                button: Some(MouseButton::Left),
                screen_mouse: None,
            },
        )
        .drag(
            &mouse(MouseKind::Down(MouseButton::Left), 5, 5),
            DragOptions::new(CellOffset::new(2, -1)),
        );

        let mut no_capture = None;
        let phase = EventCtx::at(
            path.clone(),
            Rect::ZERO,
            &mut transients,
            PointerInputs {
                capture: Some(&mut no_capture),
                button: None,
                screen_mouse: None,
            },
        )
        .drag(
            &mouse(MouseKind::Drag(MouseButton::Left), 9, 2),
            DragOptions::new(CellOffset::default()).start_if(false),
        );
        assert_eq!(
            phase,
            DragPhase::Moved {
                offset: CellOffset::new(6, -4),
                position: Position::new(9, 2),
            }
        );
    }

    #[test]
    fn captured_drag_ignores_unrelated_buttons_without_transient_or_capture() {
        let path = vec![ChildId::Static("drag")];
        let mut transients = HashMap::new();
        let mut capture = None;
        let mut ctx = EventCtx::at(
            path.clone(),
            Rect::ZERO,
            &mut transients,
            PointerInputs {
                capture: Some(&mut capture),
                button: Some(MouseButton::Right),
                screen_mouse: None,
            },
        );

        assert_eq!(
            ctx.drag(
                &mouse(MouseKind::Down(MouseButton::Right), 1, 1),
                DragOptions::default(),
            ),
            DragPhase::Ignored
        );
        assert!(transients.is_empty());
        assert!(capture.is_none());
    }

    /// A running gesture belongs to the button that started it. Another
    /// button's movement and release are not part of it, and must not move it
    /// or end it — the same press can be held while a second button is
    /// clicked elsewhere.
    #[test]
    fn a_running_drag_ignores_another_buttons_movement_and_release() {
        let path = vec![ChildId::Static("drag")];
        let mut transients = HashMap::new();
        let mut capture = None;
        EventCtx::at(
            path.clone(),
            Rect::ZERO,
            &mut transients,
            PointerInputs {
                capture: Some(&mut capture),
                button: Some(MouseButton::Left),
                screen_mouse: None,
            },
        )
        .drag(
            &mouse(MouseKind::Down(MouseButton::Left), 5, 5),
            DragOptions::new(CellOffset::default()),
        );

        let mut no_capture = None;
        let mut ctx = EventCtx::at(
            path.clone(),
            Rect::ZERO,
            &mut transients,
            PointerInputs {
                capture: Some(&mut no_capture),
                button: None,
                screen_mouse: None,
            },
        );
        assert_eq!(
            ctx.drag(
                &mouse(MouseKind::Drag(MouseButton::Right), 9, 2),
                DragOptions::default(),
            ),
            DragPhase::Ignored,
            "the right button is not what is being dragged"
        );
        assert_eq!(
            ctx.drag(
                &mouse(MouseKind::Up(MouseButton::Right), 9, 2),
                DragOptions::default(),
            ),
            DragPhase::Ignored,
            "and releasing it does not end the left gesture"
        );

        // The left gesture is still there, still anchored, and still counts as
        // unmoved: nothing the right button did touched it.
        assert_eq!(
            ctx.drag(
                &mouse(MouseKind::Up(MouseButton::Left), 5, 5),
                DragOptions::default(),
            ),
            DragPhase::Ended {
                position: Position::new(5, 5),
                moved: false,
            }
        );
        assert!(transients.is_empty());
    }

    #[test]
    fn drag_tracks_delta_from_the_anchor_offset() {
        // Anchored at cell (5, 5) with the offset (2, -1) current there.
        let anchor = AnchorPoint {
            column: 5,
            row: 5,
            offset: CellOffset::new(2, -1),
        };

        // Move to (8, 4): delta (+3, -1) on top of the anchor (2, -1).
        assert_eq!(anchor.offset_at(8, 4), CellOffset::new(5, -2));
        // `offset_at` is pure: asking again from the anchor cell repeats it.
        assert_eq!(anchor.offset_at(5, 5), CellOffset::new(2, -1));
    }

    #[test]
    fn clamp_keeps_the_rect_inside_the_area() {
        let area = Rect::new(0, 0, 20, 10);
        let rect = Rect::new(8, 4, 4, 2); // centered-ish box

        // A modest offset is left untouched.
        assert_eq!(
            clamp_offset(area, rect, CellOffset::new(2, 1)),
            CellOffset::new(2, 1)
        );
        // Pushing right past the edge clamps to flush-right (x: 20-4-8 = 8).
        assert_eq!(
            clamp_offset(area, rect, CellOffset::new(99, 0)).x,
            (area.width - rect.width - rect.x).cast_signed()
        );
        // Pushing left past the edge clamps to flush-left (x: -8).
        assert_eq!(
            clamp_offset(area, rect, CellOffset::new(-99, 0)).x,
            -rect.x.cast_signed()
        );
    }

    #[test]
    fn offset_u16_saturates() {
        assert_eq!(offset_u16(10, 5), 15);
        assert_eq!(offset_u16(10, -3), 7);
        assert_eq!(offset_u16(2, -9), 0);
    }
}
