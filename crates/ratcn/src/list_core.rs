//! The shared substance of components built from an ordered set of
//! value-keyed items: item identity, a uniform-row viewport, and what a pointer
//! gesture over one of those rows asks for.
//!
//! [`List`](crate::List) presents such a set as a column of rows, and
//! [`Select`](crate::Select) presents one as a dropdown panel. What they
//! present differs, but what they *are* — an ordered collection of value-keyed
//! items, some disabled — is the same. That
//! substance lives here once, next to the index arithmetic in
//! [`crate::linear_nav`], so two of them can never drift apart on how items are
//! identified, how clicks map to rows, or when a list scrolls.
//!
//! The module splits along that line. [`assert_unique_values`] applies to any
//! component keyed by item value, whatever its own item type and however it is
//! laid out — [`Tabs`](crate::Tabs) uses it for a row of tabs, and so is
//! [`ListItem`] itself, which a tab is. Everything else here —
//! [`RowViewport`], [`WheelHold`], [`windowed_rows`], [`row_intent`] — assumes
//! items stacked in a scrolling column of uniform-height rows.
//!
//! Nothing here paints or emits. [`row_intent`] decides what a pointer gesture
//! over a row means, but not what to say about it: components own their look,
//! their bindings, and their messages, and this module answers only the
//! questions every one of them asks.

use std::ops::Range;

use ratatui::{
    layout::{Position, Rect},
    text::{Line, Text},
};

use crate::linear_nav::{self};
use crate::runtime::{DeclareCtx, MouseButton, MouseKind};

/// Items a wheel notch moves the view by, shared so two list-shaped
/// components can never scroll at different speeds.
pub const SCROLL_STEP: usize = 3;

/// Where a wheel left the view, and which item it holds the view against.
///
/// The wheel scrolls the view and leaves the cursor alone, so it can push the
/// cursor off-screen — and paint must not immediately drag it back. That needs
/// one bit of memory between frames, which no component instance can hold: a
/// fresh one is built every frame. This lives in the runtime's
/// identity-scoped transient store instead, written by event handlers and read
/// by paint, and it disappears with the component's identity (a select's hold
/// dies when its popup closes).
///
/// The hold stands only while the list has not moved under it: the item the
/// cursor was left on is still that item, still at that row, in a list still
/// that long. Each half of that earns its place. Anchoring by row alone would
/// survive one item being swapped for another at the same position and leave the
/// new one off-screen — these components key focus and selection by value, so
/// the value is what identifies the anchor. Anchoring by value alone would
/// survive reordering, filtering, insertion, and removal, leaving a held
/// offset that says nothing about which items now sit there. Requiring all of it
/// is what makes the held offset worth honoring: while the hold stands, the row
/// it names still holds the same items it named.
///
/// The rules, in the order they matter:
///
/// - [`hold`](Self::hold) records a wheel scroll: the view moves to `offset`
///   and holds there while the list stays as it was.
/// - [`settle`](Self::settle) runs once per frame, where the component
///   declares. It releases the hold for good once anything has moved, and
///   answers with the offset that declaration paints from. Releasing is
///   permanent: without it, moving the cursor away and back would revive a
///   stale hold and throw the cursor off-screen again.
///
/// The declaration settles it rather than event handling because only the
/// declaration sees every cursor change: a select's options are scrolled by
/// the panel but moved by the keys its trigger handles. The hold is stored
/// through [`DeclareCtx::transient_mut`](crate::runtime::DeclareCtx::transient_mut),
/// so it survives between frames and the wheel's own event handler writes it
/// from the other side.
///
/// This is render-derived presentation state. The cursor, the selection, and
/// any app-bound scroll offset stay app-owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WheelHold<T> {
    offset: usize,
    /// What the list looked like where it mattered when the wheel took the hold.
    /// `None` while nothing holds the view.
    held: Option<Anchor<T>>,
}

/// The list as the wheel left it: the item under the cursor, the row it sat on,
/// and how many items there were. Captured together in [`WheelHold::hold`] so
/// no caller can assemble a half-anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Anchor<T> {
    value: T,
    index: usize,
    len: usize,
}

/// An unheld view at the top of the list: nothing holds it, so the cursor is
/// kept visible.
impl<T> Default for WheelHold<T> {
    fn default() -> Self {
        Self {
            offset: 0,
            held: None,
        }
    }
}

impl<T: Clone + PartialEq> WheelHold<T> {
    /// Record a wheel scroll to `offset`, holding the view there while `items`
    /// still reads as it does now where `cursor` sits.
    ///
    /// A `cursor` of `None` anchors nothing: with no cursor there is none to
    /// push off-screen, and the view simply starts from `offset`.
    pub fn hold(&mut self, offset: usize, items: &[ListItem<T>], cursor: Option<usize>) {
        self.offset = offset;
        self.held = cursor
            .and_then(|index| Some((items.get(index)?, index)))
            .map(|(item, index)| Anchor {
                value: item.value.clone(),
                index,
                len: items.len(),
            });
    }

    /// Settle the hold and answer with the offset this frame paints from,
    /// recorded in `viewport` so event-time hit-testing answers against what
    /// this frame paints.
    ///
    /// `cursor` is where the cursor sits among `items`, and `requested` an
    /// app-bound scroll offset — `None` for a component that owns its own
    /// scrolling, which resumes from where the wheel left the view.
    ///
    /// While the hold stands the cursor may sit off-screen, which is the whole
    /// point of the wheel. It stands only while the cursor is still on the
    /// anchored row, that row still carries the anchored item, and `items` is
    /// still the length it was. Replacing that item, reordering, filtering,
    /// inserting, removing, or moving the cursor elsewhere all release the hold
    /// and scroll the cursor back into view: a held offset is worth honoring
    /// only while it still names the rows it was held on.
    pub fn settle(
        &mut self,
        items: &[ListItem<T>],
        cursor: Option<usize>,
        requested: Option<usize>,
        viewport: &mut RowViewport,
        area: Rect,
    ) {
        if !self
            .held
            .as_ref()
            .is_some_and(|anchor| anchor.holds(items, cursor))
        {
            self.held = None;
        }
        let offset = viewport.cursor_visible_offset(
            area,
            items.len(),
            requested.unwrap_or(self.offset),
            if self.held.is_some() { None } else { cursor },
        );
        self.offset = offset;
        viewport.record_painted_offset(offset);
    }
}

impl<T: Clone + PartialEq + 'static> WheelHold<T> {
    /// [`settle`](Self::settle) the hold stored at the current declaration's
    /// identity, or an unheld view when nothing is stored there.
    ///
    /// A hold is written by a wheel event and read back by the next declaration,
    /// so a declaration that has never been wheeled finds nothing — and must
    /// still resolve the offset it paints from. Settling through here is what
    /// makes the absent hold mean "unheld" rather than "no offset", in the one
    /// place both list-shaped components reach for it.
    pub fn settle_transient<S, M>(
        ctx: &mut DeclareCtx<'_, S, M>,
        items: &[ListItem<T>],
        cursor: Option<usize>,
        requested: Option<usize>,
        viewport: &mut RowViewport,
        area: Rect,
    ) {
        let mut unheld = Self::default();
        ctx.transient_mut::<Self>()
            .unwrap_or(&mut unheld)
            .settle(items, cursor, requested, viewport, area);
    }
}

impl<T: PartialEq> Anchor<T> {
    /// Whether the list still reads as it did where this anchor was taken, with
    /// the cursor still on it.
    fn holds(&self, items: &[ListItem<T>], cursor: Option<usize>) -> bool {
        cursor == Some(self.index)
            && items.len() == self.len
            && items
                .get(self.index)
                .is_some_and(|item| item.value == self.value)
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
/// Values must be unique within one component's declaration, or focus,
/// selection, and pointer actions are ambiguous — two rows would answer to the
/// same identity. A debug build panics on duplicates during declaration; see
/// [`assert_unique_values`] for why the check stops there.
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
    /// state may still name the row, so a component keeps a selected value
    /// while it is disabled.
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

/// Everything a custom row-rendering closure — [`List::paint_item`] or
/// [`Select::paint_item`] — knows about the row it is drawing.
///
/// [`List::paint_item`]: crate::List::paint_item
/// [`Select::paint_item`]: crate::Select::paint_item
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
/// `paint_item` closure returning the wrong number of lines must not be able
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

/// Map the window `rows` of `items` to the lines a widget paints, each forced to
/// `row_height` lines.
///
/// `row` is handed each item with its index *in the whole list*, never in the
/// window: focus, selection, and the arithmetic that turns a click's screen row
/// back into an item all count from the start of the list, so a windowed paint
/// that renumbered its rows would light up and answer for the wrong one. Both
/// list-shaped components build their painted rows through here, so neither can
/// renumber them.
///
/// The result is one row per item in `rows`, each exactly `row_height` lines
/// tall — see [`fit_to_height`] for why that is not the closure's choice.
///
/// # Panics
///
/// Panics if `rows` is not within `items`, as any slice would.
#[must_use]
pub fn windowed_rows<T>(
    items: &[ListItem<T>],
    rows: Range<usize>,
    row_height: u16,
    mut row: impl FnMut(usize, &ListItem<T>) -> Text<'static>,
) -> Vec<Text<'static>> {
    let first = rows.start;
    items[rows]
        .iter()
        .enumerate()
        .map(|(position, item)| fit_to_height(row(first + position, item), row_height))
        .collect()
}

/// What a pointer gesture over a column of value-keyed rows asks for.
///
/// The answer [`row_intent`] gives, kept separate from any component's messages
/// so both list-shaped components can reach the same decision and then emit
/// their own bindings from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowIntent {
    /// A primary press landed on a disabled row: consume it. The runtime focuses
    /// an unhandled press on the component it hit, and dead content must not be
    /// a way to focus the control through it.
    BlockPress,
    /// Move the cursor onto this row.
    Focus(usize),
    /// Commit this row.
    Commit(usize),
    /// Motion within the row the cursor already sits on: answered, with nothing
    /// to emit.
    Stay,
    /// Nothing here answers this gesture; let it bubble.
    Bubble,
}

/// What a pointer gesture asks of a column of rows: the shared decision behind
/// [`List`](crate::List)'s rows and a Select panel's options.
///
/// `kind` is the gesture, `row` the item the pointer is over (`None` off the
/// rows — see [`RowViewport::row_at`]), `cursor` where the cursor sits, and
/// `commits` whether the control has a selection binding to commit to.
///
/// The rules, whichever control asks:
///
/// - A disabled row answers nothing, except that a primary press on one is
///   consumed ([`RowIntent::BlockPress`]).
/// - Motion onto another row moves the cursor; motion within the cursor's own
///   row is [`RowIntent::Stay`]. One cursor, moved by keys and pointer alike.
/// - A primary click commits its row, or moves the cursor there when there is
///   nothing to commit to — the same thing hovering already does.
/// - Everything else bubbles: a press on an enabled row (the runtime's to turn
///   into focus), a release, a drag, a non-primary button, a wheel notch (which
///   scrolls the view rather than addressing a row, so each control handles it
///   before asking).
///
/// A gesture that hit no row bubbles. A control whose footprint is opaque to the
/// pointer consumes that itself; this cannot know.
#[must_use]
pub fn row_intent<T>(
    kind: MouseKind,
    items: &[ListItem<T>],
    row: Option<usize>,
    cursor: Option<usize>,
    commits: bool,
) -> RowIntent {
    let Some(index) = row else {
        return RowIntent::Bubble;
    };
    let disabled = disabled_at(items, index);
    match kind {
        MouseKind::Down(MouseButton::Left) if disabled => RowIntent::BlockPress,
        MouseKind::Moved | MouseKind::Click(MouseButton::Left) if disabled => RowIntent::Bubble,
        MouseKind::Moved if cursor == Some(index) => RowIntent::Stay,
        MouseKind::Click(MouseButton::Left) if commits => RowIntent::Commit(index),
        MouseKind::Moved | MouseKind::Click(MouseButton::Left) => RowIntent::Focus(index),
        _ => RowIntent::Bubble,
    }
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
/// Call it from a component's declaration-time validation (`prepare`) with an
/// iterator over the identifying values — e.g. `items.iter().map(ListItem::value)`
/// for [`ListItem`]s, or the equivalent for any other value-keyed item type.
/// Duplicate values make focus, selection, and pointer actions ambiguous — two
/// rows answer to the same identity — so declaration panics. `component` names
/// the caller in the panic message.
///
/// Item values are only [`PartialEq`], so there is nothing to sort or hash by
/// and the scan is quadratic: every value is compared with every later one.
/// Components declare a fresh instance every frame, which is why the built-in
/// ones guard this call with [`cfg!(debug_assertions)`](cfg) — the answer
/// cannot change unless the items do, and a release build should not re-derive
/// it sixty times a second.
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
    fn a_hold_persists_only_while_its_item_stays_put() {
        let numbered = |values: [u8; 6]| values.map(|value| ListItem::new(value, "row"));
        // Two items fit, so a hold at 4 leaves a cursor near the top off-screen.
        let area = Rect::new(0, 0, 10, 2);
        let mut viewport = RowViewport::new(1);
        let unchanged = numbered([1, 2, 3, 4, 5, 6]);
        let mut hold = WheelHold::default();

        hold.hold(4, &unchanged, Some(0));
        hold.settle(&unchanged, Some(0), None, &mut viewport, area);
        assert_eq!(
            viewport.painted_offset(),
            4,
            "nothing moved, so the wheel keeps the view where it left it"
        );

        // The anchored item is still in the list, at another row: the held
        // offset no longer names the rows it was held on.
        hold.hold(4, &unchanged, Some(0));
        hold.settle(
            &numbered([2, 3, 1, 4, 5, 6]),
            Some(2),
            None,
            &mut viewport,
            area,
        );
        assert_eq!(
            viewport.painted_offset(),
            2,
            "reordering releases the hold and shows the cursor"
        );

        // Another item at the anchored row.
        hold.hold(4, &unchanged, Some(0));
        hold.settle(
            &numbered([7, 2, 3, 4, 5, 6]),
            Some(0),
            None,
            &mut viewport,
            area,
        );
        assert_eq!(
            viewport.painted_offset(),
            0,
            "a different item under the cursor releases the hold"
        );
    }

    /// A hold names a row number, which only means something in a list of the
    /// same length. Filtering an item out from under it releases the hold and
    /// brings the cursor back into view.
    #[test]
    fn filtering_the_list_under_a_hold_brings_the_cursor_back() {
        let area = Rect::new(0, 0, 10, 2);
        let mut viewport = RowViewport::new(1);
        let long: Vec<ListItem<u8>> = (0..60).map(|value| ListItem::new(value, "row")).collect();
        let mut hold = WheelHold::default();

        hold.hold(45, &long, Some(0));
        hold.settle(&long, Some(0), None, &mut viewport, area);
        assert_eq!(viewport.painted_offset(), 45, "the wheel holds the view");

        // The filter keeps the cursor's own item exactly where it was and drops
        // half the tail, so only the length gives the change away.
        let filtered: Vec<ListItem<u8>> = long.iter().take(30).cloned().collect();
        hold.settle(&filtered, Some(0), None, &mut viewport, area);
        assert_eq!(
            viewport.painted_offset(),
            0,
            "the cursor comes back into view instead of the view clamping to the new tail"
        );
    }

    /// The whole shared pointer policy, as one table: both list-shaped
    /// components answer from this, so a change here is a change to both.
    #[test]
    fn row_intent_answers_each_gesture_over_a_row() {
        let items = [
            ListItem::new(0, "a"),
            ListItem::new(1, "b"),
            ListItem::new(2, "c").disabled(true),
        ];
        let cursor = Some(1);
        let down = MouseKind::Down(MouseButton::Left);
        let click = MouseKind::Click(MouseButton::Left);

        // A press only ever blocks a disabled row; on an enabled one it is the
        // runtime's to turn into focus.
        assert_eq!(
            row_intent(down, &items, Some(2), cursor, true),
            RowIntent::BlockPress
        );
        assert_eq!(
            row_intent(down, &items, Some(0), cursor, true),
            RowIntent::Bubble
        );
        // Motion moves the one cursor, and rests on the row it is already on.
        assert_eq!(
            row_intent(MouseKind::Moved, &items, Some(0), cursor, true),
            RowIntent::Focus(0)
        );
        assert_eq!(
            row_intent(MouseKind::Moved, &items, Some(1), cursor, true),
            RowIntent::Stay
        );
        assert_eq!(
            row_intent(MouseKind::Moved, &items, Some(2), cursor, true),
            RowIntent::Bubble,
            "a disabled row is not somewhere the cursor may go"
        );
        // A click commits, or moves the cursor when there is nothing to commit.
        assert_eq!(
            row_intent(click, &items, Some(0), cursor, true),
            RowIntent::Commit(0)
        );
        assert_eq!(
            row_intent(click, &items, Some(0), cursor, false),
            RowIntent::Focus(0)
        );
        assert_eq!(
            row_intent(click, &items, Some(2), cursor, true),
            RowIntent::Bubble
        );
        // Off the rows, and gestures a column does not answer at all.
        assert_eq!(
            row_intent(click, &items, None, cursor, true),
            RowIntent::Bubble
        );
        for kind in [
            MouseKind::Up(MouseButton::Left),
            MouseKind::Drag(MouseButton::Left),
            MouseKind::Down(MouseButton::Right),
            MouseKind::Click(MouseButton::Middle),
        ] {
            assert_eq!(
                row_intent(kind, &items, Some(0), cursor, true),
                RowIntent::Bubble,
                "{kind:?} is not a column's gesture"
            );
        }
    }

    /// A windowed paint numbers its rows by their position in the whole list, or
    /// the row a click lands on is not the row that was drawn there.
    #[test]
    fn windowed_rows_hands_out_global_indices_and_a_uniform_height() {
        let items: Vec<ListItem<u8>> = (0..6).map(|value| ListItem::new(value, "row")).collect();
        let rows = windowed_rows(&items, 2..5, 2, |index, item| {
            Text::from(format!("{index}:{}", item.value()))
        });

        assert_eq!(rows.len(), 3);
        assert!(
            rows.iter().all(|row| row.lines.len() == 2),
            "every row is padded to the declared height"
        );
        assert_eq!(
            rows.iter()
                .map(|row| row.lines[0].to_string())
                .collect::<Vec<_>>(),
            ["2:2", "3:3", "4:4"],
            "the window's third item is item 4, not item 2"
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
