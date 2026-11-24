use std::{fmt, rc::Rc, sync::Arc};

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Flex, Layout, Position, Rect},
    style::{Color, Style, Styled},
    text::{Line, Span},
    widgets::{Block, Cell, Row, StatefulWidget, Table as NativeTable, TableState, Widget},
};

use crate::Theme;
use crate::color::{
    DISABLED_DIM, FIELD_FOCUS_SHIFT, FIELD_HOVER_SHIFT, ROW_FOCUS_SHIFT, away_from, blendable, dim,
};
use crate::linear_nav::{self, Axis, NavOutcome};
use crate::list_core::{
    self, KeyIntent, ListItem, ListItemState, PressedItem, RowIntent, RowViewport, WheelHold,
};
use crate::runtime::{
    Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, KeyEvent, MouseButton, MouseKind,
    PaintCtx, PointerShape, ScopeOptions, Step,
};
use crate::theme::resolve_style;

/// Every color a table paints.
///
/// A table is a list with columns, so its body derives from the theme exactly
/// as [`ListStyle`](crate::ListStyle) does: the same well, the same focus and
/// hover shifts, the same cursor row, and the same muted text that brightens
/// for the cursor and the selection. A table beside a list reads as one family.
///
/// The roles past the list's are the table's own:
///
/// - **The column cursor.** With a column cursor bound, the cursor row drops to
///   [`focused_band_background`](Self::focused_band_background) and only the
///   active cell keeps the cursor row's fill, so the cursor reads as a cell.
///   The active column's heading brightens to
///   [`active_header_foreground`](Self::active_header_foreground).
/// - **The header.** Headings are bold, muted like ordinary rows. A sortable
///   heading under the pointer takes the hover fill; when the cursor itself is
///   on the header, its cell paints like the cursor row.
///
/// [`from_theme`](Self::from_theme) derives all of this from a [`Theme`]; build
/// one by hand only for colors the theme cannot express.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableStyle {
    /// Text of an ordinary row.
    pub foreground: Color,
    /// Backdrop while the table has neither focus nor the pointer.
    pub background: Color,
    /// Backdrop while the table has focus.
    pub focused_background: Color,
    /// Backdrop while the table is hovered. Hover wins over focus.
    pub hovered_background: Color,
    /// Text of a selected row the cursor is not on. Selection has no fill.
    pub selected_foreground: Color,
    /// Text of the cursor row when it is not selected.
    pub focused_foreground: Color,
    /// Fill behind the cursor row, or behind the cursor cell when a column
    /// cursor is shown.
    pub focused_row_background: Color,
    /// Text of the cursor row when it is also selected.
    pub selected_focused_foreground: Color,
    /// Fill behind the cursor row (or cell) when it is also selected.
    pub selected_focused_background: Color,
    /// Fill behind the rest of the cursor row while a column cursor is shown.
    pub focused_band_background: Color,
    /// Text of a heading.
    pub header_foreground: Color,
    /// Text of the active column's heading.
    pub active_header_foreground: Color,
    /// Text of a sortable heading under the pointer.
    pub hovered_header_foreground: Color,
    /// Fill behind a sortable heading under the pointer.
    pub hovered_header_background: Color,
    /// Text of a disabled row, or of the whole disabled table.
    pub disabled_foreground: Color,
    /// Fill behind a disabled row, or behind the whole disabled table.
    pub disabled_background: Color,
}

impl TableStyle {
    /// Neutral ANSI styling for painting without a [`Theme`].
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            foreground: Color::Reset,
            background: Color::Reset,
            focused_background: Color::Reset,
            hovered_background: Color::DarkGray,
            // A list marks selection with a marker; a table row has only its text.
            selected_foreground: Color::LightCyan,
            focused_foreground: Color::Black,
            focused_row_background: Color::Cyan,
            selected_focused_foreground: Color::Black,
            selected_focused_background: Color::Cyan,
            focused_band_background: Color::Gray,
            header_foreground: Color::Reset,
            active_header_foreground: Color::LightCyan,
            hovered_header_foreground: Color::Black,
            hovered_header_background: Color::Gray,
            disabled_foreground: Color::DarkGray,
            disabled_background: Color::Reset,
        }
    }

    /// Derive every table color from `theme`, rung for rung as a list does.
    ///
    /// The band and the hovered heading sit one hover step above the focused
    /// well, between the hovered well and the cursor row, so they read as
    /// lesser than the cursor while staying on the ladder. Text on them is the
    /// ordinary foreground, which the theme already guarantees on the higher
    /// cursor rung.
    #[must_use]
    pub fn from_theme(theme: &Theme) -> Self {
        let backdrop = blendable(theme.background, theme.surface);
        let away = away_from(theme.background);
        let focused_background = dim(theme.field, away, FIELD_FOCUS_SHIFT);
        let focused_row_background = dim(focused_background, away, ROW_FOCUS_SHIFT);
        let band = dim(focused_background, away, FIELD_HOVER_SHIFT);
        Self {
            foreground: theme.muted_foreground,
            background: theme.field,
            focused_background,
            hovered_background: dim(theme.field, away, FIELD_HOVER_SHIFT),
            selected_foreground: theme.foreground,
            focused_foreground: theme.foreground,
            focused_row_background,
            selected_focused_foreground: theme.foreground,
            selected_focused_background: focused_row_background,
            focused_band_background: band,
            header_foreground: theme.muted_foreground,
            active_header_foreground: theme.foreground,
            hovered_header_foreground: theme.foreground,
            hovered_header_background: band,
            disabled_foreground: dim(theme.muted_foreground, backdrop, DISABLED_DIM),
            disabled_background: dim(theme.field, backdrop, DISABLED_DIM),
        }
    }

    const fn resolve_surface(&self, focused: bool, hovered: bool, disabled: bool) -> Color {
        if disabled {
            self.disabled_background
        } else if hovered {
            self.hovered_background
        } else if focused {
            self.focused_background
        } else {
            self.background
        }
    }

    /// One body row: the cursor and the selection independently, disabled
    /// over both.
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
        let (foreground, fill) = match (focused, selected) {
            (true, true) => (
                self.selected_focused_foreground,
                self.selected_focused_background,
            ),
            (true, false) => (self.focused_foreground, self.focused_row_background),
            (false, true) => (self.selected_foreground, background),
            (false, false) => (self.foreground, background),
        };
        Style::default().fg(foreground).bg(fill)
    }

    /// The fill a cursor cell or cursor heading takes.
    const fn cursor(&self, selected: bool) -> Style {
        let (foreground, background) = if selected {
            (
                self.selected_focused_foreground,
                self.selected_focused_background,
            )
        } else {
            (self.focused_foreground, self.focused_row_background)
        };
        Style::new().fg(foreground).bg(background)
    }
}

/// Themed Ratatui table. Render with `render_widget` or pass a native
/// [`TableState`] to `render_stateful_widget`; while [`focused`](Self::focused),
/// the native selected row paints as the cursor row.
///
/// ```
/// use ratatui::{layout::Constraint, widgets::Row};
/// use ratcn::{TableWidget, Theme};
///
/// let theme = Theme::default_dark();
/// let _widget = TableWidget::new(
///     [Row::new(["Alice", "$42.00"])],
///     [Constraint::Fill(1), Constraint::Length(8)],
/// )
/// .header(Row::new(["Name", "Amount"]))
/// .themed(&theme)
/// .focused(true);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableWidget<'a> {
    native: NativeTable<'a>,
    header: Option<Row<'a>>,
    footer: Option<Row<'a>>,
    style: TableStyle,
    focused: bool,
    hovered: bool,
    disabled: bool,
}

impl<'a> TableWidget<'a> {
    /// Accept native rows, cells, and width constraints, including borrowed text.
    #[must_use]
    pub fn new(
        rows: impl IntoIterator<Item = impl Into<Row<'a>>>,
        widths: impl IntoIterator<Item = impl Into<Constraint>>,
    ) -> Self {
        Self {
            native: NativeTable::new(rows, widths),
            header: None,
            footer: None,
            style: TableStyle::fallback(),
            focused: false,
            hovered: false,
            disabled: false,
        }
    }

    /// A fixed native header, including its height, margins and cell styles.
    #[must_use]
    pub fn header(mut self, row: Row<'a>) -> Self {
        self.header = Some(row);
        self
    }

    /// A fixed native footer.
    #[must_use]
    pub fn footer(mut self, row: Row<'a>) -> Self {
        self.footer = Some(row);
        self
    }

    /// Apply theme colors.
    #[must_use]
    pub fn themed(self, theme: &Theme) -> Self {
        self.style(TableStyle::from_theme(theme))
    }

    /// Use an explicit set of colors.
    #[must_use]
    pub const fn style(mut self, style: TableStyle) -> Self {
        self.style = style;
        self
    }

    /// Paint as the focused control: the focus backdrop and the cursor row.
    /// Unfocused, the selected row paints as an ordinary one, so two tables
    /// side by side cannot both look active.
    #[must_use]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Paint the hovered backdrop. Hover wins when the table is also focused.
    #[must_use]
    pub const fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Paint the whole table as disabled, with no cursor row. Colors set on
    /// native rows and cells are left to the caller.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Finish styling and use any native builder (block, flex, spacing,
    /// highlight symbol/spacing, or individual highlight styles).
    #[must_use]
    pub fn into_inner(self) -> NativeTable<'a> {
        let style = self.style;
        let surface = style.resolve_surface(self.focused, self.hovered, self.disabled);
        let (text, header) = if self.disabled {
            (style.disabled_foreground, style.disabled_foreground)
        } else {
            (style.foreground, style.header_foreground)
        };
        let cursor = if self.focused && !self.disabled {
            style.cursor(false)
        } else {
            Style::default()
        };
        let mut table = self
            .native
            .style(Style::default().fg(text).bg(surface))
            .row_highlight_style(cursor)
            .column_highlight_style(Style::default())
            .cell_highlight_style(Style::default());
        if let Some(row) = self.header {
            table = table.header(styled_row(row, Style::default().fg(header).bold()));
        }
        if let Some(row) = self.footer {
            table = table.footer(styled_row(row, Style::default().fg(text)));
        }
        table
    }
}

/// `base` beneath whatever style the row already carries.
fn styled_row(row: Row<'_>, base: Style) -> Row<'_> {
    let style = base.patch(Styled::style(&row));
    row.style(style)
}

impl Widget for TableWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Widget::render(self.into_inner(), area, buf);
    }
}

impl StatefulWidget for TableWidget<'_> {
    type State = TableState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut TableState) {
        StatefulWidget::render(self.into_inner(), area, buf, state);
    }
}

/// Requested ordering of one column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    /// Smallest first.
    Ascending,
    /// Largest first.
    Descending,
}

/// Zero-based column and order. The app sorts the underlying records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableSort {
    /// Index in the declared column widths.
    pub column: usize,
    /// Sort direction.
    pub direction: SortDirection,
}

type Read<S, T> = Box<dyn Fn(&S) -> Option<T>>;
type Binding<S, T, M> = (Read<S, T>, Box<dyn Fn(T) -> M>);
type FocusBinding<S, T, M> = (Read<S, T>, Box<dyn Fn(T, usize) -> M>);
type HeaderFocusBinding<S, M> = (Box<dyn Fn(&S) -> bool>, Box<dyn Fn() -> M>);
type ScrollBinding<S, M> = (Box<dyn Fn(&S) -> usize>, Box<dyn Fn(usize) -> M>);
type PaintItem<S, T> = Box<dyn Fn(&S, ListItemState<'_, T>) -> Row<'static>>;
type StyleFn = Box<dyn Fn(&Theme) -> TableStyle>;

#[derive(Debug, PartialEq, Clone)]
enum Hit<T> {
    Row(T),
    Header(usize),
}

/// An interactive table of value-keyed items. Like `List`, the app owns the
/// cursor, committed selection and ordering. Only visible items are formatted.
/// Supply one width per column; the default row displays the item's label.
///
/// Bind `column_focus` for Left/Right movement. Clicking a sortable heading
/// sorts by it; bind `header_focus` too and the cursor can go above the first
/// row onto the header, where Enter or Space sorts. Tab and Shift+Tab remain
/// ordinary component traversal.
///
/// ```
/// use ratatui::{layout::Constraint, widgets::Row};
/// use ratcn::{ListItem, Table, TableSort};
///
/// # #[derive(Default)]
/// # struct AppState {
/// #     row: Option<u64>,
/// #     on_header: bool,
/// #     selected: Option<u64>,
/// #     column: usize,
/// #     sort: Option<TableSort>,
/// # }
/// # enum Msg { Row(u64, usize), Header, Select(u64), Column(usize), Sort(TableSort) }
/// let _table = Table::new(
///     [ListItem::new(1u64, "Groceries"), ListItem::new(2, "Rent")],
///     [Constraint::Fill(1), Constraint::Length(10)],
/// )
/// .header(["Description", "Amount"])
/// .paint_item(|_: &AppState, row| Row::new([row.label.to_owned(), "$42.00".to_owned()]))
/// .item_focus(|s| s.row, Msg::Row)
/// .header_focus(|s| s.on_header, || Msg::Header)
/// .selection(|s| s.selected, Msg::Select)
/// .column_focus(|s| Some(s.column), Msg::Column)
/// .sort([0, 1], |s| s.sort, Msg::Sort);
/// ```
pub struct Table<T, S, M> {
    items: Arc<[ListItem<T>]>,
    widths: Vec<Constraint>,
    header: Option<Vec<Line<'static>>>,
    footer: Option<Vec<Line<'static>>>,
    block: Option<Block<'static>>,
    spacing: u16,
    flex: Flex,
    focused: Option<FocusBinding<S, T, M>>,
    header_focus: Option<HeaderFocusBinding<S, M>>,
    selected: Option<Binding<S, T, M>>,
    column: Option<Binding<S, usize, M>>,
    sort: Option<Binding<S, TableSort, M>>,
    sortable: Vec<usize>,
    scroll: Option<ScrollBinding<S, M>>,
    paint_item: Option<PaintItem<S, T>>,
    style: Option<StyleFn>,
    disabled: bool,
    viewport: RowViewport,
    body: Rect,
    head: Rect,
    foot: Rect,
    columns: Rc<[Rect]>,
}

impl<T, S, M> fmt::Debug for Table<T, S, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Table")
            .field("rows", &self.items.len())
            .field("widths", &self.widths)
            .field("disabled", &self.disabled)
            .finish_non_exhaustive()
    }
}

impl<T, S, M> Table<T, S, M> {
    /// Build a table over items identified by unique values, in display order.
    #[must_use]
    pub fn new(
        items: impl IntoIterator<Item = impl Into<ListItem<T>>>,
        widths: impl IntoIterator<Item = impl Into<Constraint>>,
    ) -> Self {
        Self::from_shared(items.into_iter().map(Into::into).collect(), widths)
    }

    /// Reuse app-held items across frame declarations.
    #[must_use]
    pub fn from_shared(
        items: Arc<[ListItem<T>]>,
        widths: impl IntoIterator<Item = impl Into<Constraint>>,
    ) -> Self {
        Self {
            items,
            widths: widths.into_iter().map(Into::into).collect(),
            header: None,
            footer: None,
            block: None,
            spacing: 1,
            flex: Flex::Start,
            focused: None,
            header_focus: None,
            selected: None,
            column: None,
            sort: None,
            sortable: Vec::new(),
            scroll: None,
            paint_item: None,
            style: None,
            disabled: false,
            viewport: RowViewport::new(1),
            body: Rect::default(),
            head: Rect::default(),
            foot: Rect::default(),
            columns: Rc::default(),
        }
    }

    /// Fixed one-line header, one heading per column. A heading keeps its own
    /// spans, style and alignment; a sorted one gains a `▲`/`▼` mark on the
    /// side away from its alignment.
    #[must_use]
    pub fn header(mut self, headings: impl IntoIterator<Item = impl Into<Line<'static>>>) -> Self {
        self.header = Some(headings.into_iter().map(Into::into).collect());
        self
    }
    /// Fixed one-line footer, outside the scrollable body.
    #[must_use]
    pub fn footer(mut self, cells: impl IntoIterator<Item = impl Into<Line<'static>>>) -> Self {
        self.footer = Some(cells.into_iter().map(Into::into).collect());
        self
    }
    /// Surround the table with a native block (including titles and padding).
    #[must_use]
    pub fn block(mut self, block: Block<'static>) -> Self {
        self.block = Some(block);
        self
    }
    /// Space between columns, one cell by default.
    #[must_use]
    pub const fn column_spacing(mut self, spacing: u16) -> Self {
        self.spacing = spacing;
        self
    }
    /// Position columns using Ratatui's layout policy.
    #[must_use]
    pub const fn flex(mut self, flex: Flex) -> Self {
        self.flex = flex;
        self
    }
    /// Uniform body-row height for paging and hit-testing. Row-local heights
    /// and margins are replaced; use `TableWidget` for variable-height rows.
    #[must_use]
    pub const fn row_height(mut self, height: u16) -> Self {
        self.viewport = RowViewport::new(height);
        self
    }
    /// Format a visible item with native cells, spans, alignment and styling.
    #[must_use]
    pub fn paint_item(
        mut self,
        paint: impl Fn(&S, ListItemState<'_, T>) -> Row<'static> + 'static,
    ) -> Self {
        self.paint_item = Some(Box::new(paint));
        self
    }
    /// Bind the row cursor and the resulting top-item offset, as on `List`.
    #[must_use]
    pub fn item_focus(
        mut self,
        read: impl Fn(&S) -> Option<T> + 'static,
        change: impl Fn(T, usize) -> M + 'static,
    ) -> Self {
        self.focused = Some((Box::new(read), Box::new(change)));
        self
    }
    /// Let the cursor go above the first row onto the header, where Left/Right
    /// choose a column and Enter or Space sorts it.
    ///
    /// `read` reports whether the cursor is currently on the header, and
    /// `on_enter` fires when stepping up onto it (from the first row, or from
    /// an empty table). Set header focus in `on_enter`'s update, and clear it
    /// in your `item_focus` and `selection` updates when moving back into the
    /// rows. Needs `item_focus`, `column_focus`, a header and a sortable
    /// column.
    #[must_use]
    pub fn header_focus(
        mut self,
        read: impl Fn(&S) -> bool + 'static,
        on_enter: impl Fn() -> M + 'static,
    ) -> Self {
        self.header_focus = Some((Box::new(read), Box::new(on_enter)));
        self
    }
    /// Commit with Enter, Space, or click. Set the cursor (and clear
    /// `header_focus`, if bound) in the click's update.
    #[must_use]
    pub fn selection(
        mut self,
        read: impl Fn(&S) -> Option<T> + 'static,
        select: impl Fn(T) -> M + 'static,
    ) -> Self {
        self.selected = Some((Box::new(read), Box::new(select)));
        self
    }
    /// Bind the column cursor for Left/Right (or h/l) and cell highlighting.
    ///
    /// Clicking a body cell commits its row through [`selection`](Self::selection)
    /// without changing the column cursor; if you also want clicking a sortable
    /// heading to move the active column, update it in [`sort`](Self::sort)'s
    /// message handler.
    #[must_use]
    pub fn column_focus(
        mut self,
        read: impl Fn(&S) -> Option<usize> + 'static,
        change: impl Fn(usize) -> M + 'static,
    ) -> Self {
        self.column = Some((Box::new(read), Box::new(change)));
        self
    }
    /// Enable sorting on the specified zero-based header columns. A new column
    /// starts ascending; activating it again reverses direction. Update the
    /// record order and store the request in the app's message handler (and
    /// update the active column too if [`column_focus`](Self::column_focus) is
    /// bound).
    #[must_use]
    pub fn sort(
        mut self,
        columns: impl IntoIterator<Item = usize>,
        read: impl Fn(&S) -> Option<TableSort> + 'static,
        change: impl Fn(TableSort) -> M + 'static,
    ) -> Self {
        self.sortable = columns.into_iter().collect();
        self.sort = Some((Box::new(read), Box::new(change)));
        self
    }
    /// Bind the scroll offset. Unbound tables still scroll, like `List`.
    #[must_use]
    pub fn scroll(
        mut self,
        read: impl Fn(&S) -> usize + 'static,
        change: impl Fn(usize) -> M + 'static,
    ) -> Self {
        self.scroll = Some((Box::new(read), Box::new(change)));
        self
    }
    /// Resolve custom colors against the current theme each frame.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> TableStyle + 'static) -> Self {
        self.style = Some(Box::new(style));
        self
    }
    /// Dim the control and remove it from input and focus traversal.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Number of columns that have both a width and a heading.
    fn header_columns(&self) -> usize {
        self.header
            .as_ref()
            .map_or(0, |headings| headings.len().min(self.widths.len()))
    }

    /// Whether the header has a heading a click or Enter can sort by.
    fn sortable_header(&self) -> bool {
        let count = self.header_columns();
        self.sortable.iter().any(|&i| i < count)
    }

    /// Whether the header is somewhere the cursor can go.
    fn header_is_a_place(&self) -> bool {
        self.header_focus.is_some()
            && self.focused.is_some()
            && self.column.is_some()
            && self.sortable_header()
    }
}

impl<T: Clone + PartialEq + 'static, S, M> Table<T, S, M> {
    fn cursor(&self, state: &S) -> Option<usize> {
        list_core::index_of(&self.items, &(self.focused.as_ref()?.0)(state)?)
    }
    /// Whether the cursor is currently on the header.
    fn on_header(&self, state: &S) -> bool {
        self.header_is_a_place()
            && self
                .header_focus
                .as_ref()
                .is_some_and(|(read, _)| read(state))
    }
    /// The column the header cursor is on: with no column cursor yet, the first.
    fn header_column(&self, state: &S) -> usize {
        self.active_column(state).unwrap_or(0)
    }
    fn active_column(&self, state: &S) -> Option<usize> {
        (self.column.as_ref()?.0)(state).filter(|&i| i < self.widths.len())
    }
    fn sort_by(&self, state: &S, column: usize) -> EventResult<M> {
        let Some((read, change)) = &self.sort else {
            return EventResult::Ignored;
        };
        if !self.sortable.contains(&column) || column >= self.header_columns() {
            return EventResult::Ignored;
        }
        let ascending = TableSort {
            column,
            direction: SortDirection::Ascending,
        };
        let request = if read(state) == Some(ascending) {
            TableSort {
                column,
                direction: SortDirection::Descending,
            }
        } else {
            ascending
        };
        EventResult::Emit(change(request))
    }
    fn move_to(&self, state: &S, index: usize) -> EventResult<M> {
        let Some((_, change)) = &self.focused else {
            return EventResult::Ignored;
        };
        let offset = self
            .scroll
            .as_ref()
            .map_or(self.viewport.painted_offset(), |(read, _)| read(state));
        EventResult::Emit(change(
            self.items[index].value().clone(),
            self.viewport
                .cursor_visible_offset(self.body, self.items.len(), offset, Some(index)),
        ))
    }
    fn select(&self, index: usize) -> EventResult<M> {
        self.selected
            .as_ref()
            .map_or(EventResult::Ignored, |(_, f)| {
                EventResult::Emit(f(self.items[index].value().clone()))
            })
    }
    fn key(&self, key: KeyEvent, state: &S) -> EventResult<M> {
        // On the header the cursor is always on a column, so step from there.
        let col = if self.on_header(state) {
            Some(self.header_column(state))
        } else {
            self.active_column(state)
        };
        if !key.modifiers.any()
            && matches!(
                key.code,
                KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l')
            )
            && let Some((_, change)) = &self.column
        {
            return match linear_nav::nav_key_target(
                key,
                Axis::Horizontal,
                self.widths.len(),
                col,
                1,
                |_| false,
            ) {
                Some(NavOutcome::Move(i)) => EventResult::Emit(change(i)),
                Some(NavOutcome::Stay) => EventResult::Consumed,
                None => EventResult::Ignored,
            };
        }
        if self.focused.is_none() {
            return EventResult::Ignored;
        }
        if self.on_header(state) {
            return self.header_key(key, state);
        }
        match list_core::key_intent(
            key,
            Axis::Vertical,
            &self.items,
            self.cursor(state),
            self.viewport.visible_items(self.body),
        ) {
            Some(KeyIntent::Move(i)) => self.move_to(state, i),
            Some(KeyIntent::Commit(i)) => self.select(i),
            // A step up from the first row climbs onto the header.
            Some(KeyIntent::Stay)
                if linear_nav::step_key(key, Axis::Vertical) == Some(Step::Backward) =>
            {
                self.header_focus
                    .as_ref()
                    .filter(|_| self.header_is_a_place())
                    .map_or(EventResult::Consumed, |(_, enter)| {
                        EventResult::Emit(enter())
                    })
            }
            Some(KeyIntent::Stay) => EventResult::Consumed,
            None if self.header_is_a_place()
                && !linear_nav::has_enabled(self.items.len(), |i| self.items[i].is_disabled())
                && linear_nav::step_key(key, Axis::Vertical).is_some() =>
            {
                self.header_focus
                    .as_ref()
                    .map_or(
                        EventResult::Ignored,
                        |(_, enter)| EventResult::Emit(enter()),
                    )
            }
            None => EventResult::Ignored,
        }
    }
    /// A key while the cursor is on the header, which sits above the first
    /// row: commit keys sort, keys that go down come into the rows, and keys
    /// that go up have nowhere to go.
    fn header_key(&self, key: KeyEvent, state: &S) -> EventResult<M> {
        if !key.modifiers.any() && matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
            return match self.sort_by(state, self.header_column(state)) {
                EventResult::Ignored => EventResult::Consumed,
                result => result,
            };
        }
        // Classify the vertical navigation direction through the shared keymap.
        let Some(NavOutcome::Move(dir)) =
            linear_nav::nav_key_target(key, Axis::Vertical, 3, Some(1), 1, |_| false)
        else {
            return EventResult::Ignored;
        };
        if dir == 0 {
            return EventResult::Consumed;
        }
        let Some(first) =
            linear_nav::first_enabled(self.items.len(), |i| self.items[i].is_disabled())
        else {
            return EventResult::Consumed;
        };
        if linear_nav::step_key(key, Axis::Vertical) == Some(Step::Forward) {
            return self.move_to(state, first);
        }
        // Edge or page keys going down land where they would from the first
        // row, or on the first row itself when it is the only enabled row.
        match list_core::key_intent(
            key,
            Axis::Vertical,
            &self.items,
            Some(first),
            self.viewport.visible_items(self.body),
        ) {
            Some(KeyIntent::Move(i)) => self.move_to(state, i),
            Some(KeyIntent::Stay) => self.move_to(state, first),
            Some(KeyIntent::Commit(_)) => EventResult::Consumed,
            None => EventResult::Ignored,
        }
    }
    /// The visible body rows, each styled for its cursor, selection and
    /// disabled state. With a column cursor shown, the cursor row is the band
    /// and the cursor cell is placed by the native cell highlight.
    fn body_rows(
        &self,
        state: &S,
        style: &TableStyle,
        surface: Color,
        cursor: Option<usize>,
        band: bool,
        selected: Option<&T>,
    ) -> Vec<Row<'static>> {
        let start = self.viewport.painted_offset().min(self.items.len());
        self.items
            .iter()
            .enumerate()
            .skip(start)
            .take(self.viewport.visible_items(self.body))
            .map(|(index, item)| {
                let row_state = ListItemState {
                    index,
                    value: item.value(),
                    label: item.label(),
                    focused: cursor == Some(index),
                    selected: selected == Some(item.value()),
                    disabled: self.disabled || item.is_disabled(),
                };
                let mut role = style.resolve_row(
                    row_state.focused,
                    row_state.selected,
                    row_state.disabled,
                    surface,
                );
                if row_state.focused && band {
                    role = role.bg(style.focused_band_background);
                }
                let row = self.paint_item.as_ref().map_or_else(
                    || Row::new([item.label().to_owned()]),
                    |paint| paint(state, row_state),
                );
                styled_row(row, role)
                    .height(self.viewport.rows_per_item())
                    .top_margin(0)
                    .bottom_margin(0)
            })
            .collect()
    }
    fn header_at(&self, point: Position) -> Option<usize> {
        let count = self.header_columns();
        self.columns
            .iter()
            .position(|r| r.contains(point))
            .filter(|&i| i < count && self.sortable.contains(&i))
    }

    /// The header as a native row: each heading with its sort mark and the
    /// state of its column. The cursor wins over the pointer, the pointer over
    /// the active column.
    fn header_row(
        &self,
        headings: &[Line<'static>],
        state: &S,
        style: &TableStyle,
        hover: Option<usize>,
        cursor_visible: bool,
    ) -> Row<'static> {
        let active = self.active_column(state).filter(|_| cursor_visible);
        let cursor = (cursor_visible && self.on_header(state)).then(|| self.header_column(state));
        let sorted = self
            .sort
            .as_ref()
            .and_then(|(read, _)| read(state))
            .filter(|s| s.column < self.header_columns() && self.sortable.contains(&s.column));
        Row::new(headings.iter().enumerate().map(|(i, heading)| {
            let mut heading = heading.clone();
            if let Some(sort) = sorted.filter(|s| s.column == i) {
                let col_width = self.columns.get(i).map_or(0, |r| usize::from(r.width));
                if heading.width().saturating_add(2) <= col_width {
                    let mark = Span::raw(match sort.direction {
                        SortDirection::Ascending => "▲",
                        SortDirection::Descending => "▼",
                    });
                    if heading.alignment == Some(Alignment::Right) {
                        heading.spans.splice(0..0, [mark, Span::raw(" ")]);
                    } else {
                        heading.spans.extend([Span::raw(" "), mark]);
                    }
                }
            }
            let cell = if self.disabled {
                Style::default()
            } else if cursor == Some(i) {
                style.cursor(false)
            } else if hover == Some(i) {
                Style::default()
                    .fg(style.hovered_header_foreground)
                    .bg(style.hovered_header_background)
            } else if active == Some(i) {
                Style::default().fg(style.active_header_foreground)
            } else {
                Style::default()
            };
            Cell::from(heading).style(cell)
        }))
    }
}

impl<T: Clone + PartialEq + 'static, S, M> Component<S, M> for Table<T, S, M> {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, S, M>) {
        if cfg!(debug_assertions) {
            list_core::assert_unique_values(self.items.iter().map(ListItem::value), "Table");
            assert!(
                self.header_focus.is_none() || self.header_is_a_place(),
                "Table::header_focus needs item_focus, column_focus, a header and a sortable column"
            );
        }
        let inner = self
            .block
            .as_ref()
            .map_or(ctx.area(), |b| b.inner(ctx.area()));
        [self.head, self.body, self.foot] = Layout::vertical([
            Constraint::Length(u16::from(self.header.is_some())),
            Constraint::Fill(1),
            Constraint::Length(u16::from(self.footer.is_some())),
        ])
        .areas(inner);
        self.body.height -= self.body.height % self.viewport.rows_per_item();
        self.columns = Layout::horizontal(self.widths.clone())
            .flex(self.flex)
            .spacing(self.spacing)
            .split(self.head);
        let cursor = (!self.on_header(ctx.state()))
            .then(|| self.cursor(ctx.state()))
            .flatten();
        let requested = self.scroll.as_ref().map(|(read, _)| read(ctx.state()));
        WheelHold::settle_in(
            ctx,
            &self.items,
            cursor,
            requested,
            &mut self.viewport,
            self.body,
        );
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, S>) {
        let state = ctx.state();
        let style = resolve_style(self.style.as_deref(), ctx.theme, TableStyle::from_theme);
        let area = ctx.area();
        // One cursor, shown while the table is focused or under the pointer,
        // exactly as a list shows its own.
        let interactive = !self.disabled && !self.widths.is_empty();
        let hovered = interactive && ctx.hovered();
        let cursor_visible = interactive && (ctx.focused() || hovered);
        let themed = |rows: Vec<Row<'static>>| {
            TableWidget::new(rows, self.widths.iter().copied())
                .style(style)
                .focused(cursor_visible)
                .hovered(hovered)
                .disabled(self.disabled)
        };
        let surface = style.resolve_surface(cursor_visible, hovered, self.disabled);
        let base_fg = if self.disabled {
            style.disabled_foreground
        } else {
            style.foreground
        };
        ctx.with_buffer(area, |a, b| {
            b.set_style(a, Style::default().fg(base_fg).bg(surface));
        });
        if let Some(block) = &self.block {
            ctx.widget(block, area);
        }
        let hover = ctx
            .hover_position()
            .and_then(|p| self.header_at(p))
            .filter(|_| interactive);
        if let Some(headings) = &self.header {
            let row = self.header_row(headings, state, &style, hover, cursor_visible);
            let widget = themed(Vec::new()).header(row).into_inner();
            ctx.widget(
                widget.flex(self.flex).column_spacing(self.spacing),
                self.head,
            );
        }
        if let Some(cells) = &self.footer {
            let widget = themed(Vec::new())
                .footer(Row::new(cells.clone()))
                .into_inner();
            ctx.widget(
                widget.flex(self.flex).column_spacing(self.spacing),
                self.foot,
            );
        }

        let on_header = self.on_header(state);
        let active = self.active_column(state).filter(|_| cursor_visible);
        let cursor = self
            .cursor(state)
            .filter(|&i| cursor_visible && !on_header && !self.items[i].is_disabled());
        let selected = self.selected.as_ref().and_then(|(read, _)| read(state));
        let start = self.viewport.painted_offset().min(self.items.len());
        let visible = self.viewport.visible_items(self.body);
        let rows = self.body_rows(
            state,
            &style,
            surface,
            cursor,
            active.is_some(),
            selected.as_ref(),
        );
        // The row already carries the cursor foreground; only swap the active
        // cell's fill so explicit span colors inside `paint_item` survive.
        let cursor_bg = if cursor.is_some_and(|i| selected.as_ref() == Some(self.items[i].value()))
        {
            style.selected_focused_background
        } else {
            style.focused_row_background
        };
        let table = themed(rows)
            .into_inner()
            .flex(self.flex)
            .column_spacing(self.spacing)
            .row_highlight_style(Style::default())
            .cell_highlight_style(Style::new().bg(cursor_bg));
        let mut native = TableState::default()
            .with_selected(
                cursor
                    .and_then(|i| i.checked_sub(start))
                    .filter(|&i| i < visible),
            )
            .with_selected_column(active);
        ctx.with_buffer(self.body, |r, b| {
            StatefulWidget::render(table, r, b, &mut native);
        });
        if interactive
            && (hover.is_some()
                || self.selected.is_some()
                    && ctx
                        .hover_position()
                        .and_then(|p| self.viewport.row_at(self.body, self.items.len(), p.x, p.y))
                        .is_some_and(|i| !self.items[i].is_disabled()))
        {
            ctx.set_pointer_shape(PointerShape::Pointer);
        }
    }

    fn handle_event(&mut self, event: &Event, state: &S, ctx: &mut EventCtx<'_>) -> EventResult<M> {
        if self.disabled || self.widths.is_empty() {
            return EventResult::Ignored;
        }
        match event {
            Event::Mouse(m) => {
                let cursor = (!self.on_header(state))
                    .then(|| self.cursor(state))
                    .flatten();
                if let MouseKind::Scroll(direction) = m.kind {
                    if self.body.is_empty() {
                        return EventResult::Ignored;
                    }
                    let Some(next) =
                        self.viewport
                            .wheel(direction, &self.items, cursor, self.body, ctx)
                    else {
                        return EventResult::Ignored;
                    };
                    return self
                        .scroll
                        .as_ref()
                        .filter(|(read, _)| read(state) != next)
                        .map_or(EventResult::Consumed, |(_, change)| {
                            EventResult::Emit(change(next))
                        });
                }
                let header = self.header_at(Position::new(m.column, m.row));
                let mut pressed_here = true;
                let row = self
                    .viewport
                    .row_at(self.body, self.items.len(), m.column, m.row);
                if matches!(
                    m.kind,
                    MouseKind::Down(MouseButton::Left) | MouseKind::Click(MouseButton::Left)
                ) {
                    let hit = header.map(Hit::Header).or_else(|| {
                        row.filter(|&i| !self.items[i].is_disabled())
                            .map(|i| Hit::Row(self.items[i].value().clone()))
                    });
                    if m.kind == MouseKind::Down(MouseButton::Left) {
                        PressedItem::press(ctx, hit);
                    } else if !PressedItem::releases_on(ctx, hit.as_ref()) {
                        pressed_here = false;
                    } else if let Some(i) = header {
                        return self.sort_by(state, i);
                    }
                }
                // As on a list, only a click that would act is vetoed when its
                // press landed somewhere else.
                match list_core::row_intent(
                    m.kind,
                    &self.items,
                    row,
                    cursor,
                    self.selected.is_some(),
                ) {
                    RowIntent::Focus(_) | RowIntent::Commit(_) if !pressed_here => {
                        EventResult::Consumed
                    }
                    RowIntent::Focus(i) => self.move_to(state, i),
                    RowIntent::Commit(i) => self.select(i),
                    RowIntent::BlockPress | RowIntent::Stay => EventResult::Consumed,
                    RowIntent::Bubble if header.is_some() && !pressed_here => EventResult::Consumed,
                    RowIntent::Bubble => EventResult::Ignored,
                }
            }
            Event::Key(key) => self.key(*key, state),
            _ => EventResult::Ignored,
        }
    }

    fn scope_options(&self, _: &S) -> ScopeOptions {
        ScopeOptions::default().focusable(
            !self.disabled
                && !self.widths.is_empty()
                && (self.header_is_a_place()
                    || self.focused.is_some()
                        && linear_nav::has_enabled(self.items.len(), |i| {
                            self.items[i].is_disabled()
                        })),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{FocusState, Ratcn, ScrollDirection};
    use crate::test_support::{Driver, key, mouse};
    use crate::{Button, List, ListStyle};

    #[derive(Default)]
    struct State {
        focus: FocusState,
        row: Option<u8>,
        on_header: bool,
        selected: Option<u8>,
        column: usize,
        sort: Option<TableSort>,
    }
    #[derive(Debug, PartialEq)]
    enum Msg {
        Focus(FocusState),
        Move(u8, usize),
        Header,
        Select(u8),
        Column(usize),
        Sort(TableSort),
        Button,
    }
    fn table(state: &State, disabled: bool) -> Table<u8, State, Msg> {
        let ids = if state.sort.is_some() {
            [6, 5, 4, 3, 2, 1, 0]
        } else {
            [0, 1, 2, 3, 4, 5, 6]
        };
        Table::new(
            ids.map(|id| ListItem::new(id, id.to_string()).disabled(id == 1)),
            [Constraint::Fill(1), Constraint::Length(6)],
        )
        .block(Block::bordered())
        .column_spacing(2)
        .header(["ID", "Amount"])
        .footer(["Total"])
        .row_height(2)
        .paint_item(|_, row| Row::new([format!("{}\n=", row.value), "42".to_owned()]))
        .item_focus(|s: &State| s.row, Msg::Move)
        .header_focus(|s| s.on_header, || Msg::Header)
        .selection(|s| s.selected, Msg::Select)
        .column_focus(|s| Some(s.column), Msg::Column)
        .sort([1], |s| s.sort, Msg::Sort)
        .disabled(disabled)
    }
    fn frame(driver: &mut Driver<State, Msg>, state: &State, width: u16, disabled: bool) {
        driver.render(state, |ctx| {
            ctx.component("table", table(state, disabled), Rect::new(1, 1, width, 8));
            ctx.component(
                "next",
                Button::new("Next").on_press(|| Msg::Button),
                Rect::new(1, 9, 12, 1),
            );
        });
    }
    fn focused_driver() -> Driver<State, Msg> {
        Driver::with(Ratcn::new().focus(|s: &State| &s.focus, Msg::Focus), 32, 12)
    }

    #[test]
    fn sort_tracks_record_identity_and_leaves_tab_to_the_runtime() {
        let mut driver = focused_driver();
        let mut state = State {
            row: Some(2),
            selected: Some(2),
            column: 1,
            ..State::default()
        };
        frame(&mut driver, &state, 25, false);
        let sort = TableSort {
            column: 1,
            direction: SortDirection::Ascending,
        };
        state.sort = Some(sort);
        frame(&mut driver, &state, 19, false);
        // A fully occupied column has no room for a sort mark. The label must
        // survive, including after a resize.
        assert!(driver.row(2).contains("Amount"));
        assert_eq!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::Select(2))
        );
        let x = (0..driver.area().width)
            .find(|&x| driver.cell(x, 2).symbol() == "A")
            .expect("amount header visible after resize");
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), x, 2), &state),
            EventResult::Emit(Msg::Sort(TableSort {
                column: 1,
                direction: SortDirection::Descending
            }))
        );
        assert_eq!(
            driver.event(key(KeyCode::Tab), &state),
            EventResult::Emit(Msg::Focus(FocusState::intent(["next"])))
        );
        frame(&mut driver, &state, 19, true);
        assert_eq!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::Button)
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), x, 2), &state),
            EventResult::Ignored
        );
    }

    /// The header is the place above the first row, the way a grid's column
    /// headers are: the one cursor climbs onto it, sorts from it, and comes
    /// back down. No letter key is taken from the app on the way.
    #[test]
    fn the_cursor_climbs_onto_the_header_to_sort_and_letters_stay_with_the_app() {
        let mut driver = focused_driver();
        let mut state = State {
            row: Some(2),
            ..State::default()
        };
        frame(&mut driver, &state, 25, false);
        assert_eq!(
            driver.event(key(KeyCode::Up), &state),
            EventResult::Emit(Msg::Move(0, 0)),
            "the disabled row is stepped over"
        );
        state.row = Some(0);
        assert_eq!(
            driver.event(key(KeyCode::Up), &state),
            EventResult::Emit(Msg::Header)
        );
        assert_eq!(
            driver.event(key(KeyCode::Char('s')), &state),
            EventResult::Ignored,
            "a single letter is the app's hotkey, as on a list"
        );
        state.on_header = true;
        frame(&mut driver, &state, 25, false);
        assert_eq!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Consumed,
            "column 0 is not sortable"
        );
        assert_eq!(
            driver.event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Column(1))
        );
        state.column = 1;
        assert_eq!(
            driver.event(key(KeyCode::Char(' ')), &state),
            EventResult::Emit(Msg::Sort(TableSort {
                column: 1,
                direction: SortDirection::Ascending
            }))
        );
        for up in [KeyCode::Up, KeyCode::Home, KeyCode::PageUp] {
            assert_eq!(driver.event(key(up), &state), EventResult::Consumed);
        }
        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Move(0, 0))
        );
        assert_eq!(
            driver.event(key(KeyCode::End), &state),
            EventResult::Emit(Msg::Move(6, 5))
        );
    }

    /// As on a list, the pointer drives the same cursor the keys do, and it
    /// stays where the pointer left it.
    #[test]
    fn pointer_motion_moves_the_one_row_cursor_and_leaves_it_there() {
        let mut driver = focused_driver();
        // Rows 2 and 3 are on screen, the cursor on 3.
        let state = State {
            row: Some(3),
            ..State::default()
        };
        frame(&mut driver, &state, 25, false);
        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 4, 3), &state),
            EventResult::Emit(Msg::Move(2, 2))
        );
        // Off the rows, on the footer, the cursor stays where it was put.
        assert!(
            !matches!(
                driver.event(mouse(MouseKind::Moved, 4, 7), &state),
                EventResult::Emit(_)
            ),
            "leaving the rows does not move the cursor"
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 22, 2), &state),
            EventResult::Consumed,
            "a heading under the pointer is a hover, not a cursor move"
        );
    }

    /// Pointer, active column and cursor each mark a heading differently, so
    /// a hovered heading never reads as the one the keys act on.
    #[test]
    fn hovered_active_and_cursor_headings_paint_apart() {
        let style = TableStyle::from_theme(&Theme::default_dark());
        let mut driver = focused_driver();
        let mut state = State {
            row: Some(0),
            ..State::default()
        };
        frame(&mut driver, &state, 25, false);
        driver.event(mouse(MouseKind::Moved, 22, 2), &state);
        frame(&mut driver, &state, 25, false);
        let active = driver.cell(2, 2);
        assert_eq!(active.fg, style.active_header_foreground);
        assert_eq!(active.bg, style.hovered_background);
        let hovered = driver.cell(22, 2);
        assert_eq!(hovered.fg, style.hovered_header_foreground);
        assert_eq!(hovered.bg, style.hovered_header_background);
        assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Pointer);
        // The cursor row is a band, and only the active cell is the cursor.
        assert_eq!(driver.cell(2, 3).bg, style.focused_row_background);
        assert_eq!(driver.cell(22, 3).bg, style.focused_band_background);

        state.on_header = true;
        frame(&mut driver, &state, 25, false);
        let cursor = driver.cell(2, 2);
        assert_eq!(
            (cursor.fg, cursor.bg),
            (style.focused_foreground, style.focused_row_background)
        );
        assert_eq!(
            driver.cell(2, 3).bg,
            style.hovered_background,
            "no row cursor"
        );
        let all = [
            style.hovered_background,
            style.hovered_header_background,
            style.focused_row_background,
        ];
        assert!(
            all.iter()
                .enumerate()
                .all(|(i, a)| all[i + 1..].iter().all(|b| a != b))
        );
    }

    /// Side by side, a table and a list paint the same well, rows, cursor and
    /// selection.
    #[test]
    fn a_table_beside_a_list_paints_the_same_rows() {
        #[derive(Default)]
        struct Pair {
            focus: FocusState,
        }
        let mut driver: Driver<Pair, FocusState> =
            Driver::with(Ratcn::new().focus(|s: &Pair| &s.focus, |f| f), 40, 4);
        let items = || (0u8..3).map(|i| ListItem::new(i, format!("item {i}")));
        let draw = |driver: &mut Driver<Pair, FocusState>, focus: &'static str| {
            let state = Pair {
                focus: FocusState::intent([focus]),
            };
            driver.render(&state, |ctx| {
                ctx.component(
                    "list",
                    List::new(items())
                        .item_focus(|_: &Pair| Some(1), |_, _| FocusState::default())
                        .selection(|_| Some(2), |_| FocusState::default()),
                    Rect::new(0, 0, 18, 3),
                );
                ctx.component(
                    "table",
                    Table::new(items(), [Constraint::Fill(1)])
                        .item_focus(|_: &Pair| Some(1), |_, _| FocusState::default())
                        .selection(|_| Some(2), |_| FocusState::default()),
                    Rect::new(20, 0, 18, 3),
                );
            });
        };
        let label = |driver: &Driver<Pair, FocusState>, from: u16, y: u16| {
            let x = (from..from + 18)
                .find(|&x| driver.cell(x, y).symbol() == "i")
                .expect("label painted");
            let cell = driver.cell(x + 1, y);
            (cell.fg, cell.bg)
        };
        draw(&mut driver, "list");
        let list: Vec<_> = (0..3).map(|y| label(&driver, 0, y)).collect();
        draw(&mut driver, "table");
        let table: Vec<_> = (0..3).map(|y| label(&driver, 20, y)).collect();
        assert_eq!(list, table);
        let style = ListStyle::from_theme(&Theme::default_dark());
        assert_eq!(list[1].1, style.focused_row_background);
    }

    #[test]
    fn a_disabled_table_dims_like_a_disabled_list_and_offers_no_click() {
        let style = TableStyle::from_theme(&Theme::default_dark());
        let mut driver = focused_driver();
        let state = State::default();
        frame(&mut driver, &state, 25, true);
        driver.event(mouse(MouseKind::Moved, 22, 2), &state);
        frame(&mut driver, &state, 25, true);
        for (x, y) in [(22, 2), (2, 3), (22, 5), (2, 7)] {
            let cell = driver.cell(x, y);
            assert_eq!(
                (cell.fg, cell.bg),
                (style.disabled_foreground, style.disabled_background)
            );
        }
        assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Default);
    }

    #[test]
    fn an_empty_table_still_sorts_from_its_header() {
        let mut driver = focused_driver();
        let mut state = State {
            column: 1,
            ..State::default()
        };
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                let empty = Table::new(
                    std::iter::empty::<ListItem<u8>>(),
                    [Constraint::Fill(1), Constraint::Length(6)],
                )
                .header(["ID", "Amount"])
                .item_focus(|s: &State| s.row, Msg::Move)
                .header_focus(|s| s.on_header, || Msg::Header)
                .column_focus(|s| Some(s.column), Msg::Column)
                .sort([1], |s| s.sort, Msg::Sort);
                ctx.component("table", empty, Rect::new(0, 0, 20, 4));
            });
        };
        render(&mut driver, &state);
        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Header),
            "entering an empty table with header_focus lands on the header"
        );
        state.on_header = true;
        render(&mut driver, &state);
        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Consumed,
            "vertical nav keys at a header boundary stay consumed, as on a list"
        );
        assert_eq!(
            driver.event(key(KeyCode::Char('x')), &state),
            EventResult::Ignored,
            "unrelated letters still bubble to the app"
        );
        assert_eq!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::Sort(TableSort {
                column: 1,
                direction: SortDirection::Ascending
            }))
        );
    }

    #[test]
    fn wheel_and_resize_hit_the_painted_record_and_reorder_cancels_a_press() {
        let mut driver = Driver::new(32, 12);
        let mut state = State::default();
        frame(&mut driver, &state, 25, false);
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 4, 3),
                &state
            ),
            EventResult::Consumed
        );
        frame(&mut driver, &state, 16, false);
        assert_eq!(driver.cell(2, 3).symbol(), "3");
        assert_eq!(driver.cell(2, 4).symbol(), "=");
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 2, 4), &state),
            EventResult::Emit(Msg::Select(3))
        );
        assert_eq!(driver.row(7).trim_matches([' ', '│']), "Total");
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 2, 7), &state),
            EventResult::Ignored
        );
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 2, 5), &state);
        state.sort = Some(TableSort {
            column: 1,
            direction: SortDirection::Ascending,
        });
        frame(&mut driver, &state, 16, false);
        assert_eq!(driver.cell(2, 5).symbol(), "2");
        assert_eq!(
            driver.event(mouse(MouseKind::Up(MouseButton::Left), 2, 5), &state),
            EventResult::Consumed
        );
    }

    #[test]
    fn a_sort_mark_sits_on_the_side_away_from_alignment() {
        let mut driver = focused_driver();
        let state = State {
            sort: Some(TableSort {
                column: 1,
                direction: SortDirection::Descending,
            }),
            ..State::default()
        };
        driver.render(&state, |ctx| {
            let table = Table::new(
                [ListItem::new(0u8, "a")],
                [Constraint::Length(8), Constraint::Length(10)],
            )
            .header([Line::from("Name"), Line::from("Amount").right_aligned()])
            .sort([0, 1], |s: &State| s.sort, Msg::Sort);
            ctx.component("table", table, Rect::new(0, 0, 19, 2));
        });
        assert_eq!(driver.row(0).trim_end(), "Name       ▼ Amount");
        let state = State {
            sort: Some(TableSort {
                column: 0,
                direction: SortDirection::Ascending,
            }),
            ..State::default()
        };
        driver.render(&state, |ctx| {
            let table = Table::new(
                [ListItem::new(0u8, "a")],
                [Constraint::Length(8), Constraint::Length(10)],
            )
            .header([Line::from("Name"), Line::from("Amount").right_aligned()])
            .sort([0, 1], |s: &State| s.sort, Msg::Sort);
            ctx.component("table", table, Rect::new(0, 0, 19, 2));
        });
        assert_eq!(driver.row(0).trim_end(), "Name ▲       Amount");
    }

    /// A press and its release must land on the same heading or row, and a
    /// click that would do nothing anyway bubbles, as on a list.
    #[test]
    fn a_click_acts_only_where_its_press_landed() {
        let mut driver = focused_driver();
        let state = State::default();
        frame(&mut driver, &state, 25, false);
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 22, 2), &state);
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 4, 3), &state),
            EventResult::Consumed,
            "pressed on a heading, released on a row"
        );
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 4, 3), &state);
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 22, 2), &state),
            EventResult::Consumed,
            "pressed on a row, released on a heading"
        );
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 4, 3), &state);
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 4, 7), &state),
            EventResult::Ignored,
            "released on the footer, where a click does nothing"
        );
    }

    /// A Tab stop needs a key that does something there. A header only a
    /// click can sort, or a column cursor without a row cursor, is not one.
    #[test]
    fn a_table_is_focusable_only_where_keys_act() {
        let headed = || {
            Table::<u8, State, Msg>::new([ListItem::new(0u8, "a")], [Constraint::Fill(1)])
                .header(["Name"])
                .sort([0], |s| s.sort, Msg::Sort)
        };
        let state = State::default();
        assert!(!headed().scope_options(&state).focusable);
        assert!(
            !headed()
                .column_focus(|s| Some(s.column), Msg::Column)
                .scope_options(&state)
                .focusable
        );
        let navigable = headed().item_focus(|s| s.row, Msg::Move);
        assert!(navigable.scope_options(&state).focusable);
    }

    /// Default state starts with no cursor — matching `List` — even when
    /// `header_focus` is bound; once on the header, the cursor is always on a
    /// heading.
    #[test]
    fn the_header_cursor_is_explicit_and_always_visible_on_the_header() {
        let style = TableStyle::from_theme(&Theme::default_dark());
        let mut driver = focused_driver();
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                let table = table(state, false).column_focus(|_| None, Msg::Column);
                ctx.component("table", table, Rect::new(1, 1, 25, 8));
            });
        };
        let mut state = State::default();
        render(&mut driver, &state);
        assert_ne!(
            driver.cell(2, 2).bg,
            style.focused_row_background,
            "default state starts with no cursor, not on the header"
        );
        assert_eq!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Move(0, 0)),
            "first navigation key lands on the first enabled row, as on a list"
        );

        state.on_header = true;
        render(&mut driver, &state);
        assert_eq!(driver.cell(2, 2).bg, style.focused_row_background);
        assert_eq!(
            driver.event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Column(1)),
            "Right steps from the column the cursor is painted on"
        );
        // A stale row value while not on the header is no cursor.
        state.on_header = false;
        state.row = Some(9);
        render(&mut driver, &state);
        assert_ne!(driver.cell(2, 2).bg, style.focused_row_background);
        assert_eq!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Ignored
        );
    }

    /// Explicit span foreground colors inside `paint_item` survive even on the
    /// active cursor cell when `column_focus` is bound, as on a `List`.
    #[test]
    fn cursor_cell_preserves_explicit_span_foreground() {
        let style = TableStyle::from_theme(&Theme::default_dark());
        let mut driver = focused_driver();
        let state = State {
            row: Some(0),
            column: 1,
            ..State::default()
        };
        driver.render(&state, |ctx| {
            let table = Table::new(
                [ListItem::new(0u8, "a")],
                [Constraint::Length(6), Constraint::Length(8)],
            )
            .paint_item(|_, _| {
                Row::new([
                    Line::from("item"),
                    Line::from(Span::styled("-$5.00", Style::new().fg(Color::Red))),
                ])
            })
            .item_focus(|s: &State| s.row, Msg::Move)
            .column_focus(|s| Some(s.column), Msg::Column);
            ctx.component("table", table, Rect::new(0, 0, 16, 2));
        });
        let cell = driver.cell(7, 0);
        assert_eq!(cell.symbol(), "-");
        assert_eq!(cell.fg, Color::Red);
        assert_eq!(cell.bg, style.focused_row_background);
    }

    /// `End` and `PageDown` from the header still descend into a single-row table.
    #[test]
    fn end_and_page_down_leave_the_header_in_a_single_row_table() {
        let mut driver = focused_driver();
        let state = State {
            on_header: true,
            ..State::default()
        };
        driver.render(&state, |ctx| {
            let table = Table::new([ListItem::new(7u8, "only")], [Constraint::Fill(1)])
                .header(["Name"])
                .item_focus(|s: &State| s.row, Msg::Move)
                .header_focus(|s| s.on_header, || Msg::Header)
                .column_focus(|s| Some(s.column), Msg::Column)
                .sort([0], |s| s.sort, Msg::Sort);
            ctx.component("table", table, Rect::new(0, 0, 16, 4));
        });
        for code in [KeyCode::Down, KeyCode::End, KeyCode::PageDown] {
            assert_eq!(
                driver.event(key(code), &state),
                EventResult::Emit(Msg::Move(7, 0)),
                "{code:?} from header should enter the only row"
            );
        }
    }

    /// A narrow right-aligned sorted heading drops the sort mark rather than
    /// clipping the label itself.
    #[test]
    fn right_aligned_sorted_heading_keeps_its_label_when_narrow() {
        let mut driver = focused_driver();
        let state = State {
            sort: Some(TableSort {
                column: 0,
                direction: SortDirection::Descending,
            }),
            ..State::default()
        };
        driver.render(&state, |ctx| {
            let table = Table::new([ListItem::new(0u8, "a")], [Constraint::Length(6)])
                .header([Line::from("Amount").right_aligned()])
                .sort([0], |s: &State| s.sort, Msg::Sort);
            ctx.component("table", table, Rect::new(0, 0, 6, 2));
        });
        assert_eq!(driver.row(0).trim_end(), "Amount");
    }

    /// Trailing columns without headings are neither hoverable nor sortable,
    /// and a zero-column table offers no pointer hand.
    #[test]
    fn short_headers_and_empty_widths_do_not_expose_dead_targets() {
        let mut driver = focused_driver();
        let state = State::default();
        driver.render(&state, |ctx| {
            let table = Table::new(
                [ListItem::new(0u8, "a")],
                [Constraint::Length(6), Constraint::Length(6)],
            )
            .header(["Only"])
            .sort([0, 1], |s: &State| s.sort, Msg::Sort);
            ctx.component("table", table, Rect::new(0, 0, 14, 2));
        });
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 8, 0), &state),
            EventResult::Ignored,
            "column 1 has no heading and must not sort"
        );
        driver.event(mouse(MouseKind::Moved, 8, 0), &state);
        driver.render(&state, |ctx| {
            let empty = Table::new([ListItem::new(0u8, "a")], std::iter::empty::<Constraint>())
                .selection(|s: &State| s.selected, Msg::Select);
            ctx.component("empty", empty, Rect::new(0, 0, 14, 2));
        });
        assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Default);
    }

    #[test]
    fn standalone_widget_preserves_native_geometry_and_explicit_cell_styles() {
        let area = Rect::new(0, 0, 15, 8);
        let rows = [
            Row::new(["A\nB", "C"]).height(2).bottom_margin(1),
            Row::new(["D", "E"]),
        ];
        let header = Row::new(["Name", "Value"])
            .style(Style::default().fg(Color::Yellow))
            .bottom_margin(1);
        let footer = Row::new(["Total"]);
        let widths = [7, 6];
        let style = TableStyle::from_theme(&Theme::default_dark());
        let mut got = Buffer::empty(area);
        let mut expected = got.clone();
        let state = TableState::default().with_selected(1);
        StatefulWidget::render(
            TableWidget::new(rows.clone(), widths)
                .header(header.clone())
                .footer(footer.clone())
                .style(style)
                .focused(true),
            area,
            &mut got,
            &mut state.clone(),
        );
        let raw = NativeTable::new(rows, widths)
            .style(
                Style::default()
                    .fg(style.foreground)
                    .bg(style.focused_background),
            )
            .header(header.style(Style::default().fg(Color::Yellow).bold()))
            .footer(footer.style(Style::default().fg(style.foreground)))
            .row_highlight_style(
                Style::default()
                    .fg(style.focused_foreground)
                    .bg(style.focused_row_background),
            );
        StatefulWidget::render(raw, area, &mut expected, &mut state.clone());
        assert_eq!(got, expected);
    }
}
