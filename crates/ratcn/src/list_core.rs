//! The shared substance of components built from an ordered set of
//! value-keyed items: item identity, and a uniform-row viewport.
//!
//! [`List`](crate::List) is one presentation of such a set; a Select dropdown
//! or a menu is another. What they present differs, but what they *are* — an
//! ordered collection of value-keyed items, some disabled — is the same. That
//! substance lives here once, next to the index arithmetic in
//! [`crate::linear_nav`], so two of them can never drift apart on how items are
//! identified, how clicks map to rows, or when a list scrolls.
//!
//! The module splits along that line. [`assert_unique_values`] applies to any
//! component keyed by item value, whatever its own item type and however it is
//! laid out — [`Tabs`](crate::Tabs) uses it for a row of tabs. Everything else
//! here — [`ListItem`], [`RowViewport`], [`WheelPark`] — assumes items stacked
//! in a scrolling column of uniform-height rows.
//!
//! Nothing here paints or handles events. Components own their look, their
//! bindings, and their messages; this module answers only the questions every
//! one of them asks.

use ratatui::{
    layout::{Position, Rect},
    text::{Line, Text},
};

use crate::linear_nav::{self};

/// Items a wheel notch moves the view by, shared so two list-shaped
/// components can never scroll at different speeds.
pub const SCROLL_STEP: usize = 3;

/// Where a wheel left the view, and whether it still holds it there against
/// the cursor.
///
/// The wheel scrolls the view and leaves the cursor alone, so it can push the
/// cursor off-screen — and paint must not immediately drag it back. That needs
/// one bit of memory between frames, which no component instance can hold: a
/// fresh one is built every frame. This lives in the runtime's
/// identity-scoped transient store instead, written by event handlers and read
/// by paint, and it disappears with the component's identity (a select's park
/// dies when its popup closes).
///
/// The rules, in the order they matter:
///
/// - [`park`](Self::park) records a wheel scroll: the view moves to `offset`
///   and holds there while the cursor stays where the wheel left it.
/// - [`settle`](Self::settle) runs at the start of paint. It releases the hold
///   for good once anything has moved the cursor, and then records the offset
///   paint resolved. Releasing is permanent: without it, moving the cursor
///   away and back would revive a stale park and throw the cursor off-screen
///   again.
/// - [`cursor_to_show`](Self::cursor_to_show) tells paint whether to keep the
///   cursor visible. While the park holds, it does not — that is the whole
///   point of the wheel.
///
/// Paint settles it rather than event handling because only paint sees every
/// cursor change: a select's options are scrolled by the panel but moved by
/// the keys its trigger handles. Both steps are idempotent, so running them
/// in each of the two declaration passes reaches the same value — the
/// condition [`RenderCtx::transient_mut`](crate::runtime::RenderCtx::transient_mut)
/// imposes.
///
/// This is render-derived presentation state. The cursor, the selection, and
/// any app-bound scroll offset stay app-owned.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WheelPark {
    offset: usize,
    anchor: Option<usize>,
    parked: bool,
}

impl WheelPark {
    /// Record a wheel scroll to `offset`, holding the view there while the
    /// cursor stays at `cursor`.
    pub const fn park(&mut self, offset: usize, cursor: Option<usize>) {
        self.offset = offset;
        self.anchor = cursor;
        self.parked = true;
    }

    /// Release the hold once anything has moved the cursor away from where the
    /// wheel left it. A released park never holds again, so returning to that
    /// cursor cannot revive it. Call this before
    /// [`cursor_to_show`](Self::cursor_to_show).
    pub const fn settle(&mut self, cursor: Option<usize>) {
        if !same_index(self.anchor, cursor) {
            self.parked = false;
        }
    }

    /// Record the offset paint resolved, so the next frame starts from the
    /// view actually on screen.
    pub const fn record(&mut self, painted: usize) {
        self.offset = painted;
    }

    /// The offset to paint from, for a component that owns its own scrolling.
    /// A component with an app-bound offset reads that instead.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// The cursor paint must keep visible: `None` while the wheel still holds
    /// the view, so the cursor may sit off-screen.
    #[must_use]
    pub const fn cursor_to_show(&self, cursor: Option<usize>) -> Option<usize> {
        if self.parked && same_index(self.anchor, cursor) {
            None
        } else {
            cursor
        }
    }
}

const fn same_index(left: Option<usize>, right: Option<usize>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        (None, None) => true,
        _ => false,
    }
}

/// One row of a list-shaped component: an identifying `value` and the `label`
/// shown for it.
///
/// The `value` is what focus and selection are recorded as — not the row
/// index. That is the point: sort the list, filter it, insert a row at the
/// top, and the selected item is still the same item. Use whatever identifies
/// the thing in your domain (a `TaskId`, a `PathBuf`), not its position.
///
/// A plain `&str` or `String` converts into a `ListItem<String>` that uses the
/// text as both value and label, which is fine for short static lists where
/// the labels are unique.
///
/// Values must be unique within one component's declaration. Duplicate values
/// panic during declaration because focus, selection, and pointer actions
/// would otherwise be ambiguous — see [`assert_unique_values`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem<T> {
    value: T,
    label: String,
    disabled: bool,
}

impl<T> ListItem<T> {
    /// A row identified by `value` and displayed as `label`.
    #[must_use]
    pub fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            disabled: false,
        }
    }

    /// Dim this row and prevent user-driven focus and selection.
    ///
    /// Keyboard movement skips over it and clicks on it do nothing. Controlled
    /// state may still identify the row, so components can preserve a previously
    /// selected value while it is disabled.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// The identifying value given to [`new`](Self::new).
    #[must_use]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// The display label given to [`new`](Self::new).
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Whether [`disabled`](Self::disabled) marked this row unselectable.
    #[must_use]
    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }
}

impl From<&str> for ListItem<String> {
    fn from(label: &str) -> Self {
        Self::new(label.to_owned(), label)
    }
}

impl From<String> for ListItem<String> {
    fn from(label: String) -> Self {
        Self::new(label.clone(), label)
    }
}

/// Everything a custom row-rendering closure — [`List::render_item`] or
/// [`Select::render_item`] — knows about the row it is drawing.
///
/// [`List::render_item`]: crate::List::render_item
/// [`Select::render_item`]: crate::Select::render_item
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListItemState<'a, T> {
    /// Position in the list as declared, counting from 0.
    pub index: usize,
    /// The row's identifying value, for looking up whatever else you need to
    /// draw it.
    pub value: &'a T,
    /// The label given to [`ListItem::new`].
    pub label: &'a str,
    /// Whether this row receives the component's cursor styling. Each component
    /// decides when its cursor is visually exposed.
    pub focused: bool,
    /// This row is part of the current selection.
    pub selected: bool,
    /// This row is disabled, whether individually or because the whole
    /// component is.
    pub disabled: bool,
}

/// Force `text` to exactly `height` lines: pad short rows with blanks, drop
/// extra lines from long ones.
///
/// Uniform item height is what lets hit-testing divide a screen row by it, so a
/// `render_item` closure returning the wrong number of lines must not be able
/// to desynchronise clicks from items.
#[must_use]
pub fn fit_to_height(mut text: Text<'static>, height: u16) -> Text<'static> {
    let height = usize::from(height);
    text.lines.truncate(height);
    while text.lines.len() < height {
        text.lines.push(Line::default());
    }
    text
}

/// The index of the item whose value equals `value`, or `None` when no item
/// carries it. This is how a component turns its app-held value (the focused
/// or selected item) back into a row position each frame.
#[must_use]
pub fn index_of<T: PartialEq>(items: &[ListItem<T>], value: &T) -> Option<usize> {
    items.iter().position(|item| item.value == *value)
}

/// Whether the item at `index` is disabled. Out-of-range indices read as
/// enabled, matching how a short disabled slice reads elsewhere in the crate.
#[must_use]
pub fn disabled_at<T>(items: &[ListItem<T>], index: usize) -> bool {
    items.get(index).is_some_and(|item| item.disabled)
}

/// Panic if two of the given item values are equal.
///
/// Call it from a component's declaration-time validation (`resolve`) with an
/// iterator over the identifying values — e.g. `items.iter().map(ListItem::value)`
/// for [`ListItem`]s, or the equivalent for any other value-keyed item type.
/// Duplicate values would make focus, selection, and pointer actions
/// ambiguous — two rows would answer to the same identity — so declaration
/// fails loudly rather than one of them winning silently. `component` names
/// the caller in the panic message.
///
/// # Panics
///
/// Panics when any two values are equal.
pub fn assert_unique_values<'a, T: PartialEq + 'a>(
    values: impl IntoIterator<Item = &'a T>,
    component: &str,
) {
    let values: Vec<&T> = values.into_iter().collect();
    for (index, value) in values.iter().enumerate() {
        assert!(
            !values[index + 1..].iter().any(|other| other == value),
            "{component} item values must be unique within a {component} declaration"
        );
    }
}

/// A viewport of uniform-height rows: which item is on top, and how pointer
/// rows map back to items.
///
/// List-shaped components scroll by *item*, not by screen row, and hit-test a
/// click by pure arithmetic: divide the pointer's row by the per-item height,
/// add the offset that was painted. That only works when two things hold —
/// every item is the same height, and the offset used for hit-testing is the
/// one actually painted. This type holds both: the per-item height, and the
/// **painted offset** — the top-item index the last render actually drew,
/// recorded with [`record_painted_offset`](Self::record_painted_offset) so
/// event-time arithmetic works against what is on screen rather than against
/// app state that may be one frame newer.
///
/// The viewport is render-derived runtime state, not a second copy of
/// app-owned scroll: a component that binds scroll still reads the requested
/// offset from app state each frame and resolves it through
/// [`cursor_visible_offset`](Self::cursor_visible_offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowViewport {
    rows_per_item: u16,
    painted_offset: usize,
}

impl RowViewport {
    /// A viewport whose items each occupy `row_height` terminal rows. A height
    /// of 0 is treated as 1, so the divisions below stay safe.
    #[must_use]
    pub const fn new(row_height: u16) -> Self {
        Self {
            rows_per_item: if row_height == 0 { 1 } else { row_height },
            painted_offset: 0,
        }
    }

    /// Terminal rows each item occupies. Never 0.
    #[must_use]
    pub const fn rows_per_item(&self) -> u16 {
        self.rows_per_item
    }

    /// The top-item index the last render drew, as recorded by
    /// [`record_painted_offset`](Self::record_painted_offset). 0 before the
    /// first render.
    #[must_use]
    pub const fn painted_offset(&self) -> usize {
        self.painted_offset
    }

    /// Record the offset this frame is about to paint, so later events
    /// hit-test against it.
    pub const fn record_painted_offset(&mut self, offset: usize) {
        self.painted_offset = offset;
    }

    /// How many whole items fit in `area`. This is the unit paging and
    /// scrolling work in — screen rows are only a paint concern.
    #[must_use]
    pub const fn visible_items(&self, area: Rect) -> usize {
        (area.height / self.rows_per_item) as usize
    }

    /// The item index under the pointer at `(column, row)`, using the painted
    /// offset, or `None` outside `area` or past the last of `len` items.
    ///
    /// Several screen rows can belong to one item, so the pointer row is
    /// divided by the per-item height before the lookup — otherwise a click on
    /// an item's second line would hit the wrong item.
    #[must_use]
    pub fn row_at(&self, area: Rect, len: usize, column: u16, row: u16) -> Option<usize> {
        if !area.contains(Position { x: column, y: row }) {
            return None;
        }
        let rows_per_item = usize::from(self.rows_per_item);
        let local_row = usize::from(row - area.y);
        if local_row >= self.visible_items(area).saturating_mul(rows_per_item) {
            return None;
        }
        linear_nav::index_at_row(len, self.painted_offset, local_row / rows_per_item)
    }

    /// The offset that keeps `cursor` visible in `area`, starting from a
    /// `requested` offset — [`linear_nav::cursor_visible_offset`] measured in
    /// this viewport's items.
    #[must_use]
    pub fn cursor_visible_offset(
        &self,
        area: Rect,
        len: usize,
        requested: usize,
        cursor: Option<usize>,
    ) -> usize {
        linear_nav::cursor_visible_offset(len, self.visible_items(area), requested, cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_of_and_disabled_at_answer_by_value_and_position() {
        let items = [
            ListItem::new(1, "one"),
            ListItem::new(2, "two").disabled(true),
        ];
        assert_eq!(index_of(&items, &2), Some(1));
        assert_eq!(index_of(&items, &3), None);
        assert!(disabled_at(&items, 1));
        assert!(!disabled_at(&items, 0));
        assert!(!disabled_at(&items, 99));
    }

    #[test]
    fn duplicate_values_panic_with_the_component_name() {
        let items = [ListItem::new(1, "a"), ListItem::new(1, "b")];
        let panic = std::panic::catch_unwind(|| {
            assert_unique_values(items.iter().map(ListItem::value), "List");
        })
        .expect_err("duplicates must panic");
        let message = panic
            .downcast_ref::<String>()
            .expect("panic carries a String");
        assert_eq!(
            message,
            "List item values must be unique within a List declaration"
        );
    }

    #[test]
    fn row_at_divides_multi_row_items_and_uses_the_painted_offset() {
        let mut viewport = RowViewport::new(2);
        viewport.record_painted_offset(3);
        let area = Rect::new(0, 10, 10, 4);
        // Second screen row of the first visible item.
        assert_eq!(viewport.row_at(area, 10, 0, 11), Some(3));
        // First row of the second visible item.
        assert_eq!(viewport.row_at(area, 10, 0, 12), Some(4));
        // Outside the area, and past the last item.
        assert_eq!(viewport.row_at(area, 10, 0, 9), None);
        assert_eq!(viewport.row_at(area, 4, 0, 12), None);
    }

    #[test]
    fn row_at_excludes_the_partial_row_below_the_last_whole_item() {
        let viewport = RowViewport::new(2);
        // 5 rows fit two whole 2-row items; the dangling fifth row hits nothing.
        let area = Rect::new(0, 0, 10, 5);
        assert_eq!(viewport.visible_items(area), 2);
        assert_eq!(viewport.row_at(area, 10, 0, 4), None);
    }

    #[test]
    fn zero_row_height_is_treated_as_one() {
        let viewport = RowViewport::new(0);
        assert_eq!(viewport.rows_per_item(), 1);
    }
}
