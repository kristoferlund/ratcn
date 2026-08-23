//! The index arithmetic behind moving through an ordered list of things.
//!
//! Every control with items in a row or column — [`List`](crate::List),
//! [`Select`](crate::Select), and [`Tabs`](crate::Tabs) — needs the same
//! handful of answers. Where does Down go from here? What if the next three
//! items are disabled? Where does Page Down land near the end? Getting those
//! subtly wrong in each control separately is how a UI ends up feeling
//! inconsistent, so the math lives here once.
//!
//! These are pure functions over plain numbers. They hold no state, do no
//! hit-testing, and know nothing about rendering: a caller that wants to map a
//! click to an item checks the [`Rect`](ratatui::layout::Rect) itself and
//! subtracts the area's origin before calling [`index_at_row`]. Which item is
//! focused, what is selected, and where the view is scrolled all stay in app
//! state, stored however the app prefers.

use crate::runtime::{KeyCode, KeyEvent, ScrollDirection, Step};

/// The next *enabled* index after moving one step from `from` toward
/// `direction`, skipping indices for which `disabled` holds. Clamps at the last
/// enabled index in that direction — no wrap — and returns `from` unchanged when
/// there is no other enabled index to move to. `from` itself need not be
/// enabled, so a parked selection on a disabled item still steps to a neighbor.
///
/// This is the one-step move for controls whose items can be individually
/// disabled, such as tabs and lists.
#[must_use]
pub fn step_enabled(
    len: usize,
    from: usize,
    direction: Step,
    disabled: impl Fn(usize) -> bool,
) -> usize {
    let next = match direction {
        Step::Forward => (from.saturating_add(1)..len).find(|&i| !disabled(i)),
        Step::Backward => (0..from.min(len)).rev().find(|&i| !disabled(i)),
    };
    next.unwrap_or(from)
}

/// The first enabled index in `0..len` (the target of a Home key), or `None`
/// when every item is disabled or the collection is empty.
#[must_use]
pub fn first_enabled(len: usize, disabled: impl Fn(usize) -> bool) -> Option<usize> {
    (0..len).find(|&i| !disabled(i))
}

/// The last enabled index in `0..len` (the target of an End key), or `None`
/// when every item is disabled or the collection is empty.
#[must_use]
pub fn last_enabled(len: usize, disabled: impl Fn(usize) -> bool) -> Option<usize> {
    (0..len).rev().find(|&i| !disabled(i))
}

/// Is there any index a cursor could land on — that is, does `0..len` hold an
/// enabled one?
///
/// The focusability question every item control asks: an empty control, or one
/// whose every item is disabled, has nothing for a cursor to sit on and so is
/// not a focus stop. Answered from [`first_enabled`], so the two cannot
/// disagree about what "enabled somewhere" means.
#[must_use]
pub fn has_enabled(len: usize, disabled: impl Fn(usize) -> bool) -> bool {
    first_enabled(len, disabled).is_some()
}

/// Move `page_size` physical rows from `from`, landing on an enabled item. At
/// either end, this clamps to the furthest enabled item in that direction.
/// An out-of-range `from` is clamped before movement.
#[must_use]
fn page_enabled(
    len: usize,
    from: usize,
    direction: Step,
    page_size: usize,
    disabled: impl Fn(usize) -> bool,
) -> usize {
    if len == 0 {
        return from;
    }
    let from = from.min(len - 1);
    let target = match direction {
        Step::Forward => from.saturating_add(page_size).min(len - 1),
        Step::Backward => from.saturating_sub(page_size),
    };
    match direction {
        Step::Forward => (target..len)
            .find(|&index| !disabled(index))
            .or_else(|| last_enabled(len, disabled))
            .unwrap_or(from),
        Step::Backward => (0..=target)
            .rev()
            .find(|&index| !disabled(index))
            .or_else(|| first_enabled(len, disabled))
            .unwrap_or(from),
    }
}

/// Where a navigation key lands, resolved by [`nav_key_target`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavOutcome {
    /// The cursor moves to this index.
    Move(usize),
    /// The key was a navigation key but the cursor is already where it would
    /// land (Up at the top, Home on the first item). Controls usually consume
    /// the key without emitting anything.
    Stay,
}

/// How far a recognised navigation key moves the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavMove {
    /// One item.
    Step(Step),
    /// All the way to the first or last enabled item.
    Edge(Step),
    /// A viewport's worth of items.
    Page(Step),
    /// Half a viewport, the granularity Ctrl+D and Ctrl+U carry from `vi`.
    HalfPage(Step),
}

/// Which way a control's items run, and so which keys move along them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Items stacked in a column: Up/Down, `k`/`j`, and the page keys.
    Vertical,
    /// Items in a row: Left/Right and `h`/`l`. A row has no pages.
    Horizontal,
}

/// The movement a key asks for along `axis`, or `None` if it is not a
/// navigation key there.
///
/// This is the whole key map in one place, and the reason it takes a
/// [`KeyEvent`] rather than a [`KeyCode`]: a third of it is modifier chords.
///
/// Alt never navigates — it belongs to the app. Neither does Shift: `J` and `K`
/// are not `j` and `k`, and leaving Shift free keeps range-selection available
/// later without changing what any key means today.
fn nav_move(key: KeyEvent, axis: Axis) -> Option<NavMove> {
    if key.modifiers.alt || key.modifiers.shift {
        return None;
    }
    if key.modifiers.ctrl {
        // The readline and `vi` chords. They are here rather than left to the
        // app because every control with a cursor wants the same four.
        return match (key.code, axis) {
            (KeyCode::Char('n'), _) => Some(NavMove::Step(Step::Forward)),
            (KeyCode::Char('p'), _) => Some(NavMove::Step(Step::Backward)),
            (KeyCode::Char('d'), Axis::Vertical) => Some(NavMove::HalfPage(Step::Forward)),
            (KeyCode::Char('u'), Axis::Vertical) => Some(NavMove::HalfPage(Step::Backward)),
            _ => None,
        };
    }
    match (key.code, axis) {
        (KeyCode::Up | KeyCode::Char('k'), Axis::Vertical)
        | (KeyCode::Left | KeyCode::Char('h'), Axis::Horizontal) => {
            Some(NavMove::Step(Step::Backward))
        }
        (KeyCode::Down | KeyCode::Char('j'), Axis::Vertical)
        | (KeyCode::Right | KeyCode::Char('l'), Axis::Horizontal) => {
            Some(NavMove::Step(Step::Forward))
        }
        (KeyCode::Home, _) => Some(NavMove::Edge(Step::Backward)),
        (KeyCode::End, _) => Some(NavMove::Edge(Step::Forward)),
        (KeyCode::PageUp, Axis::Vertical) => Some(NavMove::Page(Step::Backward)),
        (KeyCode::PageDown, Axis::Vertical) => Some(NavMove::Page(Step::Forward)),
        _ => None,
    }
}

/// Does this key step a cursor by exactly one item along `axis` — an arrow,
/// its `vi` letter, or Ctrl+N/Ctrl+P?
///
/// A collapsed control asks this to decide whether a key should open it: what
/// would move a cursor should first reveal the cursor.
#[must_use]
pub fn is_step_key(key: KeyEvent, axis: Axis) -> bool {
    matches!(nav_move(key, axis), Some(NavMove::Step(_)))
}

/// Resolve one navigation key against a cursor over `len` items along `axis`,
/// skipping disabled indices.
///
/// This is the key map every item control shares:
///
/// | Keys | Moves |
/// |---|---|
/// | Up / Down, `k` / `j` (vertical); Left / Right, `h` / `l` (horizontal); Ctrl+P / Ctrl+N | one item |
/// | Home / End | to the first / last enabled item |
/// | `PageUp` / `PageDown` (vertical only) | one viewport (`page_size` items) |
/// | Ctrl+U / Ctrl+D (vertical only) | half a viewport |
///
/// Three sets of names for the same movements: arrows for everyone, `hjkl` for
/// `vi`, and the Ctrl chords readline put in every shell and text field. None
/// of them collide, so a control can offer all three and let the user reach for
/// whichever they already know.
///
/// Commit keys (Enter, Space) are not navigation and stay with the calling
/// control, which decides what a commit means.
///
/// - `None` — not a navigation key, or no enabled item exists to land on;
///   ignore the key.
/// - `Some(NavOutcome::Move(index))` — move the cursor there.
/// - `Some(NavOutcome::Stay)` — a navigation key with nowhere new to go;
///   consume it.
///
/// A `cursor` of `None` means the cursor is nowhere yet; any navigation key
/// then targets the first enabled item.
#[must_use]
pub fn nav_key_target(
    key: KeyEvent,
    axis: Axis,
    len: usize,
    cursor: Option<usize>,
    page_size: usize,
    disabled: impl Fn(usize) -> bool,
) -> Option<NavOutcome> {
    let movement = nav_move(key, axis)?;
    // A list with nothing enabled does not navigate, cursor or no cursor. This
    // is asked before any movement because the index helpers below answer in
    // plain indices: with everything disabled they hand back `from`, which
    // would read as `Stay` — a key consumed by a control that cannot move.
    let first = first_enabled(len, &disabled)?;
    let Some(cursor) = cursor else {
        return Some(NavOutcome::Move(first));
    };
    // Half a page still moves at least one item, so Ctrl+D in a one-row
    // viewport behaves like Down rather than doing nothing.
    let half = (page_size / 2).max(1);
    let target = match movement {
        NavMove::Step(direction) => step_enabled(len, cursor, direction, &disabled),
        NavMove::Edge(Step::Backward) => first,
        NavMove::Edge(Step::Forward) => last_enabled(len, &disabled).unwrap_or(first),
        NavMove::Page(direction) => page_enabled(len, cursor, direction, page_size, &disabled),
        NavMove::HalfPage(direction) => page_enabled(len, cursor, direction, half, &disabled),
    };
    Some(if target == cursor {
        NavOutcome::Stay
    } else {
        NavOutcome::Move(target)
    })
}

/// The scroll offset that keeps `cursor` visible in a `viewport_height`-row
/// viewport, starting from a `requested` offset.
///
/// The requested offset is first clamped to the last full page. A cursor above
/// the viewport pulls the offset up to itself; a cursor below pulls the offset
/// just far enough that the cursor becomes the last visible row; a cursor
/// already visible (or `None`) leaves the clamped offset alone. This is the
/// single keep-cursor-visible policy — every scrolling control computes its
/// painted offset through it, so two controls can never disagree about when a
/// list scrolls.
#[must_use]
pub fn cursor_visible_offset(
    len: usize,
    viewport_height: usize,
    requested: usize,
    cursor: Option<usize>,
) -> usize {
    let offset = clamp_scroll_offset(len, viewport_height, requested);
    let Some(cursor) = cursor else {
        return offset;
    };
    if viewport_height == 0 {
        offset
    } else if cursor < offset {
        cursor
    } else if cursor >= offset.saturating_add(viewport_height) {
        cursor.saturating_add(1).saturating_sub(viewport_height)
    } else {
        offset
    }
}

/// Clamp a scroll `offset` to `0..=len.saturating_sub(viewport_height)`, so
/// the last page of `len` rows never scrolls past the bottom of a
/// `viewport_height`-row viewport.
#[must_use]
fn clamp_scroll_offset(len: usize, viewport_height: usize, offset: usize) -> usize {
    offset.min(len.saturating_sub(viewport_height))
}

/// The scroll offset after moving one wheel notch by `step` rows from
/// `current`, clamped to the last full page. `None` for a horizontal notch,
/// which a column of rows does not answer.
#[must_use]
pub fn wheel_offset(
    len: usize,
    viewport_height: usize,
    current: usize,
    direction: ScrollDirection,
    step: usize,
) -> Option<usize> {
    let next = match direction {
        ScrollDirection::Up => current.saturating_sub(step),
        ScrollDirection::Down => current.saturating_add(step),
        ScrollDirection::Left | ScrollDirection::Right => return None,
    };
    Some(clamp_scroll_offset(len, viewport_height, next))
}

/// The item index for a `local_row` (a row already known to be inside the
/// control's viewport, e.g. `row - area.y`) given `scroll_offset` (the index
/// of the top visible row). `None` past the last item. The caller owns the
/// `Rect.contains(...)` gate and the row-within-area subtraction — this is
/// pure index math, not a hit-test.
#[must_use]
pub fn index_at_row(len: usize, scroll_offset: usize, local_row: usize) -> Option<usize> {
    let index = scroll_offset.checked_add(local_row)?;
    (index < len).then_some(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Modifiers;

    #[test]
    fn step_enabled_skips_disabled_indices() {
        // Items 1 and 2 disabled; forward from 0 lands on 3, not 1.
        let disabled = |i: usize| i == 1 || i == 2;
        assert_eq!(step_enabled(4, 0, Step::Forward, disabled), 3);
        assert_eq!(step_enabled(4, 3, Step::Backward, disabled), 0);
    }

    #[test]
    fn step_enabled_stays_put_at_the_edge() {
        let none_disabled = |_: usize| false;
        assert_eq!(step_enabled(3, 2, Step::Forward, none_disabled), 2);
        assert_eq!(step_enabled(3, 0, Step::Backward, none_disabled), 0);
        // Only enabled item: no move in either direction.
        let all_but_one = |i: usize| i != 1;
        assert_eq!(step_enabled(3, 1, Step::Forward, all_but_one), 1);
        assert_eq!(step_enabled(3, 1, Step::Backward, all_but_one), 1);
    }

    #[test]
    fn step_enabled_steps_off_a_parked_disabled_index() {
        // `from` is disabled; a step still finds the nearest enabled neighbor.
        let disabled = |i: usize| i == 1;
        assert_eq!(step_enabled(3, 1, Step::Forward, disabled), 2);
        assert_eq!(step_enabled(3, 1, Step::Backward, disabled), 0);
    }

    #[test]
    fn first_and_last_enabled_find_the_edges_or_none() {
        let disabled = |i: usize| i == 0 || i == 3;
        assert_eq!(first_enabled(4, disabled), Some(1));
        assert_eq!(last_enabled(4, disabled), Some(2));
        assert_eq!(first_enabled(0, |_| false), None);
        assert_eq!(first_enabled(3, |_| true), None);
        assert_eq!(last_enabled(3, |_| true), None);
    }

    #[test]
    fn page_enabled_moves_by_rows_and_clamps_to_enabled_edges() {
        let disabled = |i: usize| i == 0 || i == 4;
        assert_eq!(page_enabled(5, 1, Step::Forward, 3, disabled), 3);
        assert_eq!(page_enabled(5, 3, Step::Backward, 3, disabled), 1);
    }

    #[test]
    fn page_enabled_clamps_out_of_range_origins_before_scanning() {
        let in_range = |index: usize| {
            assert!(index < 3);
            false
        };

        assert_eq!(page_enabled(3, usize::MAX, Step::Forward, 1, in_range), 2);
        assert_eq!(page_enabled(3, usize::MAX, Step::Backward, 1, in_range), 1);
    }

    #[test]
    fn clamp_scroll_offset_caps_at_the_viewport_aware_maximum() {
        // 10 items, 4-row viewport: max offset is 6.
        assert_eq!(clamp_scroll_offset(10, 4, 0), 0);
        assert_eq!(clamp_scroll_offset(10, 4, 6), 6);
        assert_eq!(clamp_scroll_offset(10, 4, 99), 6);
        // No viewport height: clamps to the last item.
        assert_eq!(clamp_scroll_offset(10, 0, 99), 10);
    }

    #[test]
    fn wheel_offset_clamps_at_zero_and_the_viewport_aware_maximum() {
        // 10 items, 4-row viewport: max offset is 6.
        assert_eq!(wheel_offset(10, 4, 0, ScrollDirection::Up, 3), Some(0));
        assert_eq!(wheel_offset(10, 4, 5, ScrollDirection::Down, 3), Some(6));
        assert_eq!(wheel_offset(10, 4, 2, ScrollDirection::Down, 3), Some(5));
        assert_eq!(wheel_offset(10, 4, 2, ScrollDirection::Up, 3), Some(0));
    }

    #[test]
    fn wheel_offset_with_no_viewport_height_clamps_to_the_last_item() {
        assert_eq!(wheel_offset(10, 0, 0, ScrollDirection::Down, 100), Some(10));
    }

    #[test]
    fn wheel_offset_saturates_before_clamping() {
        assert_eq!(
            wheel_offset(10, 4, usize::MAX, ScrollDirection::Down, 1),
            Some(6)
        );
    }

    #[test]
    fn wheel_offset_clamps_an_out_of_range_current_offset() {
        assert_eq!(wheel_offset(10, 4, 99, ScrollDirection::Up, 1), Some(6));
    }

    #[test]
    fn wheel_offset_declines_horizontal_notches() {
        assert_eq!(wheel_offset(10, 4, 2, ScrollDirection::Left, 1), None);
        assert_eq!(wheel_offset(10, 4, 2, ScrollDirection::Right, 1), None);
    }

    #[test]
    fn index_at_row_respects_scroll_offset_and_item_count() {
        // First visible row, no scroll.
        assert_eq!(index_at_row(10, 0, 0), Some(0));
        // Third visible local row, with a scroll offset of 4.
        assert_eq!(index_at_row(10, 4, 2), Some(6));
        // Past the last item.
        assert_eq!(index_at_row(10, 8, 2), None);
    }

    #[test]
    fn index_at_row_returns_none_on_overflow() {
        assert_eq!(index_at_row(usize::MAX, usize::MAX, 1), None);
    }

    #[test]
    fn nav_key_target_maps_each_navigation_key() {
        let none = |_: usize| false;
        assert_eq!(
            nav_key_target(KeyCode::Down.into(), Axis::Vertical, 5, Some(1), 2, none),
            Some(NavOutcome::Move(2))
        );
        assert_eq!(
            nav_key_target(KeyCode::Up.into(), Axis::Vertical, 5, Some(1), 2, none),
            Some(NavOutcome::Move(0))
        );
        assert_eq!(
            nav_key_target(KeyCode::Home.into(), Axis::Vertical, 5, Some(3), 2, none),
            Some(NavOutcome::Move(0))
        );
        assert_eq!(
            nav_key_target(KeyCode::End.into(), Axis::Vertical, 5, Some(3), 2, none),
            Some(NavOutcome::Move(4))
        );
        assert_eq!(
            nav_key_target(
                KeyCode::PageDown.into(),
                Axis::Vertical,
                5,
                Some(0),
                2,
                none
            ),
            Some(NavOutcome::Move(2))
        );
        assert_eq!(
            nav_key_target(KeyCode::PageUp.into(), Axis::Vertical, 5, Some(4), 2, none),
            Some(NavOutcome::Move(2))
        );
    }

    /// A row answers Left/Right and `h`/`l` where a column answers Up/Down and
    /// `j`/`k`; the chords and Home/End are shared, and a row has no pages.
    #[test]
    fn the_horizontal_axis_swaps_the_arrows_and_drops_the_page_keys() {
        let none = |_: usize| false;
        let ctrl = |code: char| KeyEvent {
            code: KeyCode::Char(code),
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        };
        for (key, vertical, horizontal) in [
            (
                KeyEvent::new(KeyCode::Right),
                None,
                Some(NavOutcome::Move(2)),
            ),
            (
                KeyEvent::new(KeyCode::Char('l')),
                None,
                Some(NavOutcome::Move(2)),
            ),
            (
                KeyEvent::new(KeyCode::Char('h')),
                None,
                Some(NavOutcome::Move(0)),
            ),
            (
                KeyEvent::new(KeyCode::Down),
                Some(NavOutcome::Move(2)),
                None,
            ),
            (
                KeyEvent::new(KeyCode::Char('j')),
                Some(NavOutcome::Move(2)),
                None,
            ),
            (
                KeyEvent::new(KeyCode::PageDown),
                Some(NavOutcome::Move(3)),
                None,
            ),
            (ctrl('d'), Some(NavOutcome::Move(2)), None),
            (
                ctrl('n'),
                Some(NavOutcome::Move(2)),
                Some(NavOutcome::Move(2)),
            ),
            (
                KeyEvent::new(KeyCode::End),
                Some(NavOutcome::Move(4)),
                Some(NavOutcome::Move(4)),
            ),
        ] {
            assert_eq!(
                nav_key_target(key, Axis::Vertical, 5, Some(1), 2, none),
                vertical,
                "{key:?} on a column"
            );
            assert_eq!(
                nav_key_target(key, Axis::Horizontal, 5, Some(1), 2, none),
                horizontal,
                "{key:?} on a row"
            );
        }
    }

    #[test]
    fn nav_key_target_with_nowhere_new_to_go_stays() {
        let none = |_: usize| false;
        assert_eq!(
            nav_key_target(KeyCode::Up.into(), Axis::Vertical, 3, Some(0), 1, none),
            Some(NavOutcome::Stay)
        );
        assert_eq!(
            nav_key_target(KeyCode::Home.into(), Axis::Vertical, 3, Some(0), 1, none),
            Some(NavOutcome::Stay)
        );
    }

    #[test]
    fn nav_key_target_without_a_cursor_lands_on_the_first_enabled_item() {
        let disabled = |i: usize| i == 0;
        assert_eq!(
            nav_key_target(KeyCode::Down.into(), Axis::Vertical, 3, None, 1, disabled),
            Some(NavOutcome::Move(1))
        );
        assert_eq!(
            nav_key_target(KeyCode::End.into(), Axis::Vertical, 3, None, 1, disabled),
            Some(NavOutcome::Move(1))
        );
    }

    #[test]
    fn nav_key_target_ignores_commit_keys_and_all_disabled_lists() {
        assert_eq!(
            nav_key_target(KeyCode::Enter.into(), Axis::Vertical, 3, Some(0), 1, |_| {
                false
            }),
            None
        );
        assert_eq!(
            nav_key_target(KeyCode::Down.into(), Axis::Vertical, 3, None, 1, |_| true),
            None
        );
        assert_eq!(
            nav_key_target(KeyCode::Home.into(), Axis::Vertical, 3, Some(1), 1, |_| {
                true
            }),
            None
        );
    }

    #[test]
    fn nav_key_target_never_stays_in_an_all_disabled_list() {
        // A cursor parked in a list with nothing enabled has nowhere to go, and
        // `Stay` would be a lie: it tells the caller to consume the key. Every
        // movement must decline instead, not just the Home/End pair whose index
        // helpers happen to answer `None`.
        for key in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::Home,
            KeyCode::End,
        ] {
            assert_eq!(
                nav_key_target(key.into(), Axis::Vertical, 3, Some(1), 2, |_| true),
                None,
                "{key:?} in an all-disabled list"
            );
        }
        for code in ['n', 'p', 'd', 'u'] {
            let key = KeyEvent {
                code: KeyCode::Char(code),
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::NONE
                },
            };
            assert_eq!(
                nav_key_target(key, Axis::Vertical, 3, Some(1), 2, |_| true),
                None,
                "Ctrl+{code} in an all-disabled list"
            );
        }
    }

    #[test]
    fn cursor_visible_offset_pulls_the_viewport_to_the_cursor() {
        // 10 items, 4-row viewport.
        assert_eq!(cursor_visible_offset(10, 4, 0, None), 0);
        // Cursor above the viewport pulls the offset up to it.
        assert_eq!(cursor_visible_offset(10, 4, 5, Some(2)), 2);
        // Cursor below pulls it just far enough to become the last row.
        assert_eq!(cursor_visible_offset(10, 4, 0, Some(6)), 3);
        // Cursor already visible leaves the clamped offset alone.
        assert_eq!(cursor_visible_offset(10, 4, 2, Some(3)), 2);
        // Requested offset past the end clamps first.
        assert_eq!(cursor_visible_offset(10, 4, 99, None), 6);
    }

    #[test]
    fn cursor_visible_offset_with_no_viewport_keeps_the_clamped_offset() {
        assert_eq!(cursor_visible_offset(10, 0, 3, Some(7)), 3);
    }
}
