use std::fmt;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Span, Text},
    widgets::Widget,
};

use crate::Theme;
use crate::color::{DISABLED_DIM, FIELD_FOCUS_LIGHTEN, FIELD_HOVER_LIGHTEN, dim, lighten};
use crate::linear_nav::{self, NavOutcome, ScrollStep};
use crate::list_core::{
    self, ListItem, ListItemState, RowIntent, RowViewport, SCROLL_STEP, WheelPark,
};
use crate::runtime::{
    Component, Event, EventCtx, EventResult, KeyCode, KeyEvent, MouseKind, PaintCtx, RenderCtx,
    ScrollDirection,
};
use crate::selection_indicator;
use crate::text_width::display_width;
use crate::theme::resolve_style;

const ROW_FOCUS_LIGHTEN: u16 = 15;

/// Every color a list can paint.
///
/// Two different meanings of "focused" appear in these names, and getting them
/// straight is most of understanding this struct:
///
/// - **The list itself has focus** — the whole widget is the active control.
///   That picks the list's own backdrop: `focused_background` instead of
///   `background`.
/// - **The list is hovered** — `hovered_background` replaces either backdrop,
///   so moving the pointer remains visible even when the list already has focus.
/// - **A row is the focused row** — the cursor is on it. That styles one row:
///   `focused_foreground` on `focused_row_background`.
/// - **The list is disabled** — `disabled_background` replaces all three
///   backdrops, whatever the focus and hover state.
///
/// Rows are then colored by two independent facts, whether the cursor is on the
/// row and whether the row is selected, giving four combinations. Disabled
/// overrides all of them. Note that a row only counts as focused when the list
/// has focus too, so moving focus away leaves the cursor row painted as an
/// ordinary (or selected) row rather than as a highlight.
///
/// [`from_theme`](Self::from_theme) derives all of this from a [`Theme`]; build
/// one by hand only for colors the theme cannot express, and pass it via
/// [`List::style`] or [`ListWidget::style`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListStyle {
    /// Text color of an ordinary row.
    pub foreground: Color,
    /// Backdrop of the list while it does not have focus. Disabled wins over
    /// all three backdrops: a disabled list uses
    /// [`disabled_background`](Self::disabled_background) throughout.
    pub background: Color,
    /// Backdrop of the list while it has focus.
    pub focused_background: Color,
    /// Backdrop of the list while it is hovered. Hover wins over focus.
    pub hovered_background: Color,
    /// Text color of a selected row the cursor is not on. Selection has no fill
    /// of its own; the row keeps the list's backdrop.
    pub selected_foreground: Color,
    /// Text color of the cursor row when it is not selected.
    pub focused_foreground: Color,
    /// Fill behind the cursor row when it is not selected.
    pub focused_row_background: Color,
    /// Text color of the cursor row when it is also selected.
    pub selected_focused_foreground: Color,
    /// Fill behind the cursor row when it is also selected.
    pub selected_focused_background: Color,
    /// The `●`/`■` marker on a selected row, in the default row rendering.
    pub selected_marker: Color,
    /// The `○`/`□` marker on an unselected row, in the default row rendering.
    pub unselected_marker: Color,
    /// Text color of a disabled row, and of its marker.
    pub disabled_foreground: Color,
    /// Fill behind a disabled row.
    pub disabled_background: Color,
}

impl ListStyle {
    /// A neutral style using plain ANSI colors, for painting without a
    /// [`Theme`]. Prefer [`from_theme`](Self::from_theme) when one is available.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            foreground: Color::Reset,
            background: Color::Reset,
            focused_background: Color::Reset,
            hovered_background: Color::DarkGray,
            selected_foreground: Color::Black,
            focused_foreground: Color::Reset,
            focused_row_background: Color::Cyan,
            selected_focused_foreground: Color::Black,
            selected_focused_background: Color::Cyan,
            selected_marker: Color::LightGreen,
            unselected_marker: Color::DarkGray,
            disabled_foreground: Color::DarkGray,
            disabled_background: Color::Reset,
        }
    }

    /// Derive every list color from `theme`.
    ///
    /// The focus and cursor-row fills are lightened from the theme's field
    /// color rather than being separate theme entries, so a custom theme only
    /// supplies the base colors and the list stays consistent with the rest of
    /// the UI.
    #[must_use]
    pub const fn from_theme(theme: &Theme) -> Self {
        let focused_background = lighten(theme.field, FIELD_FOCUS_LIGHTEN);
        let hovered_background = lighten(theme.field, FIELD_HOVER_LIGHTEN);
        let focused_row_background = lighten(focused_background, ROW_FOCUS_LIGHTEN);
        Self {
            foreground: theme.muted_foreground,
            background: theme.field,
            focused_background,
            hovered_background,
            selected_foreground: theme.foreground,
            focused_foreground: theme.muted_foreground,
            focused_row_background,
            selected_focused_foreground: theme.foreground,
            selected_focused_background: focused_row_background,
            selected_marker: theme.primary,
            unselected_marker: theme.muted_foreground,
            disabled_foreground: theme.muted_foreground,
            disabled_background: dim(theme.field, theme.surface, DISABLED_DIM),
        }
    }

    const fn resolve_surface(&self, focused: bool, hovered: bool, disabled: bool) -> Color {
        if disabled {
            // The same backdrop the rows get, so a list shorter than its area
            // does not show a seam past its last item — and the same answer
            // `SelectStyle` gives for a disabled trigger.
            self.disabled_background
        } else if hovered {
            self.hovered_background
        } else if focused {
            self.focused_background
        } else {
            self.background
        }
    }

    fn resolve_row(
        &self,
        focused: bool,
        selected: bool,
        disabled: bool,
        background: Color,
    ) -> Style {
        if disabled {
            return Style::default()
                .fg(self.disabled_foreground)
                .bg(self.disabled_background);
        }
        let (foreground, background) = match (focused, selected) {
            (true, true) => (
                self.selected_focused_foreground,
                self.selected_focused_background,
            ),
            (true, false) => (self.focused_foreground, self.focused_row_background),
            // Selection has no fill: keep whichever backdrop the list has.
            (false, true) => (self.selected_foreground, background),
            (false, false) => (self.foreground, background),
        };
        Style::default().fg(foreground).bg(background)
    }
}

/// A list that only draws — an ordinary ratatui [`Widget`] with no focus,
/// events, or state.
///
/// **Usable in any ratatui app.** Nothing here depends on
/// [`Ratcn`](crate::runtime::Ratcn) or the component layer: render it directly
/// and keep driving selection and scrolling however you already do.
///
/// Rows are pre-rendered [`Text`]s, each free to span several lines, and
/// everything else is addressed by *item index*: which item the cursor is on,
/// which are selected, which are disabled.
/// Explicit colors in the supplied text are preserved; row styles provide the
/// colors for text that does not set its own.
/// That makes it easy to drive from any data you like, but it also means the
/// widget has no idea what a row means — reorder your data and the indices refer
/// to different things.
///
/// Use [`List`] instead when you want that handled: it keys focus and selection
/// by a value you choose, and adds keyboard and mouse handling. It paints
/// through this widget internally.
///
/// Scrolling is the caller's: hand over the rows that are actually on screen and
/// say where they start with [`first_item`](Self::first_item). The widget holds
/// no scroll position and never adjusts one, so whatever owns it — your app, or
/// the [`List`] component — is the only scroll policy in play, and a long list
/// costs a long list's worth of [`Text`] only if you build one.
///
/// # Sizing and row heights
///
/// There is no measurement method, because there is nothing to measure: the
/// widget is area-driven. It paints into the area it is given, top to bottom,
/// stopping when the area runs out. How much room a list deserves is a layout
/// question the caller answers with a ratatui `Constraint`, not something the
/// list can answer from its items.
///
/// Rows may be any height — each item is a [`Text`], so one item may be one
/// line and the next three, and the widget paints each at its own height.
/// Keeping them uniform is the caller's job whenever anything maps a screen row
/// back to an item, because the arithmetic that does so counts *items*, not
/// lines: mixed heights make the row a click lands on no longer correspond to
/// the item that arithmetic names. The [`List`] component takes that job on —
/// it normalizes every item to its [`row_height`](List::row_height) with
/// [`fit_to_height`](crate::list_core::fit_to_height), padding short rows and
/// truncating tall ones — which is why clicking, paging, and wheel scrolling
/// are exact there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListWidget<'a> {
    items: &'a [Text<'static>],
    first_item: usize,
    focused_row: Option<usize>,
    selected_rows: &'a [usize],
    disabled_rows: &'a [bool],
    focused: bool,
    hovered: bool,
    disabled: bool,
    style: ListStyle,
    focus_symbol: &'a str,
}

impl<'a> ListWidget<'a> {
    /// The rows to paint, top to bottom, nothing focused or selected, using
    /// [`ListStyle::fallback`].
    ///
    /// These are the rows on screen, not a whole list to scroll through:
    /// a scrolled caller passes the window and names its position with
    /// [`first_item`](Self::first_item).
    #[must_use]
    pub const fn new(items: &'a [Text<'static>]) -> Self {
        Self {
            items,
            first_item: 0,
            focused_row: None,
            selected_rows: &[],
            disabled_rows: &[],
            focused: false,
            hovered: false,
            disabled: false,
            style: ListStyle::fallback(),
            focus_symbol: "",
        }
    }

    /// Take colors from `theme`.
    #[must_use]
    pub const fn themed(mut self, theme: &Theme) -> Self {
        self.style = ListStyle::from_theme(theme);
        self
    }

    /// Use these exact colors, ignoring any theme.
    #[must_use]
    pub const fn style(mut self, style: ListStyle) -> Self {
        self.style = style;
        self
    }

    /// The index the first row given to [`new`](Self::new) has in the whole
    /// list. Defaults to 0.
    ///
    /// It is what lines the painted window up with the index-addressed state:
    /// [`focused_row`](Self::focused_row), [`selected_rows`](Self::selected_rows),
    /// and [`disabled_rows`](Self::disabled_rows) all count from the start of
    /// the list, not from the top of the window, so scrolling changes only this
    /// number and the rows. The widget never adjusts it — keep-visible and
    /// clamping policy belong to the caller (see
    /// [`linear_nav`](crate::linear_nav) for the arithmetic [`List`] uses).
    #[must_use]
    pub const fn first_item(mut self, first_item: usize) -> Self {
        self.first_item = first_item;
        self
    }

    /// Which row the cursor is on, by index.
    ///
    /// Only highlighted while the list is also [`focused`](Self::focused) — an
    /// unfocused list shows no cursor, so two lists side by side cannot both
    /// look active.
    #[must_use]
    pub const fn focused_row(mut self, focused_row: Option<usize>) -> Self {
        self.focused_row = focused_row;
        self
    }

    /// Indices of the selected rows, counting from the start of the list. Pass
    /// one index for single selection, or several for multi-selection; the
    /// widget does not care which you mean. Only rows it paints are consulted,
    /// so a windowed caller need only name the selected rows in its window.
    ///
    /// This is an index list, not a mask: selection is sparse — usually zero or
    /// one row, any number under multi-selection — so you name the selected
    /// rows rather than flag every row. Contrast
    /// [`disabled_rows`](Self::disabled_rows), which describes every row and is
    /// therefore a positional mask.
    #[must_use]
    pub const fn selected_rows(mut self, selected_rows: &'a [usize]) -> Self {
        self.selected_rows = selected_rows;
        self
    }

    /// A disabled flag per item, counting from the start of the list. Entries
    /// past the end of the slice read as enabled, so a short slice is fine.
    ///
    /// This is a positional mask, not an index list: disabledness is a property
    /// of every row, usually derived straight from the items, so one flag per
    /// item lines up without a lookup. It matches
    /// [`TabsWidget::disabled_tabs`](crate::TabsWidget::disabled_tabs), while
    /// sparse [`selected_rows`](Self::selected_rows) stays an index list.
    #[must_use]
    pub const fn disabled_rows(mut self, disabled_rows: &'a [bool]) -> Self {
        self.disabled_rows = disabled_rows;
        self
    }

    /// Paint as the focused control: the focus backdrop, the cursor row
    /// highlighted, and the focus symbol shown.
    #[must_use]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Paint the list's hovered backdrop. Hover wins when the list is also
    /// focused, while cursor-row visibility remains controlled by
    /// [`focused`](Self::focused).
    #[must_use]
    pub const fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Paint the whole list as disabled, overriding per-row styling.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// A marker drawn in front of the cursor row, such as `"> "`. Empty by
    /// default. Only shown while the list is focused and enabled.
    ///
    /// It occupies columns to the left of every row, so a wide symbol narrows
    /// the space available for labels.
    #[must_use]
    pub const fn focus_symbol(mut self, focus_symbol: &'a str) -> Self {
        self.focus_symbol = focus_symbol;
        self
    }
}

impl Widget for ListWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.width == 0 || area.height == 0 {
            return;
        }
        // The whole widget area first, so rows past the last item keep the
        // list backdrop.
        buf.set_style(
            area,
            Style::default()
                .fg(self.style.foreground)
                .bg(self
                    .style
                    .resolve_surface(self.focused, self.hovered, self.disabled)),
        );
        // The focus symbol occupies a column in front of every row, reserved
        // only while there is a cursor to point at — the same frames the
        // cursor row is highlighted.
        let cursor_shown = self.focused && !self.disabled && self.focused_row.is_some();
        let symbol_width = if cursor_shown {
            u16::try_from(display_width(self.focus_symbol)).unwrap_or(u16::MAX)
        } else {
            0
        };
        let text_x = area.x.saturating_add(symbol_width).min(area.right());
        let text_width = area.width.saturating_sub(symbol_width);

        let mut y = area.y;
        for (row, item) in self.items.iter().enumerate() {
            if y >= area.bottom() {
                break;
            }
            let index = self.first_item.saturating_add(row);
            let height = u16::try_from(item.lines.len().max(1))
                .unwrap_or(u16::MAX)
                .min(area.bottom() - y);
            let row_area = Rect::new(area.x, y, area.width, height);
            // The row's colors fill its full width; the item's own spans then
            // patch over them, so explicit colors in the text are preserved
            // and everything else inherits the row style.
            buf.set_style(row_area, self.row_style(index));
            if cursor_shown && self.focused_row == Some(index) {
                Span::raw(self.focus_symbol).render(row_area, buf);
            }
            item.render(Rect::new(text_x, y, text_width, height), buf);
            y = y.saturating_add(height);
        }
    }
}

impl ListWidget<'_> {
    fn row_style(&self, index: usize) -> Style {
        self.style.resolve_row(
            self.focused && self.focused_row == Some(index),
            self.selected_rows.contains(&index),
            self.disabled || self.disabled_rows.get(index).copied().unwrap_or(false),
            self.style
                .resolve_surface(self.focused, self.hovered, self.disabled),
        )
    }
}

type ReadFn<S, T> = Box<dyn Fn(&S) -> Option<T>>;
type MultiSelectionFn<S, T> = Box<dyn Fn(&S, &T) -> bool>;
type OnChangeFn<T, M> = Box<dyn Fn(T) -> M>;
type OnFocusChangeFn<T, M> = Box<dyn Fn(T, usize) -> M>;
type ScrollFn<S> = Box<dyn Fn(&S) -> usize>;
type RenderItemFn<S, T> = Box<dyn for<'a> Fn(&S, ListItemState<'a, T>) -> Text<'static>>;
type StyleFn = Box<dyn Fn(&Theme) -> ListStyle>;

/// A scrollable list of items, declared with
/// [`render_component`](crate::runtime::RenderCtx::render_component).
///
/// # Cursor and selection are different things
///
/// The **cursor** (called *item focus* here) is where the user is looking. Arrow
/// keys, Home, End, and Page keys move it, and moving it is not a choice —
/// nothing is committed. **Selection** is the choice: Enter, Space, or a left
/// click commits the row under the cursor.
///
/// Keeping them apart is what lets a user browse a list without changing
/// anything, which is why they are two separate bindings rather than one
/// "current item".
///
/// # Bindings
///
/// Each binding is a pair — a reader for the current value and a constructor for
/// the message that changes it — passed in one call so a reader and writer
/// pointing at different fields cannot drift apart. Everything is keyed by your
/// item value, never by row index, so filtering or reordering the list keeps the
/// same item current.
///
/// Item values must be unique within each declaration, or focus, selection, and
/// pointer actions are ambiguous. A debug build panics on duplicates during
/// declaration.
///
/// - [`item_focus`](Self::item_focus) — the cursor. Without it the list is
///   paint- and pointer-only, is not a keyboard focus stop, and ignores keys.
/// - [`selection`](Self::selection) — single selection.
/// - [`multi_selection`](Self::multi_selection) — checkbox-style selection.
///   Mutually exclusive with `selection`.
/// - [`scroll`](Self::scroll) — the scroll offset, if the app wants to own it.
///
/// Disabled items are dimmed and skipped by both keyboard and mouse.
///
/// ```
/// use ratcn::{List, ListItem};
///
/// # #[derive(Clone, Copy, PartialEq)]
/// # struct TaskId(u64);
/// # struct AppState { focused_task: Option<TaskId>, selected_task: Option<TaskId> }
/// # enum Msg { TaskFocused(TaskId, usize), TaskSelected(TaskId) }
/// let _list = List::new([
///     ListItem::new(TaskId(7), "Write spec"),
///     ListItem::new(TaskId(3), "Ship it").disabled(true),
/// ])
/// .item_focus(|s: &AppState| s.focused_task, Msg::TaskFocused)
/// .selection(|s: &AppState| s.selected_task, Msg::TaskSelected);
/// ```
pub struct List<T, S, M> {
    items: Vec<ListItem<T>>,
    focused_item: Option<ReadFn<S, T>>,
    on_focus_change: Option<OnFocusChangeFn<T, M>>,
    selected: Option<ReadFn<S, T>>,
    on_select: Option<OnChangeFn<T, M>>,
    selected_many: Option<MultiSelectionFn<S, T>>,
    on_toggle: Option<OnChangeFn<T, M>>,
    scroll: Option<ScrollFn<S>>,
    on_scroll_change: Option<Box<dyn Fn(usize) -> M>>,
    disabled: bool,
    render_item: Option<RenderItemFn<S, T>>,
    style: Option<StyleFn>,
    focus_symbol: String,
    /// Row height plus the painted offset — render-derived runtime state kept
    /// so hit-testing and wheel arithmetic work against what is on screen,
    /// never a second copy of app-owned scroll.
    viewport: RowViewport,
}

impl<T: fmt::Debug, S, M> fmt::Debug for List<T, S, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("List")
            .field("items", &self.items)
            .field("item_focus", &self.focused_item.is_some())
            .field("selection", &self.selected.is_some())
            .field("multi_selection", &self.selected_many.is_some())
            .field("style", &self.style.is_some())
            .field("disabled", &self.disabled)
            .finish_non_exhaustive()
    }
}

impl<T, S, M> List<T, S, M> {
    /// A list of `items`, with no bindings yet.
    ///
    /// Accepts anything that converts into a [`ListItem`], so `["one", "two"]`
    /// works for a quick list of strings and
    /// `[ListItem::new(id, label), ...]` for anything keyed by a real value.
    #[must_use]
    pub fn new(items: impl IntoIterator<Item = impl Into<ListItem<T>>>) -> Self {
        Self {
            items: items.into_iter().map(Into::into).collect(),
            focused_item: None,
            on_focus_change: None,
            selected: None,
            on_select: None,
            selected_many: None,
            on_toggle: None,
            scroll: None,
            on_scroll_change: None,
            disabled: false,
            render_item: None,
            style: None,
            focus_symbol: String::new(),
            viewport: RowViewport::new(1),
        }
    }

    /// Bind the cursor: where to read it from, and what message moves it.
    ///
    /// `read` returns the value of the item the cursor is on, or `None` when it
    /// is nowhere yet — in which case the first arrow key moves it onto the
    /// first enabled item. `on_change` is called with the value moved to and the
    /// resulting top-item scroll offset. Store both in one update when
    /// [`scroll`](Self::scroll) is bound; an unbound list can ignore the offset.
    ///
    /// Moving the cursor commits nothing; see [`selection`](Self::selection).
    /// Without this binding the list remains available for painting, bound
    /// scrolling, and pointer selection, but is not focusable and does not
    /// consume keyboard navigation or Enter.
    #[must_use]
    pub fn item_focus(
        mut self,
        read: impl Fn(&S) -> Option<T> + 'static,
        on_change: impl Fn(T, usize) -> M + 'static,
    ) -> Self {
        self.focused_item = Some(Box::new(read));
        self.on_focus_change = Some(Box::new(on_change));
        self
    }

    /// Bind single selection: at most one item chosen at a time.
    ///
    /// `read` returns the selected value, or `None` for nothing selected.
    /// `on_select` is called with the item the user committed — Enter or Space
    /// on the cursor row, or a left click on a row.
    ///
    /// A pointer click can commit a row without a preceding pointer-move event.
    /// When [`item_focus`](Self::item_focus) is also bound, the update handling
    /// this message should store the committed value as both selection and item
    /// focus so later keyboard input continues from the clicked row.
    ///
    /// Selecting is an action, not a movement, which is why this is `on_select`
    /// and not `on_selection_change`.
    ///
    /// # Panics
    ///
    /// Rendering panics if this is combined with
    /// [`multi_selection`](Self::multi_selection). A list is one mode or the
    /// other; supporting both at once would make "what does Enter do" depend on
    /// which binding was declared last.
    #[must_use]
    pub fn selection(
        mut self,
        read: impl Fn(&S) -> Option<T> + 'static,
        on_select: impl Fn(T) -> M + 'static,
    ) -> Self {
        self.selected = Some(Box::new(read));
        self.on_select = Some(Box::new(on_select));
        self
    }

    /// Bind multi-selection: any number of items chosen, checkbox style.
    ///
    /// `read` is a predicate asked "is this one selected?" for each row the list
    /// draws, rather than a function returning a collection. That way your app
    /// can store the selection however it likes — a `HashSet`, a flag on each
    /// record, a computed rule — without converting it every frame, and a list
    /// scrolled to fifteen of a thousand rows asks fifteen times. `on_toggle`
    /// is called with the item the user flipped; your update function decides
    /// whether that means adding or removing.
    ///
    /// When [`item_focus`](Self::item_focus) is also bound, the update handling
    /// a pointer toggle should store this value as item focus as well. A click
    /// need not be preceded by a pointer-move event.
    ///
    /// Switches the default row markers from `●`/`○` to `■`/`□`.
    ///
    /// # Panics
    ///
    /// Rendering panics if this is combined with
    /// [`selection`](Self::selection).
    #[must_use]
    pub fn multi_selection(
        mut self,
        read: impl Fn(&S, &T) -> bool + 'static,
        on_toggle: impl Fn(T) -> M + 'static,
    ) -> Self {
        self.selected_many = Some(Box::new(read));
        self.on_toggle = Some(Box::new(on_toggle));
        self
    }

    /// Bind the requested scroll offset — the index of the topmost visible item.
    ///
    /// Before paint, the list adjusts the requested value to keep the focused
    /// item visible. Focus movement computes the same resulting offset from the
    /// current app value and passes it to the [`item_focus`](Self::item_focus)
    /// message constructor, so its update can persist cursor and scroll
    /// atomically. Hit-testing and wheel input use the actual painted offset.
    ///
    /// The wheel is the exception to keeping the cursor visible: it scrolls
    /// the view and leaves the cursor where it is, so the cursor may scroll
    /// out of sight — the behavior a scrollable list has everywhere else. It
    /// emits the offset one notch away from the painted one, and consumes the
    /// event without emitting when the app already holds that value. Paint
    /// resumes scrolling the cursor into view as soon as anything moves — the
    /// cursor, or the items under it; see
    /// [`WheelPark`](crate::list_core::WheelPark) for exactly when the wheeled
    /// view stops being honored. Render-driven adjustment itself cannot emit a
    /// message.
    ///
    /// Only needed when something outside the list has to know or change where
    /// it is scrolled to, such as a scrollbar drawn alongside it. Left unbound,
    /// the list owns the offset itself — keyboard movement and the wheel both
    /// still scroll it, exactly as they do when bound; there is simply no
    /// message and no app-held value.
    #[must_use]
    pub fn scroll(
        mut self,
        read: impl Fn(&S) -> usize + 'static,
        on_change: impl Fn(usize) -> M + 'static,
    ) -> Self {
        self.scroll = Some(Box::new(read));
        self.on_scroll_change = Some(Box::new(on_change));
        self
    }

    /// Draw each row yourself instead of using the default marker-and-label
    /// line.
    ///
    /// The closure gets app state and a [`ListItemState`] describing the row,
    /// and returns what to paint. Use it for columns, secondary text, per-row
    /// icons — anything the default cannot express. The resolved [`ListStyle`]
    /// is painted beneath the returned text, so unstyled text inherits row-state
    /// colors while explicit `Text`, `Line`, and `Span` colors are preserved.
    ///
    /// Return a [`Line`](ratatui::text::Line) for the usual one-line row, or a
    /// [`Text`] for a taller one — a name above a subtitle, say. A multi-line
    /// row also needs [`row_height`](Self::row_height) set to match, since every
    /// item must be the same height for clicks to land on the right one.
    #[must_use]
    pub fn render_item<R: Into<Text<'static>>>(
        mut self,
        f: impl for<'a> Fn(&S, ListItemState<'a, T>) -> R + 'static,
    ) -> Self {
        self.render_item = Some(Box::new(move |state, row| f(state, row).into()));
        self
    }

    /// How many terminal rows each item occupies. Defaults to 1.
    ///
    /// Raise it when [`render_item`](Self::render_item) returns more than one
    /// line — a name above a subtitle, say. Every item gets the same height,
    /// which is what keeps clicking, paging, and scrolling exact: a returned
    /// [`Text`] is padded with blank lines or truncated to fit, so the row the
    /// user clicks is always the item the runtime thinks it is.
    ///
    /// A height of 0 is treated as 1.
    #[must_use]
    pub const fn row_height(mut self, rows: u16) -> Self {
        self.viewport = RowViewport::new(rows);
        self
    }

    /// A marker drawn in front of the cursor row, such as `"> "`. Empty by
    /// default, and only shown while the list has keyboard focus or hover.
    #[must_use]
    pub fn focus_symbol(mut self, symbol: impl Into<String>) -> Self {
        self.focus_symbol = symbol.into();
        self
    }

    /// Replace the theme-derived [`ListStyle`].
    ///
    /// The closure receives the active theme each render, so a style built from
    /// its argument follows theme switches. Ignore the argument (`|_| STYLE`)
    /// for colors that should stay fixed.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> ListStyle + 'static) -> Self {
        self.style = Some(Box::new(style));
        self
    }

    /// Dim the whole list and stop it responding.
    ///
    /// A disabled list is not focusable, so Tab skips it — as does a list that
    /// is empty or has every row disabled, since there would be nothing for the
    /// cursor to land on. Disable individual rows with [`ListItem::disabled`].
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl<T: Clone + PartialEq + 'static, S, M> List<T, S, M> {
    fn disabled_at(&self, index: usize) -> bool {
        list_core::disabled_at(&self.items, index)
    }

    fn focused_index(&self, state: &S) -> Option<usize> {
        list_core::index_of(&self.items, &(self.focused_item.as_ref()?)(state)?)
    }

    fn selected_index(&self, state: &S) -> Option<usize> {
        list_core::index_of(&self.items, &(self.selected.as_ref()?)(state)?)
    }

    fn move_focus(&self, index: usize, state: &S, area: Rect) -> EventResult<M> {
        match &self.on_focus_change {
            Some(on_change) => EventResult::Emit(on_change(
                self.items[index].value().clone(),
                self.offset_for_focus(state, area, index),
            )),
            None => EventResult::Ignored,
        }
    }

    fn offset_for_focus(&self, state: &S, area: Rect, index: usize) -> usize {
        let current = self
            .scroll
            .as_ref()
            .map_or(self.viewport.painted_offset(), |read| read(state));
        self.viewport
            .cursor_visible_offset(area, self.items.len(), current, Some(index))
    }

    fn select(&self, index: usize) -> EventResult<M> {
        if let Some(on_select) = &self.on_select {
            return EventResult::Emit(on_select(self.items[index].value().clone()));
        }
        if let Some(on_toggle) = &self.on_toggle {
            return EventResult::Emit(on_toggle(self.items[index].value().clone()));
        }
        EventResult::Ignored
    }

    fn scroll_view(
        &mut self,
        direction: ScrollDirection,
        state: &S,
        area: Rect,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<M> {
        let step = match direction {
            ScrollDirection::Up => ScrollStep::Up,
            ScrollDirection::Down => ScrollStep::Down,
            ScrollDirection::Left | ScrollDirection::Right => return EventResult::Ignored,
        };
        let painted = self.viewport.painted_offset();
        // The wheel moves the view, never the cursor: the offset is clamped
        // to the item range only, so the cursor may scroll out of sight.
        let next = linear_nav::wheel_offset(
            self.items.len(),
            self.viewport.visible_items(area),
            painted,
            step,
            SCROLL_STEP,
        );
        // Park the view against the list as the cursor was left in it, so the
        // next render honors this offset instead of scrolling the cursor back
        // into view. The park lives on the list's identity and outlives this
        // instance, which is what lets an unbound list scroll at all.
        let cursor = self.focused_index(state);
        ctx.transient::<WheelPark<T>>()
            .park(next, &self.items, cursor);
        // Keep this retained instance's hit-testing aligned with the offset
        // the next paint will use.
        self.viewport.record_painted_offset(next);
        let Some(on_change) = &self.on_scroll_change else {
            return EventResult::Consumed;
        };
        let current = self.scroll.as_ref().map_or(painted, |read| read(state));
        if current == next {
            EventResult::Consumed
        } else {
            EventResult::Emit(on_change(next))
        }
    }

    fn handle_key(&self, key: KeyEvent, state: &S, area: Rect) -> EventResult<M> {
        if self.focused_item.is_none() {
            return EventResult::Ignored;
        }
        let cursor = self.focused_index(state);
        // Navigation is asked first because it owns the Ctrl chords that the
        // modifier gate below rejects.
        if let Some(outcome) = linear_nav::nav_key_target(
            key,
            self.items.len(),
            cursor,
            self.viewport.visible_items(area).max(1),
            |i| self.disabled_at(i),
        ) {
            return match outcome {
                NavOutcome::Move(target) => self.move_focus(target, state, area),
                NavOutcome::Stay => EventResult::Consumed,
            };
        }
        if linear_nav::has_reserved_modifier(key) {
            return EventResult::Ignored;
        }
        if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
            return match cursor {
                Some(index) if !self.disabled_at(index) => self.select(index),
                _ => EventResult::Ignored,
            };
        }
        // Anything else — including every letter but `j` and `k` — bubbles, so
        // the app keeps its single-key hotkeys while a list has focus.
        EventResult::Ignored
    }
}

impl<T: Clone + PartialEq + 'static, S, M> Component<S, M> for List<T, S, M> {
    fn prepare(&mut self, _state: &S) {
        // Quadratic in the item count and re-derived on every frame's fresh
        // instance, so a release build takes the items on trust.
        if cfg!(debug_assertions) {
            list_core::assert_unique_values(self.items.iter().map(ListItem::value), "List");
        }
        assert!(
            !(self.selected.is_some() && self.selected_many.is_some()),
            "List::selection(...) and List::multi_selection(...) cannot be used together; choose one selection mode"
        );
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_, S, M>) {
        let area = ctx.area();
        let state = ctx.state();
        let focused_row = self.focused_index(state);
        let requested = self.scroll.as_ref().map(|scroll| scroll(state));
        // The wheel parks the view against the list as it stood. While nothing
        // has moved under it, the parked offset is painted as it is — the wheel
        // may leave the cursor off-screen. Once the cursor moves, or the items
        // do, it is scrolled back into view. A bound scroll offset always wins
        // over the park: the app owns it.
        WheelPark::settle_transient(
            ctx,
            &self.items,
            focused_row,
            requested,
            &mut self.viewport,
            area,
        );
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, S>) {
        let area = ctx.area();
        let state = ctx.state();
        let focused_row = self.focused_index(state);
        let selected_row = self.selected_index(state);
        let disabled_rows: Vec<bool> = self.items.iter().map(ListItem::is_disabled).collect();
        let style = resolve_style(self.style.as_deref(), ctx.theme, ListStyle::from_theme);
        // One cursor, shown while the list is either focused or under the
        // pointer; where it sits was decided during declaration.
        let cursor_visible = ctx.focused || ctx.hovered;
        let selection_mode = if self.selected_many.is_some() {
            Some(true)
        } else if self.selected.is_some() {
            Some(false)
        } else {
            None
        };
        let rows_per_item = self.viewport.rows_per_item();
        // Only the rows on screen are built, however long the list is. The
        // count rounds up because the area's trailing rows still paint the item
        // that starts in them, clipped; every index below counts from the start
        // of the list, so a custom `render_item` and retained hit-testing see
        // the same positions whatever the view is scrolled to.
        let first_item = self.viewport.painted_offset().min(self.items.len());
        let last_item = first_item
            .saturating_add(usize::from(area.height.div_ceil(rows_per_item)))
            .min(self.items.len());
        let mut selected_rows: Vec<usize> = Vec::new();
        let items = list_core::windowed_rows(
            &self.items,
            first_item..last_item,
            rows_per_item,
            |index, item| {
                // Asked per painted row rather than looked up in a list of every
                // selected index, which would make painting quadratic.
                let selected = match &self.selected_many {
                    Some(selected_many) => selected_many(state, item.value()),
                    None => selected_row == Some(index),
                };
                if selected {
                    selected_rows.push(index);
                }
                let row = ListItemState {
                    index,
                    value: item.value(),
                    label: item.label(),
                    focused: cursor_visible && focused_row == Some(index),
                    selected,
                    disabled: self.disabled || item.is_disabled(),
                };
                match &self.render_item {
                    Some(render_item) => render_item(state, row),
                    None => default_item_line(&row, selection_mode, &style),
                }
            },
        );
        ctx.render_widget(
            ListWidget::new(&items)
                .first_item(first_item)
                .focused_row(focused_row)
                .selected_rows(&selected_rows)
                .disabled_rows(&disabled_rows)
                .style(style)
                .focused(cursor_visible)
                .hovered(ctx.hovered)
                .disabled(self.disabled)
                .focus_symbol(&self.focus_symbol),
            area,
        );
    }

    fn handle_event(&mut self, event: &Event, state: &S, ctx: &mut EventCtx<'_>) -> EventResult<M> {
        if self.disabled || self.items.is_empty() {
            return EventResult::Ignored;
        }
        let area = ctx.area();
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                // The wheel addresses the view rather than a row, so it is
                // answered before the row decision is asked for.
                MouseKind::Scroll(direction) => self.scroll_view(direction, state, area, ctx),
                // `row_at` already rejects anything outside the list, so no
                // paint-state gate is needed here; whether the cursor is *shown*
                // stays a paint decision.
                kind => {
                    let row = self
                        .viewport
                        .row_at(area, self.items.len(), mouse.column, mouse.row);
                    match list_core::row_intent(
                        kind,
                        &self.items,
                        row,
                        self.focused_index(state),
                        self.selected.is_some() || self.selected_many.is_some(),
                    ) {
                        RowIntent::BlockPress | RowIntent::Stay => EventResult::Consumed,
                        RowIntent::Focus(index) => self.move_focus(index, state, area),
                        RowIntent::Commit(index) => self.select(index),
                        RowIntent::Bubble => EventResult::Ignored,
                    }
                }
            },
            Event::Key(key) => self.handle_key(*key, state, area),
            _ => EventResult::Ignored,
        }
    }

    fn is_focusable(&self, _state: &S) -> bool {
        self.focused_item.is_some()
            && !self.disabled
            && linear_nav::has_enabled(self.items.len(), |i| self.disabled_at(i))
    }
}

fn default_item_line<T>(
    row: &ListItemState<'_, T>,
    selection_mode: Option<bool>,
    style: &ListStyle,
) -> Text<'static> {
    let Some(multiple) = selection_mode else {
        return Text::from(row.label.to_string());
    };
    Text::from(selection_indicator::marker_line(
        row.label,
        row.selected,
        multiple,
        row.disabled,
        selection_indicator::MarkerColors {
            disabled: style.disabled_foreground,
            selected: style.selected_marker,
            unselected: style.unselected_marker,
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use ratatui::{Terminal, backend::TestBackend, style::Modifier, text::Line};

    use super::*;
    use crate::list_core::fit_to_height;
    use crate::runtime::{ChildId, MouseButton, MouseEvent, Ratcn};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Task {
        A,
        B,
        C,
        D,
        E,
        F,
        G,
    }

    #[derive(Debug, PartialEq)]
    enum Msg {
        Focused(Task, usize),
        Selected(Task),
        Toggled(Task),
        Scrolled(usize),
        ComponentFocus(crate::runtime::FocusState),
        Pressed,
    }

    #[derive(Default)]
    struct State {
        focused: Option<Task>,
        selected: Option<Task>,
        toggled: Vec<Task>,
        scroll: usize,
        component_focus: crate::runtime::FocusState,
    }

    /// Drives a retained `Ratcn` surface through a `TestBackend`.
    struct TestBackendDriver {
        terminal: Terminal<TestBackend>,
        ratcn: Ratcn<State, Msg>,
    }

    impl TestBackendDriver {
        fn new(width: u16, height: u16) -> Self {
            Self {
                terminal: Terminal::new(TestBackend::new(width, height)).expect("terminal"),
                ratcn: Ratcn::new(),
            }
        }

        fn render(&mut self, state: &State, items: impl IntoIterator<Item = ListItem<Task>>) {
            let items: Vec<_> = items.into_iter().collect();
            let theme = Theme::default_dark();
            self.terminal
                .draw(|frame| {
                    let area = frame.area();
                    self.ratcn.render(frame, state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("list"),
                            List::new(items.clone())
                                .item_focus(|state: &State| state.focused, Msg::Focused)
                                .selection(|state: &State| state.selected, Msg::Selected)
                                .scroll(|state: &State| state.scroll, Msg::Scrolled)
                                .focus_symbol(">"),
                            area,
                        );
                    });
                })
                .expect("draw");
        }

        fn event(&mut self, event: Event, state: &State) -> EventResult<Msg> {
            self.ratcn.handle_event(event, state)
        }

        fn row(&self, row: u16) -> String {
            let buffer = self.terminal.backend().buffer();
            let width = usize::from(buffer.area.width);
            buffer.content[usize::from(row) * width..]
                .iter()
                .take(width)
                .map(ratatui::buffer::Cell::symbol)
                .collect()
        }

        fn cell(&self, column: u16, row: u16) -> &ratatui::buffer::Cell {
            self.terminal
                .backend()
                .buffer()
                .cell((column, row))
                .expect("cell")
        }
    }

    fn item(value: Task, label: &str) -> ListItem<Task> {
        ListItem::new(value, label)
    }

    /// Renders a two-row-per-item list, so a click's screen row and its item
    /// index deliberately disagree.
    struct TallListDriver {
        terminal: Terminal<TestBackend>,
        ratcn: Ratcn<State, Msg>,
    }

    impl TallListDriver {
        const ROW_HEIGHT: u16 = 2;

        fn new(width: u16, height: u16) -> Self {
            Self {
                terminal: Terminal::new(TestBackend::new(width, height)).expect("terminal"),
                ratcn: Ratcn::new(),
            }
        }

        fn render(&mut self, state: &State, items: &[ListItem<Task>]) {
            let theme = Theme::default_dark();
            self.terminal
                .draw(|frame| {
                    let area = frame.area();
                    self.ratcn.render(frame, state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("list"),
                            List::new(items.to_vec())
                                .item_focus(|state: &State| state.focused, Msg::Focused)
                                .selection(|state: &State| state.selected, Msg::Selected)
                                .row_height(Self::ROW_HEIGHT)
                                .render_item(|_state: &State, row| {
                                    Text::from(vec![
                                        Line::from(row.label.to_string()),
                                        Line::from("  subtitle"),
                                    ])
                                }),
                            area,
                        );
                    });
                })
                .expect("draw");
        }

        fn event(&mut self, event: Event, state: &State) -> EventResult<Msg> {
            self.ratcn.handle_event(event, state)
        }
    }

    // A two-line row means screen row 1 still belongs to item 0. Dividing by the
    // row height is what keeps a click on a subtitle from selecting its
    // neighbour.
    #[test]
    fn clicking_a_second_line_selects_the_item_that_owns_it() {
        let state = State::default();
        let items = vec![item(Task::A, "Alpha"), item(Task::B, "Bravo")];
        let mut driver = TallListDriver::new(20, 6);
        driver.render(&state, &items);

        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 2, 1), &state),
            EventResult::Emit(Msg::Selected(Task::A)),
            "the subtitle line belongs to the first item"
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 2, 2), &state),
            EventResult::Emit(Msg::Selected(Task::B)),
            "the second item starts two rows down"
        );
    }

    #[test]
    fn trailing_partial_row_does_not_target_an_invisible_item() {
        let state = State::default();
        let items = vec![
            item(Task::A, "Alpha"),
            item(Task::B, "Bravo"),
            item(Task::C, "Charlie"),
        ];
        let mut driver = TallListDriver::new(20, 5);
        driver.render(&state, &items);

        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 2, 4), &state),
            EventResult::Ignored,
            "a trailing row that cannot fit a complete item is not interactive"
        );
    }

    // Paging counts items that fit, not screen rows: a 6-row area holds three
    // two-row items, so PageDown from the first lands on the third.
    #[test]
    fn paging_counts_items_that_fit_not_screen_rows() {
        let state = State {
            focused: Some(Task::A),
            ..State::default()
        };
        let items = vec![
            item(Task::A, "Alpha"),
            item(Task::B, "Bravo"),
            item(Task::C, "Charlie"),
            item(Task::D, "Delta"),
            item(Task::E, "Echo"),
        ];
        let mut driver = TallListDriver::new(20, 6);
        driver.render(&state, &items);

        assert_eq!(
            driver.event(key(KeyCode::PageDown), &state),
            EventResult::Emit(Msg::Focused(Task::D, 1)),
            "three items fit, so a page is three items"
        );
    }

    // A render_item closure returning the wrong line count must not shift later
    // items; padding and truncation keep every item exactly row_height tall.
    #[test]
    fn rows_are_padded_and_truncated_to_the_declared_height() {
        let short = fit_to_height(Text::from("one"), 3);
        assert_eq!(short.lines.len(), 3);
        assert_eq!(short.lines[0].to_string(), "one");
        assert_eq!(short.lines[2].to_string(), "");

        let long = fit_to_height(
            Text::from(vec![Line::from("a"), Line::from("b"), Line::from("c")]),
            2,
        );
        assert_eq!(long.lines.len(), 2);
        assert_eq!(long.lines[1].to_string(), "b");
    }

    // Zero would divide by zero in hit-testing, so it is clamped to one row.
    #[test]
    fn zero_row_height_is_treated_as_one() {
        let list: List<Task, State, Msg> = List::new([item(Task::A, "Alpha")]).row_height(0);
        assert_eq!(list.viewport.rows_per_item(), 1);
    }
    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code))
    }
    fn ctrl_key(ch: char) -> Event {
        Event::Key(KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: crate::runtime::Modifiers {
                ctrl: true,
                ..crate::runtime::Modifiers::NONE
            },
        })
    }
    fn mouse(kind: MouseKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: crate::runtime::Modifiers::NONE,
        })
    }

    // A fixed selected fill used to match the resting backdrop but not the
    // focused one, so focusing the list left the selected row a dark band.
    #[test]
    fn a_selected_row_keeps_the_rest_focus_or_hover_backdrop() {
        let items = [Text::from("selected"), Text::from("other")];
        let mut style = ListStyle::fallback();
        style.background = Color::Blue;
        style.focused_background = Color::Green;
        style.hovered_background = Color::Magenta;
        style.selected_foreground = Color::Yellow;
        let area = Rect::new(0, 0, 10, 2);

        for (focused, hovered, backdrop) in [
            (false, false, Color::Blue),
            (true, false, Color::Green),
            (false, true, Color::Magenta),
            (true, true, Color::Magenta),
        ] {
            let mut buffer = Buffer::empty(area);
            Widget::render(
                ListWidget::new(&items)
                    .selected_rows(&[0])
                    // Row 1 is the cursor, so row 0 is selected but not focused.
                    .focused_row(Some(1))
                    .focused(focused)
                    .hovered(hovered)
                    .style(style),
                area,
                &mut buffer,
            );

            let selected = buffer.cell((0, 0)).expect("selected cell");
            assert_eq!(
                selected.bg, backdrop,
                "selected row should match the list backdrop (focused: {focused}, hovered: {hovered})"
            );
            assert_eq!(selected.fg, Color::Yellow, "selection shows in the label");
        }
    }

    #[test]
    fn list_widget_preserves_explicit_text_colors_and_modifiers() {
        let items = [
            Text::from(Span::styled(
                "selected",
                Style::default()
                    .fg(Color::Magenta)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Text::from(Span::styled(
                "disabled",
                Style::default()
                    .fg(Color::Cyan)
                    .bg(Color::White)
                    .add_modifier(Modifier::ITALIC),
            )),
        ];
        let mut style = ListStyle::fallback();
        style.selected_foreground = Color::Yellow;
        style.background = Color::Blue;
        style.disabled_foreground = Color::DarkGray;
        style.disabled_background = Color::Red;
        let area = Rect::new(0, 0, 10, 2);
        let mut buffer = Buffer::empty(area);

        Widget::render(
            ListWidget::new(&items)
                .selected_rows(&[0])
                .disabled_rows(&[false, true])
                .style(style),
            area,
            &mut buffer,
        );

        let selected = buffer.cell((0, 0)).expect("selected custom span");
        assert_eq!(selected.fg, Color::Magenta);
        assert_eq!(selected.bg, Color::Green);
        assert!(selected.modifier.contains(Modifier::BOLD));
        let disabled = buffer.cell((0, 1)).expect("disabled custom span");
        assert_eq!(disabled.fg, Color::Cyan);
        assert_eq!(disabled.bg, Color::White);
        assert!(disabled.modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn default_markers_preserve_selected_and_unselected_colors() {
        let state = State {
            selected: Some(Task::A),
            ..State::default()
        };
        let mut style = ListStyle::fallback();
        style.selected_marker = Color::Yellow;
        style.unselected_marker = Color::Magenta;
        style.selected_foreground = Color::Red;
        style.foreground = Color::Cyan;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        "list",
                        List::new([item(Task::A, "Alpha"), item(Task::B, "Bravo")])
                            .selection(|state: &State| state.selected, Msg::Selected)
                            .style(move |_| style),
                        area,
                    );
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer.cell((1, 0)).expect("selected marker").fg,
            Color::Yellow
        );
        assert_eq!(
            buffer.cell((1, 1)).expect("unselected marker").fg,
            Color::Magenta
        );
    }

    #[test]
    fn interactive_custom_row_colors_preserve_explicit_span_colors() {
        let state = State {
            selected: Some(Task::A),
            ..State::default()
        };
        let mut style = ListStyle::fallback();
        style.selected_foreground = Color::Yellow;
        style.background = Color::Blue;
        style.disabled_foreground = Color::DarkGray;
        style.disabled_background = Color::Red;
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        "list",
                        List::new([
                            item(Task::A, "selected"),
                            item(Task::B, "disabled").disabled(true),
                        ])
                        .selection(|state: &State| state.selected, Msg::Selected)
                        .render_item(|_: &State, row| {
                            let modifier = if row.selected {
                                Modifier::BOLD
                            } else {
                                Modifier::ITALIC
                            };
                            Text::from(Span::styled(
                                row.label.to_string(),
                                Style::default()
                                    .fg(Color::Magenta)
                                    .bg(Color::Green)
                                    .add_modifier(modifier),
                            ))
                        })
                        .style(move |_| style),
                        area,
                    );
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let selected = buffer.cell((0, 0)).expect("selected custom span");
        assert_eq!(selected.fg, Color::Magenta);
        assert_eq!(selected.bg, Color::Green);
        assert!(selected.modifier.contains(Modifier::BOLD));
        let disabled = buffer.cell((0, 1)).expect("disabled custom span");
        assert_eq!(disabled.fg, Color::Magenta);
        assert_eq!(disabled.bg, Color::Green);
        assert!(disabled.modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn hidden_cursor_paints_the_caller_owned_window() {
        let items = [
            Text::from("Alpha"),
            Text::from("Bravo"),
            Text::from("Charlie"),
            Text::from("Delta"),
        ];
        let area = Rect::new(0, 0, 10, 2);

        // An unfocused or disabled list shows no cursor and reserves no focus
        // symbol column, but still paints the window it was given.
        for (focused, disabled) in [(false, false), (true, true)] {
            let mut buffer = Buffer::empty(area);

            ListWidget::new(&items[2..])
                .first_item(2)
                .focused_row(Some(3))
                .focused(focused)
                .disabled(disabled)
                .focus_symbol("> ")
                .render(area, &mut buffer);

            assert_eq!(
                buffer.cell((0, 0)).expect("first visible cell").symbol(),
                "C"
            );
        }
    }

    #[test]
    fn string_sugar_keys_items_by_their_labels() {
        let list = List::<String, State, Msg>::new(["Inbox", "Archive"]);
        assert_eq!(list.items[0].value(), "Inbox");
        assert_eq!(list.items[0].label(), "Inbox");
    }

    #[test]
    fn reorder_preserves_focused_and_selected_values() {
        let mut driver = TestBackendDriver::new(20, 3);
        let state = State {
            focused: Some(Task::B),
            selected: Some(Task::B),
            ..State::default()
        };
        driver.render(
            &state,
            [
                item(Task::A, "Alpha"),
                item(Task::B, "Bravo"),
                item(Task::C, "Charlie"),
            ],
        );
        driver.render(
            &state,
            [
                item(Task::C, "Charlie"),
                item(Task::A, "Alpha"),
                item(Task::B, "Bravo"),
            ],
        );

        assert!(driver.row(2).contains("> ● Bravo"));
        assert_eq!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::Selected(Task::B))
        );
    }

    #[test]
    fn filtering_a_focused_value_parks_without_highlighting_or_emitting() {
        let mut driver = TestBackendDriver::new(20, 2);
        let state = State {
            focused: Some(Task::B),
            selected: Some(Task::B),
            ..State::default()
        };
        driver.render(&state, [item(Task::A, "Alpha"), item(Task::B, "Bravo")]);
        driver.render(
            &state,
            [
                item(Task::A, "Alpha").disabled(true),
                item(Task::C, "Charlie"),
            ],
        );

        assert!(!driver.row(0).contains('>'));
        assert!(!driver.row(0).contains('●'));
        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Focused(Task::C, 0))
        );
        assert_eq!(
            driver.event(key(KeyCode::End), &state),
            EventResult::Emit(Msg::Focused(Task::C, 0))
        );
    }

    /// A disabled list is one flat backdrop: the rows and the empty space past
    /// the last item agree, so a short list shows no seam.
    #[test]
    fn a_disabled_list_paints_one_backdrop_past_its_last_item() {
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        let mut ratcn = Ratcn::<State, Msg>::new();
        let state = State::default();
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("list"),
                        List::new([item(Task::A, "Alpha"), item(Task::B, "Bravo")])
                            .item_focus(|state: &State| state.focused, Msg::Focused)
                            .disabled(true),
                        area,
                    );
                });
            })
            .expect("draw");

        let disabled = ListStyle::from_theme(&theme).disabled_background;
        let buffer = terminal.backend().buffer();
        for row in 0..4 {
            assert_eq!(
                buffer.cell((3, row)).expect("cell").bg,
                disabled,
                "row {row} must use the disabled backdrop, item row or not"
            );
        }
    }

    #[test]
    fn disabled_items_are_dimmed_and_ignore_primary_clicks() {
        let mut driver = TestBackendDriver::new(20, 2);
        let state = State::default();
        driver.render(
            &state,
            [
                item(Task::A, "Alpha"),
                item(Task::B, "Bravo").disabled(true),
            ],
        );

        assert_eq!(
            driver.cell(3, 1).bg,
            ListStyle::from_theme(&Theme::default_dark()).disabled_background
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 2, 1), &state),
            EventResult::Consumed
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Up(MouseButton::Left), 2, 1), &state),
            EventResult::Ignored
        );
    }

    #[test]
    fn disabled_rows_consume_pointer_down_and_cursorless_lists_do_not_take_focus() {
        #[derive(Default)]
        struct RoutedState {
            focus: crate::runtime::FocusState,
            selected: Option<Task>,
        }

        #[derive(Debug, PartialEq)]
        enum RoutedMsg {
            Focus(crate::runtime::FocusState),
            Selected(Task),
        }

        let theme = Theme::default_dark();
        let state = RoutedState {
            focus: crate::runtime::FocusState::intent([ChildId::Static("other")]),
            ..RoutedState::default()
        };
        let mut ratcn = Ratcn::new().focus(|state: &RoutedState| &state.focus, RoutedMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(20, 3)).expect("terminal");
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("other"),
                        crate::Button::<RoutedMsg>::new("Other"),
                        Rect::new(0, 0, 20, 1),
                    );
                    ctx.render_component(
                        ChildId::Static("list"),
                        List::new([
                            item(Task::A, "Alpha"),
                            item(Task::B, "Bravo").disabled(true),
                        ])
                        .selection(|state: &RoutedState| state.selected, RoutedMsg::Selected),
                        Rect::new(0, 1, 20, 2),
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 2), &state),
            EventResult::Consumed
        );
        assert_eq!(
            state.focus,
            crate::runtime::FocusState::intent([ChildId::Static("other")])
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 2), &state),
            EventResult::Ignored
        );

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
            EventResult::Ignored
        );
        assert_eq!(
            state.focus,
            crate::runtime::FocusState::intent([ChildId::Static("other")])
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 1, 1), &state),
            EventResult::Emit(RoutedMsg::Selected(Task::A))
        );
    }

    fn six_items() -> [ListItem<Task>; 6] {
        [
            item(Task::A, "Alpha"),
            item(Task::B, "Bravo"),
            item(Task::C, "Charlie"),
            item(Task::D, "Delta"),
            item(Task::E, "Echo"),
            item(Task::F, "Foxtrot"),
        ]
    }

    #[test]
    fn the_wheel_scrolls_the_view_and_leaves_the_cursor_behind() {
        let mut driver = TestBackendDriver::new(20, 3);
        let mut state = State {
            focused: Some(Task::A),
            ..State::default()
        };
        driver.render(&state, six_items());
        assert!(driver.row(0).contains("Alpha"), "{}", driver.row(0));

        // One notch scrolls the view by the wheel step. The cursor stays on
        // Alpha, which the wheel is free to scroll out of sight.
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 2, 0),
                &state
            ),
            EventResult::Emit(Msg::Scrolled(3))
        );
        state.scroll = 3;
        driver.render(&state, six_items());
        assert!(
            driver.row(0).contains("Delta"),
            "the wheeled offset survives the redraw: {}",
            driver.row(0)
        );
        assert_eq!(state.focused, Some(Task::A), "the wheel never moves it");

        // Moving the cursor again brings it back into view, and the move
        // carries the resulting offset so one message persists both.
        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Focused(Task::B, 1))
        );
        state.focused = Some(Task::B);
        state.scroll = 1;
        driver.render(&state, six_items());
        assert!(driver.row(0).contains("Bravo"), "{}", driver.row(0));
    }

    #[test]
    fn wheeling_against_a_stale_app_offset_synchronizes_it() {
        let mut driver = TestBackendDriver::new(20, 3);
        let state = State {
            // Render must scroll Foxtrot into view, so the painted offset is
            // 3 while the app still holds 0.
            focused: Some(Task::F),
            ..State::default()
        };
        driver.render(&state, six_items());
        assert!(driver.row(0).contains("Delta"), "{}", driver.row(0));

        // Wheeling up lands on the offset the app already holds, so there is
        // nothing to persist; the next render paints it.
        assert_eq!(
            driver.event(mouse(MouseKind::Scroll(ScrollDirection::Up), 2, 0), &state),
            EventResult::Consumed
        );
        driver.render(&state, six_items());
        assert!(
            driver.row(0).contains("Alpha"),
            "the parked view is painted even though the cursor is off-screen: {}",
            driver.row(0)
        );

        // Wheeling back down computes from the painted offset and emits the
        // value the app is missing.
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 2, 0),
                &state
            ),
            EventResult::Emit(Msg::Scrolled(3))
        );
    }

    #[test]
    fn focus_and_scroll_move_atomically_across_the_viewport_without_redraw() {
        let mut driver = TestBackendDriver::new(20, 3);
        let mut state = State {
            focused: Some(Task::C),
            ..State::default()
        };
        driver.render(
            &state,
            [
                item(Task::A, "Alpha"),
                item(Task::B, "Bravo"),
                item(Task::C, "Charlie"),
                item(Task::D, "Delta"),
                item(Task::E, "Echo"),
                item(Task::F, "Foxtrot"),
            ],
        );

        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Focused(Task::D, 1))
        );
        state.focused = Some(Task::D);
        state.scroll = 1;

        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Focused(Task::E, 2)),
            "the retained component reads focus and scroll from current app state"
        );
        state.focused = Some(Task::E);
        state.scroll = 2;

        assert_eq!(
            driver.event(key(KeyCode::Home), &state),
            EventResult::Emit(Msg::Focused(Task::A, 0))
        );
    }

    /// An unbound list owns its scroll offset the same way `Select`'s panel
    /// does, so the wheel works without any app-held offset.
    #[test]
    fn an_unbound_list_still_scrolls_on_the_wheel() {
        let state = State {
            focused: Some(Task::A),
            ..State::default()
        };
        let theme = Theme::default_dark();
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 3)).expect("terminal");
        let draw = |terminal: &mut Terminal<TestBackend>, ratcn: &mut Ratcn<State, Msg>| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("list"),
                            List::new(six_items())
                                .item_focus(|state: &State| state.focused, Msg::Focused),
                            area,
                        );
                    });
                })
                .expect("draw");
        };
        let row = |terminal: &Terminal<TestBackend>, row: u16| -> String {
            let buffer = terminal.backend().buffer();
            (0..buffer.area.width)
                .map(|column| buffer.cell((column, row)).expect("cell").symbol())
                .collect()
        };

        draw(&mut terminal, &mut ratcn);
        assert!(row(&terminal, 0).contains("Alpha"), "{}", row(&terminal, 0));

        assert_eq!(
            ratcn.handle_event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 2, 0),
                &state
            ),
            EventResult::Consumed,
            "there is no scroll binding, so nothing is emitted"
        );
        draw(&mut terminal, &mut ratcn);
        assert!(
            row(&terminal, 0).contains("Delta"),
            "the wheel scrolled the view and the offset survived the redraw: {}",
            row(&terminal, 0)
        );
    }

    /// Returning the cursor to where the wheel left it must not revive the
    /// parked view: the cursor the user just moved has to stay on screen.
    #[test]
    fn a_released_wheel_park_never_revives() {
        let mut state = State {
            focused: Some(Task::A),
            ..State::default()
        };
        let theme = Theme::default_dark();
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 3)).expect("terminal");
        let draw =
            |terminal: &mut Terminal<TestBackend>, ratcn: &mut Ratcn<State, Msg>, state: &State| {
                terminal
                    .draw(|frame| {
                        let area = frame.area();
                        ratcn.render(frame, state, &theme, |ctx| {
                            ctx.render_component(
                                ChildId::Static("list"),
                                List::new(six_items())
                                    .item_focus(|state: &State| state.focused, Msg::Focused),
                                area,
                            );
                        });
                    })
                    .expect("draw");
            };
        let row = |terminal: &Terminal<TestBackend>, row: u16| -> String {
            let buffer = terminal.backend().buffer();
            (0..buffer.area.width)
                .map(|column| buffer.cell((column, row)).expect("cell").symbol())
                .collect()
        };

        draw(&mut terminal, &mut ratcn, &state);
        ratcn.handle_event(
            mouse(MouseKind::Scroll(ScrollDirection::Down), 2, 0),
            &state,
        );
        draw(&mut terminal, &mut ratcn, &state);
        assert!(row(&terminal, 0).contains("Delta"), "{}", row(&terminal, 0));

        // Move the cursor off the anchor: the view follows it back.
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Focused(Task::B, 1))
        );
        state.focused = Some(Task::B);
        draw(&mut terminal, &mut ratcn, &state);
        assert!(row(&terminal, 0).contains("Bravo"), "{}", row(&terminal, 0));

        // Move it back onto the anchor. The park is spent, so the cursor
        // stays visible instead of the stale parked view returning.
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Up), &state),
            EventResult::Emit(Msg::Focused(Task::A, 0))
        );
        state.focused = Some(Task::A);
        draw(&mut terminal, &mut ratcn, &state);
        assert!(
            row(&terminal, 0).contains("Alpha"),
            "returning to the wheel anchor must not re-park the view: {}",
            row(&terminal, 0)
        );
    }

    /// The park is anchored to the item the cursor sits on, not to its row.
    /// Swapping that item for another at the same index puts a different item
    /// under the cursor, so the hold ends and the new item is scrolled in.
    #[test]
    fn replacing_the_anchored_item_releases_the_wheel_park() {
        let mut driver = TestBackendDriver::new(20, 3);
        let mut state = State {
            focused: Some(Task::A),
            ..State::default()
        };
        driver.render(&state, six_items());

        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 2, 0),
                &state
            ),
            EventResult::Emit(Msg::Scrolled(3))
        );
        state.scroll = 3;
        driver.render(&state, six_items());
        assert!(
            driver.row(0).contains("Delta"),
            "the wheel parked the view away from the cursor: {}",
            driver.row(0)
        );

        let mut items = six_items().to_vec();
        items[0] = item(Task::G, "Golf");
        state.focused = Some(Task::G);
        driver.render(&state, items);

        assert!(
            driver.row(0).contains("Golf"),
            "a park anchored by row would still be holding Delta on screen: {}",
            driver.row(0)
        );
    }

    /// Hover moves the cursor from the very first pointer motion, with no
    /// paint round-trip first, exactly as it does over a `Select` panel.
    #[test]
    fn the_first_hover_motion_moves_the_cursor() {
        let mut driver = TestBackendDriver::new(20, 3);
        let state = State {
            focused: Some(Task::A),
            ..State::default()
        };
        driver.render(&state, six_items());

        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 2, 1), &state),
            EventResult::Emit(Msg::Focused(Task::B, 0)),
            "an unfocused, not-yet-hovered list still tracks the pointer"
        );
    }

    #[test]
    fn horizontal_wheel_directions_are_ignored() {
        let mut list =
            List::new([item(Task::A, "Alpha")]).scroll(|state: &State| state.scroll, Msg::Scrolled);
        let state = State::default();

        for direction in [ScrollDirection::Left, ScrollDirection::Right] {
            assert_eq!(
                list.handle_event(
                    &mouse(MouseKind::Scroll(direction), 0, 0),
                    &state,
                    &mut EventCtx::default(),
                ),
                EventResult::Ignored
            );
        }
    }

    #[test]
    fn cursorless_list_is_not_a_keyboard_stop_and_ignores_keys() {
        let theme = Theme::default_dark();
        let state = State::default();
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 2)).expect("terminal");
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        "list",
                        List::new([item(Task::A, "Alpha")])
                            .selection(|state: &State| state.selected, Msg::Selected),
                        Rect::new(0, 0, 20, 1),
                    );
                    ctx.render_component(
                        "button",
                        crate::Button::new("Next").on_press(|| Msg::Pressed),
                        Rect::new(0, 1, 20, 1),
                    );
                });
            })
            .expect("draw");

        assert_eq!(ratcn.focus_path(&[ChildId::Static("list")]), None);
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::Pressed)
        );
    }

    #[test]
    fn no_selection_mode_draws_neutral_rows_and_reports_unselected_custom_state() {
        let theme = Theme::default_dark();
        let state = State {
            focused: Some(Task::A),
            ..State::default()
        };
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 2)).expect("terminal");
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        "plain",
                        List::new([item(Task::A, "Alpha")])
                            .item_focus(|state: &State| state.focused, Msg::Focused),
                        Rect::new(0, 0, 20, 1),
                    );
                    ctx.render_component(
                        "custom",
                        List::new([item(Task::B, "Bravo")]).render_item(|_, row| {
                            Line::from(format!("{} selected={}", row.label, row.selected))
                        }),
                        Rect::new(0, 1, 20, 1),
                    );
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let row = |y: usize| {
            buffer.content[y * 20..(y + 1) * 20]
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        };
        assert!(row(0).starts_with("Alpha"));
        assert!(!row(0).contains(['○', '●', '□', '■']));
        assert!(row(1).starts_with("Bravo selected=false"));
    }

    #[test]
    fn hovered_list_paints_pointer_moved_item_focus_without_keyboard_focus() {
        let theme = Theme::default_dark();
        let mut state = State {
            focused: Some(Task::A),
            component_focus: crate::runtime::FocusState::intent(["other"]),
            ..State::default()
        };
        let mut ratcn =
            Ratcn::new().focus(|state: &State| &state.component_focus, Msg::ComponentFocus);
        let mut terminal = Terminal::new(TestBackend::new(20, 3)).expect("terminal");
        let render =
            |terminal: &mut Terminal<TestBackend>, ratcn: &mut Ratcn<State, Msg>, state: &State| {
                terminal
                    .draw(|frame| {
                        ratcn.render(frame, state, &theme, |ctx| {
                            ctx.render_component(
                                "other",
                                crate::Button::<Msg>::new("Other"),
                                Rect::new(0, 0, 20, 1),
                            );
                            ctx.render_component(
                                "list",
                                List::new([item(Task::A, "Alpha"), item(Task::B, "Bravo")])
                                    .item_focus(|state: &State| state.focused, Msg::Focused)
                                    .render_item(|_, row| {
                                        Line::from(format!(
                                            "{} {}",
                                            if row.focused { "focused" } else { "idle" },
                                            row.label
                                        ))
                                    })
                                    .focus_symbol(">"),
                                Rect::new(0, 1, 20, 2),
                            );
                        });
                    })
                    .expect("draw");
            };

        render(&mut terminal, &mut ratcn, &state);
        // One motion does both halves: the runtime writes its own hover as the
        // pointer enters, and the same event still reaches the List, whose
        // cursor follows it.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 2, 2), &state),
            EventResult::Emit(Msg::Focused(Task::B, 0))
        );
        state.focused = Some(Task::B);
        render(&mut terminal, &mut ratcn, &state);

        assert!(
            terminal
                .backend()
                .buffer()
                .cell((0, 2))
                .is_some_and(|cell| cell.symbol() == ">")
        );
        let buffer = terminal.backend().buffer();
        let painted_row: String = buffer.content[40..60]
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert!(painted_row.contains("focused Bravo"));
        assert_eq!(
            state.component_focus,
            crate::runtime::FocusState::intent(["other"])
        );
    }

    /// A list with nothing enabled has nowhere for a cursor to go, so it must
    /// consume no key at all: every one bubbles to the app. Nothing single
    /// enforces this — `linear_nav::nav_key_target` declines, the commit keys
    /// find a disabled cursor, and everything else falls through — so it is
    /// pinned here against a future key arm that forgets to ask.
    #[test]
    fn an_all_disabled_list_consumes_no_key() {
        let all_disabled = || {
            [
                item(Task::A, "A").disabled(true),
                item(Task::B, "B").disabled(true),
                item(Task::C, "C").disabled(true),
            ]
        };
        let keys = || {
            [
                key(KeyCode::Up),
                key(KeyCode::Down),
                key(KeyCode::PageUp),
                key(KeyCode::PageDown),
                key(KeyCode::Home),
                key(KeyCode::End),
                key(KeyCode::Char('j')),
                key(KeyCode::Char('k')),
                ctrl_key('n'),
                ctrl_key('p'),
                ctrl_key('d'),
                ctrl_key('u'),
                key(KeyCode::Enter),
                key(KeyCode::Char(' ')),
            ]
        };
        // A cursor is parked on one of the disabled items: the state that used
        // to name an enabled item still names one after it is disabled.
        let state = State {
            focused: Some(Task::B),
            component_focus: crate::runtime::FocusState::intent(["list"]),
            ..State::default()
        };
        let area = Rect::new(0, 0, 20, 3);

        // Driven directly as a `Component`.
        let mut list = List::new(all_disabled())
            .item_focus(|state: &State| state.focused, Msg::Focused)
            .selection(|state: &State| state.selected, Msg::Selected);
        for event in keys() {
            assert_eq!(
                list.handle_event(&event, &state, &mut EventCtx::default().with_area(area)),
                EventResult::Ignored,
                "{event:?} handled directly"
            );
        }

        // Driven through the runtime with focus stored on the list, which is
        // how a real app reaches it: a stored path routes keys to the component
        // it names whether or not that component would accept new focus.
        let theme = Theme::default_dark();
        let mut ratcn =
            Ratcn::new().focus(|state: &State| &state.component_focus, Msg::ComponentFocus);
        let mut terminal = Terminal::new(TestBackend::new(20, 3)).expect("terminal");
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        "list",
                        List::new(all_disabled())
                            .item_focus(|state: &State| state.focused, Msg::Focused)
                            .selection(|state: &State| state.selected, Msg::Selected),
                        area,
                    );
                });
            })
            .expect("draw");
        for event in keys() {
            assert_eq!(
                ratcn.handle_event(event.clone(), &state),
                EventResult::Ignored,
                "{event:?} routed through the runtime"
            );
        }
    }

    #[test]
    fn home_end_and_page_keys_skip_disabled_items_using_rendered_height() {
        let mut driver = TestBackendDriver::new(20, 3);
        let state = State {
            focused: Some(Task::C),
            ..State::default()
        };
        let items = [
            item(Task::A, "A").disabled(true),
            item(Task::B, "B"),
            item(Task::C, "C"),
            item(Task::D, "D"),
            item(Task::E, "E").disabled(true),
            item(Task::F, "F"),
        ];
        driver.render(&state, items.clone());

        assert_eq!(
            driver.event(key(KeyCode::Home), &state),
            EventResult::Emit(Msg::Focused(Task::B, 0))
        );
        assert_eq!(
            driver.event(key(KeyCode::End), &state),
            EventResult::Emit(Msg::Focused(Task::F, 3))
        );
        assert_eq!(
            driver.event(key(KeyCode::PageDown), &state),
            EventResult::Emit(Msg::Focused(Task::F, 3))
        );
        assert_eq!(
            driver.event(key(KeyCode::PageUp), &state),
            EventResult::Emit(Msg::Focused(Task::B, 0))
        );

        let leap_items = [
            item(Task::A, "A"),
            item(Task::B, "B"),
            item(Task::C, "C"),
            item(Task::D, "D").disabled(true),
            item(Task::E, "E").disabled(true),
            item(Task::F, "F"),
        ];
        driver.render(&state, leap_items);
        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Focused(Task::F, 3)),
            "a disabled leap scrolls far enough to expose its enabled target"
        );
    }

    #[test]
    fn multi_selection_toggles_the_clicked_value() {
        let mut list = List::new([item(Task::A, "Alpha")]).multi_selection(
            |state: &State, value| state.toggled.contains(value),
            Msg::Toggled,
        );
        assert_eq!(
            list.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), 1, 0),
                &State::default(),
                &mut EventCtx::default().with_area(Rect::new(0, 0, 10, 1))
            ),
            EventResult::Emit(Msg::Toggled(Task::A))
        );
    }

    #[test]
    fn a_list_without_selection_bubbles_enter_and_clicks_move_focus() {
        let mut list = List::new([item(Task::A, "Alpha"), item(Task::B, "Bravo")])
            .item_focus(|state: &State| state.focused, Msg::Focused);
        let state = State {
            focused: Some(Task::A),
            ..State::default()
        };
        let area = Rect::new(0, 0, 10, 2);

        assert_eq!(
            list.handle_event(
                &key(KeyCode::Enter),
                &state,
                &mut EventCtx::default().with_area(area)
            ),
            EventResult::Ignored
        );
        assert_eq!(
            list.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), 1, 1),
                &state,
                &mut EventCtx::default().with_area(area)
            ),
            EventResult::Emit(Msg::Focused(Task::B, 0))
        );
    }

    #[test]
    fn conflicting_selection_modes_fail_during_declaration() {
        let mut driver = TestBackendDriver::new(20, 1);
        let state = State::default();
        let panic = catch_unwind(AssertUnwindSafe(|| {
            let theme = Theme::default_dark();
            driver
                .terminal
                .draw(|frame| {
                    let area = frame.area();
                    driver.ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(
                            "list",
                            List::new([item(Task::A, "Alpha")])
                                .selection(|_: &State| None, Msg::Selected)
                                .multi_selection(|_, _| false, Msg::Toggled),
                            area,
                        );
                    });
                })
                .expect("draw");
        }))
        .expect_err("conflicting modes must panic");
        let message = panic.downcast_ref::<String>().map_or_else(
            || {
                panic
                    .downcast_ref::<&str>()
                    .copied()
                    .unwrap_or_default()
                    .to_owned()
            },
            Clone::clone,
        );
        assert!(message.contains("List::selection(...)"));
        assert!(message.contains("List::multi_selection(...)"));
    }

    // The check is a debug-build assertion, so this contract only exists where
    // debug assertions do.
    #[test]
    #[cfg(debug_assertions)]
    fn duplicate_item_values_fail_declaration_with_the_documented_panic() {
        let mut list: List<Task, State, Msg> =
            List::new([item(Task::A, "First"), item(Task::A, "Second")]);

        let panic = catch_unwind(AssertUnwindSafe(|| {
            Component::prepare(&mut list, &State::default());
        }))
        .expect_err("duplicate item values must panic");
        let message = panic.downcast_ref::<String>().map_or_else(
            || {
                panic
                    .downcast_ref::<&str>()
                    .copied()
                    .unwrap_or_default()
                    .to_owned()
            },
            Clone::clone,
        );

        assert_eq!(
            message,
            "List item values must be unique within a List declaration"
        );
    }

    // `j`/`k` and the Ctrl chords are the same navigation gesture as the arrow
    // keys, and go out through the same item-focus message.
    #[test]
    fn vim_and_readline_keys_step_the_cursor_like_the_arrows() {
        let mut driver = TestBackendDriver::new(20, 6);
        let state = State {
            focused: Some(Task::B),
            ..State::default()
        };
        let items = || {
            [
                item(Task::A, "Alpha"),
                item(Task::B, "Bravo"),
                item(Task::C, "Charlie"),
            ]
        };
        driver.render(&state, items());

        for down in [key(KeyCode::Char('j')), ctrl_key('n')] {
            assert_eq!(
                driver.event(down, &state),
                EventResult::Emit(Msg::Focused(Task::C, 0)),
                "j and Ctrl+N step forward like Down"
            );
        }
        for up in [key(KeyCode::Char('k')), ctrl_key('p')] {
            assert_eq!(
                driver.event(up, &state),
                EventResult::Emit(Msg::Focused(Task::A, 0)),
                "k and Ctrl+P step backward like Up"
            );
        }
    }

    #[test]
    fn a_letter_that_is_not_a_navigation_key_bubbles_as_an_app_hotkey() {
        let mut driver = TestBackendDriver::new(20, 6);
        let state = State {
            focused: Some(Task::A),
            ..State::default()
        };
        driver.render(&state, [item(Task::A, "Alpha"), item(Task::B, "Bravo")]);

        assert_eq!(
            driver.event(key(KeyCode::Char('a')), &state),
            EventResult::Ignored,
            "there is no typeahead: plain letters belong to the app"
        );
        assert_eq!(
            driver.event(key(KeyCode::Char('J')), &state),
            EventResult::Ignored,
            "Shift+j is not j, so it is not navigation either"
        );
    }

    #[test]
    fn space_commits_the_single_selection_cursor() {
        let mut list = List::new([item(Task::A, "Alpha"), item(Task::B, " Bravo")])
            .item_focus(|state: &State| state.focused, Msg::Focused)
            .selection(|state: &State| state.selected, Msg::Selected);
        let state = State {
            focused: Some(Task::A),
            ..State::default()
        };

        assert_eq!(
            list.handle_event(
                &key(KeyCode::Char(' ')),
                &state,
                &mut EventCtx::default().with_area(Rect::new(0, 0, 20, 2)),
            ),
            EventResult::Emit(Msg::Selected(Task::A)),
            "Space is a commit key on a list, not text: it commits the cursor row"
        );
    }

    #[test]
    fn space_toggles_the_multi_selection_cursor() {
        let mut list = List::new([item(Task::A, "Alpha"), item(Task::B, " Bravo")])
            .item_focus(|state: &State| state.focused, Msg::Focused)
            .multi_selection(
                |state: &State, value| state.toggled.contains(value),
                Msg::Toggled,
            );
        let state = State {
            focused: Some(Task::A),
            ..State::default()
        };

        assert_eq!(
            list.handle_event(
                &key(KeyCode::Char(' ')),
                &state,
                &mut EventCtx::default().with_area(Rect::new(0, 0, 20, 2)),
            ),
            EventResult::Emit(Msg::Toggled(Task::A)),
            "Space is a commit key on a list, not text: it toggles the cursor row"
        );
    }

    /// A long list costs a screenful of rows, not a listful. The window that
    /// paints is the only part built, and the indices it carries stay global,
    /// so a custom row closure and the click that lands on its row agree on
    /// which item they mean.
    #[test]
    fn a_scrolled_list_builds_only_its_window_and_keeps_indices_global() {
        #[derive(Default)]
        struct BigState {
            scroll: usize,
        }
        #[derive(Debug, PartialEq)]
        enum BigMsg {
            Scrolled(usize),
            Chose(usize),
        }

        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = std::rc::Rc::clone(&seen);
        let items: Vec<ListItem<usize>> = (0..1000)
            .map(|index| ListItem::new(index, format!("Item {index}")))
            .collect();
        let state = BigState { scroll: 40 };
        let theme = Theme::default_dark();
        let mut ratcn = Ratcn::<BigState, BigMsg>::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 5)).expect("terminal");

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("list"),
                        List::new(items)
                            .scroll(|state: &BigState| state.scroll, BigMsg::Scrolled)
                            .selection(|_: &BigState| None, BigMsg::Chose)
                            .render_item(move |_: &BigState, row: ListItemState<'_, usize>| {
                                recorded.borrow_mut().push(row.index);
                                Line::from(row.label.to_string())
                            }),
                        area,
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            *seen.borrow(),
            (40..45).collect::<Vec<usize>>(),
            "one screenful of rows is built, whatever the list's length"
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Click(MouseButton::Left), 2, 2), &state),
            EventResult::Emit(BigMsg::Chose(42)),
            "the third painted row is the forty-third item"
        );
    }

    #[test]
    fn events_before_the_first_render_are_ignored() {
        let mut driver = TestBackendDriver::new(20, 2);
        assert_eq!(
            driver.event(key(KeyCode::PageDown), &State::default()),
            EventResult::Ignored
        );
    }
}
