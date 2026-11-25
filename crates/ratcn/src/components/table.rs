use std::{cmp::Ordering, fmt, ops::Range, rc::Rc, sync::Arc};

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
    Component, DeclareCtx, Event, EventCtx, EventResult, KeyChord, KeyCode, KeyEvent, MouseButton,
    MouseKind, PaintCtx, PointerShape, ScopeOptions, ScrollDirection, Step,
};
use crate::text_width::display_width;
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
/// - **The selected cursor.** A table row has no selection marker, so the
///   selected row under the cursor (or its cursor cell) takes its own fill,
///   [`selected_focused_background`](Self::selected_focused_background).
/// - **The column cursor.** With a column cursor bound, the cursor row drops to
///   [`focused_band_background`](Self::focused_band_background) and only the
///   active cell keeps the cursor row's fill, so the cursor reads as a cell.
///   The active column's heading brightens to
///   [`active_header_foreground`](Self::active_header_foreground).
/// - **The header.** Headings are bold, muted like ordinary rows. A sortable
///   heading under the pointer takes its own hover fill,
///   [`hovered_header_background`](Self::hovered_header_background), on the
///   band's rung; when the cursor itself is on the header, its cell paints
///   like the cursor row.
/// - **The footer and the frozen divider.** Footer text takes
///   [`footer_foreground`](Self::footer_foreground); the `│` rule beside frozen
///   columns takes [`divider_foreground`](Self::divider_foreground), the
///   theme's border color, so it reads as a quiet rule rather than as text.
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
    /// Text of a selected row the cursor is not on, and of the rest of a
    /// selected cursor row while a column cursor shows. Selection off the
    /// cursor has no fill.
    pub selected_foreground: Color,
    /// Text of the cursor row when it is not selected.
    pub focused_foreground: Color,
    /// Fill behind the cursor row, or behind the cursor cell when a column
    /// cursor is shown.
    pub focused_row_background: Color,
    /// Text of the cursor row (or cursor cell) when it is also selected.
    pub selected_focused_foreground: Color,
    /// Fill behind the cursor row (or cell) when it is also selected. A table
    /// row has no selection marker, so this is what tells it apart from the
    /// plain cursor.
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
    /// Text of a footer cell.
    pub footer_foreground: Color,
    /// The `│` rule beside frozen columns once columns scroll behind them.
    pub divider_foreground: Color,
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
            // Without a marker, the selected cursor row needs its own fill.
            selected_focused_foreground: Color::White,
            selected_focused_background: Color::Blue,
            focused_band_background: Color::Gray,
            header_foreground: Color::Reset,
            active_header_foreground: Color::LightCyan,
            hovered_header_foreground: Color::Black,
            hovered_header_background: Color::Gray,
            footer_foreground: Color::Reset,
            divider_foreground: Color::DarkGray,
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
    ///
    /// A table row has no selection marker, so the selected row under the
    /// cursor — or its cursor cell, with a column cursor shown — is filled
    /// with `primary` and lettered in `primary_foreground`, as a selected tab
    /// or selected text is.
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
            selected_focused_foreground: theme.primary_foreground,
            selected_focused_background: theme.primary,
            focused_band_background: band,
            header_foreground: theme.muted_foreground,
            active_header_foreground: theme.foreground,
            hovered_header_foreground: theme.foreground,
            hovered_header_background: band,
            footer_foreground: theme.muted_foreground,
            divider_foreground: theme.border,
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
    /// highlight symbol/spacing, or individual highlight styles). Only the
    /// row highlight is themed; column and cell highlights are left to the
    /// caller.
    #[must_use]
    pub fn into_inner(self) -> NativeTable<'a> {
        let style = self.style;
        let surface = style.resolve_surface(self.focused, self.hovered, self.disabled);
        let (text, header, footer) = if self.disabled {
            let dimmed = style.disabled_foreground;
            (dimmed, dimmed, dimmed)
        } else {
            (
                style.foreground,
                style.header_foreground,
                style.footer_foreground,
            )
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
            table = table.footer(styled_row(row, Style::default().fg(footer)));
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortDirection {
    /// Smallest first.
    Ascending,
    /// Largest first.
    Descending,
}

impl SortDirection {
    /// Turn an ascending comparison into one in this direction: `ordering`
    /// as it is for [`Ascending`](Self::Ascending), reversed for
    /// [`Descending`](Self::Descending).
    ///
    /// ```
    /// use ratcn::SortDirection;
    ///
    /// let mut amounts = [3, 1, 2];
    /// let direction = SortDirection::Descending;
    /// amounts.sort_by(|a, b| direction.apply(a.cmp(b)));
    /// assert_eq!(amounts, [3, 2, 1]);
    /// ```
    #[must_use]
    pub const fn apply(self, ordering: Ordering) -> Ordering {
        match self {
            Self::Ascending => ordering,
            Self::Descending => ordering.reverse(),
        }
    }
}

/// Zero-based column and order. The app sorts the underlying records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TableSort {
    /// Index in the declared column widths.
    pub column: usize,
    /// Sort direction.
    pub direction: SortDirection,
}

/// Where a [`Table`]'s one cursor is: on the header, or on a row.
///
/// The header is the place above the first row. The cursor can go there only
/// while the table's header has a sortable heading; see [`Table`] for when
/// each value is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TableCursor<T> {
    /// On the header, on one heading.
    Header,
    /// On the row whose item carries this value.
    Row(T),
}

type Read<S, T> = Box<dyn Fn(&S) -> Option<T>>;
type Binding<S, T, M> = (Read<S, T>, Box<dyn Fn(T) -> M>);
type MultiBinding<S, T, M> = (Box<dyn Fn(&S, &T) -> bool>, Box<dyn Fn(T) -> M>);
type OffsetBinding<S, T, M> = (Read<S, T>, Box<dyn Fn(T, usize) -> M>);
type ScrollBinding<S, M> = (Box<dyn Fn(&S) -> usize>, Box<dyn Fn(usize) -> M>);
type PaintItem<S, T> = Box<dyn Fn(&S, ListItemState<'_, T>) -> Row<'static>>;
type StyleFn = Box<dyn Fn(&Theme) -> TableStyle>;

#[derive(Debug, PartialEq, Clone)]
enum Hit<T> {
    Row(T),
    Header(usize),
}

/// Emit `next` through a bound scroll offset unless the app already holds it.
/// An unbound offset has no message, so the event is only consumed.
fn emit_scroll<S, M>(
    binding: Option<&ScrollBinding<S, M>>,
    state: &S,
    next: usize,
) -> EventResult<M> {
    binding
        .filter(|(read, _)| read(state) != next)
        .map_or(EventResult::Consumed, |(_, on_change)| {
            EventResult::Emit(on_change(next))
        })
}

/// Render a native table, with `state` when one is given.
fn render_native(
    table: NativeTable<'_>,
    area: Rect,
    buf: &mut Buffer,
    state: Option<&mut TableState>,
) {
    match state {
        Some(state) => StatefulWidget::render(table, area, buf, state),
        None => Widget::render(table, area, buf),
    }
}

/// What one frame resolves once and every band paints with.
struct Look {
    style: TableStyle,
    /// The widths and flex the native table lays out: the declared ones when
    /// the columns fit, the canvas widths when they scroll.
    widths: Vec<Constraint>,
    flex: Flex,
    interactive: bool,
    hovered: bool,
    cursor_visible: bool,
    surface: Color,
    /// Text and fill beneath everything.
    base: Style,
    divider: Color,
}

/// Transient hold for horizontal wheel scrolling, mirroring [`WheelHold`].
///
/// Horizontal wheel notches scroll columns without moving the active column
/// cursor; the hold stands until the active column or column shape changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ColWheelHold {
    offset: usize,
    /// The columns as the wheel left them. `None` while nothing holds the view.
    held: Option<ColAnchor>,
}

/// The columns as the wheel left them: the active column, how many columns
/// there were, how many of them were frozen, and that they overflowed. A
/// layout that has since fitted has no wheeled view left to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ColAnchor {
    column: Option<usize>,
    columns: usize,
    frozen: usize,
    overflows: bool,
}

impl ColWheelHold {
    fn hold(&mut self, offset: usize, anchor: ColAnchor) {
        self.offset = offset;
        self.held = Some(anchor);
    }

    fn settle(
        &mut self,
        viewport: &mut ColViewport,
        active_col: Option<usize>,
        requested: Option<usize>,
    ) {
        if self.held != Some(viewport.anchor(active_col)) {
            self.held = None;
        }
        let cursor = if self.held.is_some() {
            None
        } else {
            active_col
        };
        let offset = viewport.cursor_visible_offset(requested.unwrap_or(self.offset), cursor);
        self.offset = offset;
        viewport.record_painted_offset(offset, requested);
    }
}

/// Render-derived horizontal column layout: handles both normal fitting layouts
/// and horizontally scrolled layouts with optional frozen leading columns.
#[derive(Debug, Default)]
struct ColViewport {
    /// How many columns were declared.
    count: usize,
    /// Concrete widths on the virtual canvas when overflowing; empty when all
    /// columns fit inside the viewport.
    col_widths: Vec<u16>,
    /// Virtual X origin of each column on the canvas when overflowing.
    virtual_x: Vec<u16>,
    /// Screen-space header rects for hit-testing (empty `Rect` for off-screen
    /// columns).
    columns: Rc<[Rect]>,
    canvas_width: u16,
    frozen: usize,
    frozen_span: u16,
    scroll_view_w: u16,
    spacing: u16,
    max_offset: usize,
    painted_offset: usize,
    pending_offset: Option<usize>,
    declared_requested: Option<usize>,
}

impl ColViewport {
    fn overflows(&self) -> bool {
        !self.col_widths.is_empty()
    }

    fn anchor(&self, column: Option<usize>) -> ColAnchor {
        ColAnchor {
            column,
            columns: self.count,
            frozen: self.frozen,
            overflows: self.overflows(),
        }
    }

    /// The painted column under screen column `x`, if any.
    fn column_at(&self, x: u16) -> Option<usize> {
        self.columns
            .iter()
            .position(|r| (r.x..r.right()).contains(&x))
    }

    /// Whether column `index` is painted at its full width.
    fn fully_shown(&self, index: usize) -> bool {
        self.columns
            .get(index)
            .is_some_and(|r| usize::from(r.width) == self.column_width(index))
    }

    fn render_layout(&self, widths: &[Constraint], flex: Flex) -> (Vec<Constraint>, Flex) {
        if self.overflows() {
            (
                self.col_widths
                    .iter()
                    .map(|&w| Constraint::Length(w))
                    .collect(),
                Flex::Start,
            )
        } else {
            (widths.to_vec(), flex)
        }
    }

    fn column_width(&self, index: usize) -> usize {
        if self.overflows() {
            self.col_widths.get(index).copied().map_or(0, usize::from)
        } else {
            self.columns.get(index).map_or(0, |r| usize::from(r.width))
        }
    }

    fn record_painted_offset(&mut self, offset: usize, requested: Option<usize>) {
        self.painted_offset = offset;
        self.pending_offset = None;
        self.declared_requested = requested;
    }

    /// The offset the view is at, counting wheel notches not yet painted.
    fn current_offset(&self) -> usize {
        self.pending_offset.unwrap_or(self.painted_offset)
    }

    /// The offset that shows `column`, moving the view as little as possible:
    /// builds on a bound `requested` offset when the app has updated it since
    /// declaration, otherwise on [`current_offset`](Self::current_offset).
    fn offset_showing(&self, requested: Option<usize>, column: usize) -> usize {
        let base = match (requested, self.declared_requested) {
            (Some(live), Some(declared)) if live != declared => live,
            _ => self.current_offset(),
        };
        self.cursor_visible_offset(base, Some(column))
    }

    /// The rule sits in the gap after the frozen columns, so it needs a gap,
    /// frozen columns on screen, and columns scrolled behind them.
    fn show_frozen_divider(&self) -> bool {
        self.spacing > 0 && self.frozen_span > 0 && self.painted_offset > 0
    }

    /// Lay the columns out across `head`'s width: exactly as Ratatui lays out
    /// `widths` and `flex` when there is no `canvas`, otherwise at the
    /// `canvas` widths on a virtual canvas that scrolls.
    fn lay_out(
        &mut self,
        widths: &[Constraint],
        canvas: Option<Vec<u16>>,
        flex: Flex,
        spacing: u16,
        frozen: usize,
        head: Rect,
    ) {
        self.count = widths.len();
        self.spacing = spacing;
        self.frozen = frozen.min(widths.len());
        let Some(canvas) = canvas else {
            self.col_widths.clear();
            self.virtual_x.clear();
            self.canvas_width = head.width;
            self.frozen_span = 0;
            self.scroll_view_w = head.width;
            self.max_offset = 0;
            self.columns = Layout::horizontal(widths.to_vec())
                .flex(flex)
                .spacing(spacing)
                .split(head);
            return;
        };

        self.col_widths = canvas;
        self.virtual_x.clear();
        let mut x: u16 = 0;
        for (i, &w) in self.col_widths.iter().enumerate() {
            self.virtual_x.push(x);
            x = x.saturating_add(w);
            if i + 1 < self.col_widths.len() {
                x = x.saturating_add(spacing);
            }
        }
        self.canvas_width = x;

        let scrollable = widths.len() - self.frozen;
        self.frozen_span = if self.frozen == 0 {
            0
        } else if scrollable > 0 {
            self.virtual_x[self.frozen].min(head.width)
        } else {
            self.canvas_width.min(head.width)
        };
        self.scroll_view_w = head.width - self.frozen_span;
        self.max_offset = self.compute_max_offset(scrollable);
    }

    /// Place each painted column's screen rect in `head` for hit-testing.
    /// Columns that fit were placed by [`lay_out`](Self::lay_out); scrolling
    /// ones depend on the settled offset, so they are placed after it.
    fn place_columns(&mut self, head: Rect) {
        if !self.overflows() {
            return;
        }
        let mut rects = vec![Rect::default(); self.count];
        for (i, rect) in rects.iter_mut().enumerate().take(self.frozen) {
            let vx = self.virtual_x[i];
            if vx < self.frozen_span {
                let w = self.col_widths[i].min(self.frozen_span - vx);
                *rect = Rect::new(head.x.saturating_add(vx), head.y, w, head.height);
            }
        }
        if self.frozen < self.count && self.scroll_view_w > 0 {
            let first = self.frozen + self.painted_offset;
            let origin = self.virtual_x[first];
            let scroll_x = head.x.saturating_add(self.frozen_span);
            for (i, rect) in rects.iter_mut().enumerate().skip(first) {
                let rel = self.virtual_x[i].saturating_sub(origin);
                if rel >= self.scroll_view_w {
                    break;
                }
                let w = self.col_widths[i].min(self.scroll_view_w - rel);
                *rect = Rect::new(scroll_x.saturating_add(rel), head.y, w, head.height);
            }
        }
        self.columns = Rc::from(rects);
    }

    fn compute_max_offset(&self, scrollable: usize) -> usize {
        if scrollable == 0 || self.scroll_view_w == 0 {
            return 0;
        }
        for k in 0..scrollable {
            let span = self
                .canvas_width
                .saturating_sub(self.virtual_x[self.frozen + k]);
            if span <= self.scroll_view_w {
                return k;
            }
        }
        scrollable - 1
    }

    fn cursor_visible_offset(&self, requested: usize, active_col: Option<usize>) -> usize {
        let mut offset = requested.min(self.max_offset);
        let Some(col) = active_col else {
            return offset;
        };
        if !self.overflows() || col < self.frozen || self.scroll_view_w == 0 {
            return offset;
        }
        let rel = col - self.frozen;
        if rel < offset {
            return rel;
        }
        let col_end = self.virtual_x[col].saturating_add(self.col_widths[col]);
        while offset < rel {
            let right_edge =
                self.virtual_x[self.frozen + offset].saturating_add(self.scroll_view_w);
            if col_end <= right_edge {
                break;
            }
            offset += 1;
        }
        offset.min(self.max_offset)
    }

    fn wheel(
        &mut self,
        direction: ScrollDirection,
        active_col: Option<usize>,
        ctx: &mut EventCtx<'_>,
    ) -> Option<usize> {
        if !self.overflows() || self.max_offset == 0 {
            return None;
        }
        let current = self.current_offset();
        let next = match direction {
            ScrollDirection::Left => current.saturating_sub(1),
            ScrollDirection::Right => current.saturating_add(1).min(self.max_offset),
            ScrollDirection::Up | ScrollDirection::Down => return None,
        };
        if next == current {
            return None;
        }
        ctx.transient::<ColWheelHold>()
            .hold(next, self.anchor(active_col));
        self.pending_offset = Some(next);
        Some(next)
    }

    fn composite(
        &self,
        scratch: &Buffer,
        target: Rect,
        clip: Rect,
        dest: &mut Buffer,
        divider_fg: Color,
    ) {
        let height = target.height.min(scratch.area.height);
        // Frozen leading columns.
        let frozen_w = self.frozen_span.min(target.width).min(scratch.area.width);
        blit_slice(scratch, dest, clip, 0, target.x, target.y, frozen_w, height);
        // Scrolled columns.
        if self.frozen < self.virtual_x.len() && self.scroll_view_w > 0 {
            let src_x = self.virtual_x[self.frozen + self.painted_offset];
            let dst_x = target.x.saturating_add(frozen_w);
            let avail_dst = target.width.saturating_sub(frozen_w);
            let avail_src = scratch.area.width.saturating_sub(src_x);
            let copy_w = self.scroll_view_w.min(avail_dst).min(avail_src);
            blit_slice(scratch, dest, clip, src_x, dst_x, target.y, copy_w, height);
        }
        // Subtle separator inside the trailing frozen gap once scrolled right.
        if self.show_frozen_divider() {
            let x = target.x.saturating_add(self.frozen_span - 1);
            for dy in 0..height {
                let y = target.y.saturating_add(dy);
                if clip.contains(Position::new(x, y))
                    && let Some(cell) = dest.cell_mut((x, y))
                {
                    cell.set_symbol("│").set_fg(divider_fg);
                }
            }
        }
    }
}

/// The width a column takes on a scrolling canvas, with `share` cells to
/// divide by percentage or ratio. A `Fill` column has no width of its own
/// there, so it takes `fill_min`.
fn natural_col_width(constraint: Constraint, share: u16, fill_min: u16) -> u16 {
    match constraint {
        Constraint::Length(n) | Constraint::Min(n) | Constraint::Max(n) => n,
        Constraint::Percentage(p) => {
            u16::try_from(u32::from(share) * u32::from(p.min(100)) / 100).unwrap_or(share)
        }
        Constraint::Ratio(num, den) => {
            if den == 0 {
                0
            } else {
                u16::try_from(
                    (u64::from(share) * u64::from(num) / u64::from(den)).min(u64::from(share)),
                )
                .unwrap_or(share)
            }
        }
        Constraint::Fill(_) => fill_min,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "coordinates for blitting one clipped horizontal band"
)]
fn blit_slice(
    src: &Buffer,
    dst: &mut Buffer,
    clip: Rect,
    src_x: u16,
    dst_x: u16,
    dst_y: u16,
    width: u16,
    height: u16,
) {
    if width == 0 || height == 0 {
        return;
    }
    for dy in 0..height {
        let y = dst_y.saturating_add(dy);
        for dx in 0..width {
            let x = dst_x.saturating_add(dx);
            if !clip.contains(Position::new(x, y)) {
                continue;
            }
            let Some(from) = src.cell((src_x.saturating_add(dx), dy)) else {
                continue;
            };
            let Some(to) = dst.cell_mut((x, y)) else {
                continue;
            };
            *to = from.clone();
            // Prevent a wide (2-cell) glyph from bleeding past the clipped edge.
            if dx + 1 == width && display_width(to.symbol()) > 1 {
                to.set_symbol(" ");
            }
        }
    }
}

/// A scrollable table of value-keyed rows, declared with
/// [`component`](crate::runtime::DeclareCtx::component).
///
/// A table is a [`List`](crate::List) with columns. Rows are [`ListItem`]s in
/// display order, each identified by a value you choose, and the app owns the
/// cursor, the selection, the column cursor, the sort order, and — if it wants
/// them — both scroll offsets. Supply one width per column. The default row
/// shows the item's label in the first column;
/// [`paint_item`](Self::paint_item) fills every column, and is called only for
/// the rows on screen.
///
/// # Cursor and selection are different things
///
/// The **cursor** is where the user is looking: arrow keys, Home, End, the
/// page keys and the pointer move it, and moving it commits nothing.
/// **Selection** is the choice: Enter, Space, or a left click commits a row.
/// The cursor is a [`TableCursor`] — on a row, or on the header above the
/// first row.
///
/// # Bindings
///
/// Each binding is a pair — a reader for the current value and a constructor
/// for the message that changes it — passed in one call so the two cannot
/// drift apart. Rows are keyed by item value, never by position, so sorting or
/// filtering keeps the same record current. Columns are keyed by index into
/// the declared widths: if you hide or reorder columns, remap the stored
/// column, the sort column and the sortable indices to match.
///
/// Item values must be unique within each declaration, or the cursor,
/// selection and pointer actions are ambiguous. A debug build panics on
/// duplicates during declaration.
///
/// - [`item_focus`](Self::item_focus) — the cursor and the top-row offset. Its
///   message carries the new [`TableCursor`] and the offset that keeps it in
///   view; store both in one update when [`scroll`](Self::scroll) is bound.
/// - [`selection`](Self::selection) or
///   [`multi_selection`](Self::multi_selection) — single choice or any number
///   of toggled rows (mutually exclusive). Store or toggle the value, and set
///   the cursor to `TableCursor::Row` of the same value in that update, so keys
///   continue from a clicked row.
/// - [`column_focus`](Self::column_focus) — the column cursor and the
///   horizontal column offset; store both in one update when
///   [`horizontal_scroll`](Self::horizontal_scroll) is bound.
/// - [`sort`](Self::sort) — the requested [`TableSort`]. Reorder the records
///   and store the request; move the column cursor to the sorted column too if
///   you like.
/// - [`scroll`](Self::scroll) and
///   [`horizontal_scroll`](Self::horizontal_scroll) — the vertical and
///   horizontal offsets, for when something outside the table has to know or
///   drive them. Unbound, the table scrolls itself.
///
/// Disabled items are dimmed and skipped by both keyboard and mouse.
///
/// # The header
///
/// With `item_focus` bound and at least one sortable heading, the header is a
/// place the cursor can go: Up from the first enabled row — or any vertical
/// step in a table with no enabled row — emits [`TableCursor::Header`]. On the
/// header the cursor is on one heading: the column cursor when `column_focus`
/// is bound, otherwise the sorted column, otherwise the first sortable
/// heading. Left and Right move it among the columns that have headings, and
/// only when `column_focus` is bound. Enter or Space sorts its column, and
/// Down and the other forward keys come back into the rows. Without a sortable
/// heading the header is not a place, and Up at the top stays put, as on a
/// list. Clicking a sortable heading sorts by it either way, and a key set
/// with [`sort_key`](Self::sort_key) sorts the cursor's column from the rows
/// too.
///
/// # Pointer
///
/// Pointer motion over a row moves the row cursor there. With
/// `column_focus` bound, motion over another wholly visible cell of the cursor
/// row moves the column cursor to it without scrolling. A click commits the
/// row through `selection`, or sorts a sortable heading. Hovering a heading
/// only highlights it.
///
/// # Focus
///
/// A table is a Tab stop when it is enabled, has at least one column, and a
/// key can act there: `column_focus` is bound, the header is a place, a
/// `sort_key` is set and a heading sorts, or `item_focus` is bound and some
/// row is enabled. Tab and Shift+Tab stay ordinary component traversal.
///
/// # Wide tables
///
/// Columns that fit are laid out exactly as Ratatui lays them out. They
/// scroll horizontally, keeping the active column in view, only when their
/// rigid widths and the spacing exceed the inner width: `Length` and `Min`
/// widths, and for a `Fill` column the wider of its heading (plus room for a
/// sort mark when it sorts) and its footer cell, at least 1. `Percentage` and
/// `Ratio` columns take their share of what is left after spacing, and `Max`
/// columns can shrink to nothing. Once scrolling, every column takes that
/// width — a `Max` column its maximum, a `Percentage` or `Ratio` column its
/// share — and [`flex`](Self::flex) is ignored, so prefer `Length` or `Min`
/// widths for a table that can scroll. See
/// [`frozen_columns`](Self::frozen_columns) to pin leading columns.
///
/// ```
/// use ratatui::{layout::Constraint, widgets::Row};
/// use ratcn::{ListItem, Table, TableCursor, TableSort};
///
/// # #[derive(Default)]
/// # struct AppState {
/// #     cursor: Option<TableCursor<u64>>,
/// #     selected: Option<u64>,
/// #     column: usize,
/// #     sort: Option<TableSort>,
/// # }
/// # enum Msg { Cursor(TableCursor<u64>), Select(u64), Column(usize), Sort(TableSort) }
/// let _table = Table::new(
///     [ListItem::new(1u64, "Groceries"), ListItem::new(2, "Rent")],
///     [Constraint::Fill(1), Constraint::Length(10)],
/// )
/// .header(["Description", "Amount"])
/// .paint_item(|_: &AppState, row| Row::new([row.label.to_owned(), "$42.00".to_owned()]))
/// .item_focus(|s| s.cursor, |cursor, _| Msg::Cursor(cursor))
/// .selection(|s| s.selected, Msg::Select)
/// .column_focus(|s| Some(s.column), |column, _| Msg::Column(column))
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
    frozen_columns: usize,
    focused: Option<OffsetBinding<S, TableCursor<T>, M>>,
    selected: Option<Binding<S, T, M>>,
    selected_many: Option<MultiBinding<S, T, M>>,
    column: Option<OffsetBinding<S, usize, M>>,
    sort: Option<Binding<S, TableSort, M>>,
    sortable: Vec<usize>,
    sort_key: Option<KeyChord>,
    scroll: Option<ScrollBinding<S, M>>,
    horizontal_scroll: Option<ScrollBinding<S, M>>,
    paint_item: Option<PaintItem<S, T>>,
    style: Option<StyleFn>,
    disabled: bool,
    viewport: RowViewport,
    col_viewport: ColViewport,
    body: Rect,
    head: Rect,
    foot: Rect,
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
            frozen_columns: 0,
            focused: None,
            selected: None,
            selected_many: None,
            column: None,
            sort: None,
            sortable: Vec::new(),
            sort_key: None,
            scroll: None,
            horizontal_scroll: None,
            paint_item: None,
            style: None,
            disabled: false,
            viewport: RowViewport::new(1),
            col_viewport: ColViewport::default(),
            body: Rect::default(),
            head: Rect::default(),
            foot: Rect::default(),
        }
    }

    /// Fixed one-line header, one heading per column. A heading keeps its own
    /// spans, style and alignment; a sorted one gains a `▲`/`▼` mark on the
    /// side away from its alignment, when the column has room for it.
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
    /// Position columns using Ratatui's layout policy. Ignored while the
    /// columns overflow and scroll horizontally.
    #[must_use]
    pub const fn flex(mut self, flex: Flex) -> Self {
        self.flex = flex;
        self
    }
    /// Uniform body-row height for paging and hit-testing. Row-local heights
    /// and margins are replaced; use `TableWidget` for variable-height rows.
    /// A height of 0 is treated as 1.
    #[must_use]
    pub const fn row_height(mut self, height: u16) -> Self {
        self.viewport = RowViewport::new(height);
        self
    }
    /// Format a visible item with native cells, spans, alignment and styling.
    ///
    /// The closure gets app state and a [`ListItemState`] describing the row,
    /// and returns one cell per column; cells past the declared widths are
    /// dropped. The resolved row style is painted beneath the returned row, so
    /// unstyled text inherits the row's state colors while an explicit
    /// foreground or modifier on the `Row`, a `Cell` or a `Span` wins — except
    /// on the selected cursor row (or its cursor cell, with a column cursor),
    /// which is lettered in
    /// [`selected_focused_foreground`](TableStyle::selected_focused_foreground)
    /// so it stays legible on the primary fill. A background set there
    /// replaces the row's fill — cursor row and band included — so check
    /// `!row.focused` when adding custom row backgrounds such as zebra
    /// striping. Every row is forced to [`row_height`](Self::row_height) with
    /// zero margins.
    #[must_use]
    pub fn paint_item(
        mut self,
        paint: impl Fn(&S, ListItemState<'_, T>) -> Row<'static> + 'static,
    ) -> Self {
        self.paint_item = Some(Box::new(paint));
        self
    }
    /// Bind the cursor: where to read it from, and what message moves it.
    ///
    /// `read` returns where the cursor is, or `None` when it is nowhere yet —
    /// the first vertical navigation key then moves it onto the first enabled
    /// row. A `TableCursor::Row` whose value no row carries is treated the same
    /// way, as is `TableCursor::Header` while the header cannot take the
    /// cursor. `on_change` is called with the new cursor and the resulting
    /// top-row offset. Store both in one update when [`scroll`](Self::scroll)
    /// is bound; an unbound table can ignore the offset.
    ///
    /// The cursor reaches [`TableCursor::Header`] only while the header has a
    /// sortable heading; see [the header](Self#the-header). Moving the cursor
    /// commits nothing; see [`selection`](Self::selection). Without this
    /// binding rows remain available for painting, bound scrolling and pointer
    /// selection, and take no vertical keys; [`column_focus`](Self::column_focus)
    /// and [`sort_key`](Self::sort_key) still take theirs.
    #[must_use]
    pub fn item_focus(
        mut self,
        read: impl Fn(&S) -> Option<TableCursor<T>> + 'static,
        on_change: impl Fn(TableCursor<T>, usize) -> M + 'static,
    ) -> Self {
        self.focused = Some((Box::new(read), Box::new(on_change)));
        self
    }
    /// Bind single selection: at most one row chosen at a time.
    ///
    /// `read` returns the selected value, or `None` for nothing selected.
    /// `on_select` is called with the row the user committed — Enter or Space
    /// on the cursor row, or a left click on a row. A click need not be
    /// preceded by pointer motion, so when [`item_focus`](Self::item_focus) is
    /// bound, the update handling this message should also set the cursor to
    /// `TableCursor::Row` of the same value.
    ///
    /// # Panics
    ///
    /// Rendering panics if this is combined with
    /// [`multi_selection`](Self::multi_selection): a table is one mode or the
    /// other, and Enter commits through whichever one is bound.
    #[must_use]
    pub fn selection(
        mut self,
        read: impl Fn(&S) -> Option<T> + 'static,
        on_select: impl Fn(T) -> M + 'static,
    ) -> Self {
        self.selected = Some((Box::new(read), Box::new(on_select)));
        self
    }
    /// Bind multi-selection: any number of rows chosen at once.
    ///
    /// `read` is a predicate asked "is this row selected?" for each visible
    /// row, so your app can store the selection in a `HashSet`, a flag on each
    /// record, or a computed rule without converting it every frame.
    /// `on_toggle` is called with the row the user flipped via Enter, Space,
    /// or a left click; your update function decides whether to add or remove
    /// it. When [`item_focus`](Self::item_focus) is also bound, the update
    /// handling a click should set the cursor to `TableCursor::Row` of the
    /// same value.
    ///
    /// # Panics
    ///
    /// Rendering panics if this is combined with [`selection`](Self::selection).
    #[must_use]
    pub fn multi_selection(
        mut self,
        read: impl Fn(&S, &T) -> bool + 'static,
        on_toggle: impl Fn(T) -> M + 'static,
    ) -> Self {
        self.selected_many = Some((Box::new(read), Box::new(on_toggle)));
        self
    }
    /// Bind the column cursor for Left/Right (or h/l) and cell highlighting.
    ///
    /// `read` returns the active column's index into the declared widths, or
    /// `None` for none yet. `on_change` is called with the new column and the
    /// resulting horizontal column offset — the number of non-frozen columns
    /// scrolled off to the left once the view has moved as little as it must
    /// to show that column. Store both in one update when
    /// [`horizontal_scroll`](Self::horizontal_scroll) is bound; an unbound
    /// table can ignore the offset. Adjustments the table makes while
    /// rendering, such as keeping the column in view after a sort handler
    /// moved it or after a resize, cannot emit a message.
    ///
    /// Binding this makes the table a Tab stop even without
    /// [`item_focus`](Self::item_focus). Pointer motion over another wholly
    /// visible cell of the cursor row moves the column there, as described
    /// under [pointer](Self#pointer); headings do not. To move the column when
    /// a heading is clicked, do it in [`sort`](Self::sort)'s update.
    #[must_use]
    pub fn column_focus(
        mut self,
        read: impl Fn(&S) -> Option<usize> + 'static,
        on_change: impl Fn(usize, usize) -> M + 'static,
    ) -> Self {
        self.column = Some((Box::new(read), Box::new(on_change)));
        self
    }
    /// Enable sorting on the specified zero-based header columns. Clicking a
    /// sortable heading, or Enter or Space on it while the cursor is on the
    /// header, requests ascending order, or descending when that column is
    /// already sorted ascending.
    ///
    /// `read` returns the current sort, if any. `on_change` is called with the
    /// requested [`TableSort`]: reorder the records, store the request, and
    /// move the column cursor to its column too if you like. Only columns
    /// with a heading sort.
    #[must_use]
    pub fn sort(
        mut self,
        columns: impl IntoIterator<Item = usize>,
        read: impl Fn(&S) -> Option<TableSort> + 'static,
        on_change: impl Fn(TableSort) -> M + 'static,
    ) -> Self {
        self.sortable = columns.into_iter().collect();
        self.sort = Some((Box::new(read), Box::new(on_change)));
        self
    }
    /// A key that sorts the cursor's column from anywhere in the table, rows
    /// or header, without moving the row cursor. None by default.
    ///
    /// The column is the column cursor when [`column_focus`](Self::column_focus)
    /// reads one, otherwise the sorted column, otherwise the first sortable
    /// heading; the request toggles exactly as a click does. When that column
    /// does not sort, or [`sort`](Self::sort) is unbound, the key bubbles to
    /// the app. It takes precedence over the table's own keys: pick one that
    /// does not collide with `h`/`j`/`k`/`l` if you want those.
    ///
    /// Accepts anything that converts into a [`KeyChord`], so a bare `char`
    /// or [`KeyCode`] works, with [`ctrl`](KeyChord::ctrl) /
    /// [`alt`](KeyChord::alt) for combinations. Like every `KeyChord`,
    /// matching ignores Shift. For example:
    ///
    /// ```
    /// use ratatui::layout::Constraint;
    /// use ratcn::{ListItem, Table, TableSort, runtime::KeyChord};
    ///
    /// # struct AppState { sort: Option<TableSort> }
    /// # enum Msg { Sort(TableSort) }
    /// let _table: Table<u64, AppState, Msg> =
    ///     Table::new([ListItem::new(1u64, "Rent")], [Constraint::Fill(1)])
    ///         .header(["Description"])
    ///         .sort([0], |s: &AppState| s.sort, Msg::Sort)
    ///         .sort_key(KeyChord::from('s').ctrl());
    /// ```
    #[must_use]
    pub fn sort_key(mut self, key: impl Into<KeyChord>) -> Self {
        self.sort_key = Some(key.into());
        self
    }
    /// Pin the first `count` columns on the left while remaining columns scroll
    /// horizontally. Defaults to 0.
    ///
    /// Horizontal scrolling activates automatically whenever the columns
    /// cannot fit the table's inner width (see
    /// [wide tables](Self#wide-tables)); when everything fits,
    /// `frozen_columns` has no visual effect. Frozen columns should fit the
    /// view with room to spare: freezing every column, or columns wider than
    /// the view, leaves nothing to scroll. Once columns scroll behind the
    /// frozen ones, a thin `│` rule appears in the spacing gap beside them —
    /// only when [`column_spacing`](Self::column_spacing) is above 0, since
    /// without a gap there is nowhere to draw it.
    #[must_use]
    pub const fn frozen_columns(mut self, count: usize) -> Self {
        self.frozen_columns = count;
        self
    }
    /// Bind the requested vertical scroll offset — the index of the topmost
    /// visible row.
    ///
    /// Before paint, the table adjusts the requested value to keep the cursor
    /// row visible. Cursor movement computes the resulting offset from the
    /// view on screen, counting wheel notches not yet painted, and passes it
    /// to [`item_focus`](Self::item_focus)'s `on_change`, so its update can
    /// store cursor and offset together. The vertical wheel scrolls
    /// the view without moving the cursor, emits the offset one notch away,
    /// and bubbles at either end. Render-driven adjustment cannot emit a
    /// message.
    ///
    /// Left unbound, the table owns the offset itself and keys and the wheel
    /// still scroll it.
    #[must_use]
    pub fn scroll(
        mut self,
        read: impl Fn(&S) -> usize + 'static,
        on_change: impl Fn(usize) -> M + 'static,
    ) -> Self {
        self.scroll = Some((Box::new(read), Box::new(on_change)));
        self
    }
    /// Bind the requested horizontal column offset — the number of non-frozen
    /// columns scrolled off to the left.
    ///
    /// Before paint, the table adjusts the requested value to keep the active
    /// column visible. Column movement computes the same resulting offset and
    /// passes it to [`column_focus`](Self::column_focus)'s `on_change`, so its
    /// update can store column and offset together. The horizontal wheel
    /// scrolls the view without moving the column, emits the offset one notch
    /// away, and bubbles at either end. Render-driven adjustment, such as
    /// after a resize or a sort handler moving the column, cannot emit a
    /// message.
    ///
    /// Left unbound, the table owns the offset itself and column movement and
    /// the wheel still scroll it.
    #[must_use]
    pub fn horizontal_scroll(
        mut self,
        read: impl Fn(&S) -> usize + 'static,
        on_change: impl Fn(usize) -> M + 'static,
    ) -> Self {
        self.horizontal_scroll = Some((Box::new(read), Box::new(on_change)));
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

    /// Whether `column` has a heading a click or Enter can sort by.
    fn sorts_by(&self, column: usize) -> bool {
        column < self.header_columns() && self.sortable.contains(&column)
    }

    /// Whether some heading sorts.
    fn has_sortable_heading(&self) -> bool {
        (0..self.header_columns()).any(|i| self.sorts_by(i))
    }

    /// Whether the header is somewhere the cursor can go.
    fn header_is_a_place(&self) -> bool {
        self.focused.is_some() && self.has_sortable_heading()
    }

    /// The least a `Fill` column needs: the wider of its heading — with room
    /// for a sort mark when it sorts — and its footer cell, at least 1.
    fn fill_min(&self, column: usize) -> u16 {
        let cell = |cells: &Option<Vec<Line<'static>>>| {
            cells
                .as_ref()
                .and_then(|c| c.get(column))
                .map_or(0, Line::width)
        };
        let mark = if self.sorts_by(column) { 2 } else { 0 };
        let min = (cell(&self.header) + mark).max(cell(&self.footer)).max(1);
        u16::try_from(min).unwrap_or(u16::MAX)
    }

    /// Each column's width on a scrolling canvas, or `None` when the columns
    /// fit `width` and Ratatui lays them out as usual.
    ///
    /// Only the rigid parts can overflow: `Length` and `Min` widths, and each
    /// `Fill` column's [`fill_min`](Self::fill_min), plus the gaps. A
    /// `Percentage` or `Ratio` column takes its share of whatever is there,
    /// and a `Max` column can shrink to nothing.
    fn canvas_widths(&self, width: u16) -> Option<Vec<u16>> {
        let gaps = self
            .widths
            .len()
            .saturating_sub(1)
            .saturating_mul(usize::from(self.spacing));
        let rigid = self
            .widths
            .iter()
            .enumerate()
            .map(|(i, &constraint)| match constraint {
                Constraint::Length(n) | Constraint::Min(n) => usize::from(n),
                Constraint::Fill(_) => usize::from(self.fill_min(i)),
                Constraint::Max(_) | Constraint::Percentage(_) | Constraint::Ratio(..) => 0,
            })
            .sum::<usize>()
            .saturating_add(gaps);
        if rigid <= usize::from(width) {
            return None;
        }
        let share = u16::try_from(usize::from(width).saturating_sub(gaps)).unwrap_or(width);
        Some(
            self.widths
                .iter()
                .enumerate()
                .map(|(i, &constraint)| natural_col_width(constraint, share, self.fill_min(i)))
                .collect(),
        )
    }

    /// The rows on screen, by index.
    fn visible_rows(&self) -> Range<usize> {
        let start = self.viewport.painted_offset().min(self.items.len());
        let end = start
            .saturating_add(self.viewport.visible_items(self.body))
            .min(self.items.len());
        start..end
    }
}

impl<T: Clone + PartialEq + 'static, S, M> Table<T, S, M> {
    /// Whether the cursor is on the header. Never while the header is not a
    /// place: a `Header` cursor there is no cursor.
    fn on_header(&self, state: &S) -> bool {
        self.header_is_a_place()
            && self
                .focused
                .as_ref()
                .is_some_and(|(read, _)| read(state) == Some(TableCursor::Header))
    }
    /// The row the cursor is on; none on the header, or for a value no row
    /// carries.
    fn row_cursor(&self, state: &S) -> Option<usize> {
        match (self.focused.as_ref()?.0)(state)? {
            TableCursor::Row(value) => list_core::index_of(&self.items, &value),
            TableCursor::Header => None,
        }
    }
    fn active_column(&self, state: &S) -> Option<usize> {
        (self.column.as_ref()?.0)(state).filter(|&i| i < self.widths.len())
    }
    fn sorted(&self, state: &S) -> Option<TableSort> {
        (self.sort.as_ref()?.0)(state).filter(|s| self.sorts_by(s.column))
    }
    /// The heading the header cursor is on: the column cursor's, else the
    /// sorted column, else the first sortable heading.
    fn header_column(&self, state: &S) -> Option<usize> {
        self.active_column(state)
            .filter(|&i| i < self.header_columns())
            .or_else(|| self.sorted(state).map(|s| s.column))
            .or_else(|| (0..self.header_columns()).find(|&i| self.sorts_by(i)))
    }
    /// The column the keys act on: the header cursor's on the header, the
    /// column cursor's elsewhere.
    fn current_column(&self, state: &S) -> Option<usize> {
        if self.on_header(state) {
            self.header_column(state)
        } else {
            self.active_column(state)
        }
    }
    fn sort_by(&self, state: &S, column: usize) -> EventResult<M> {
        let Some((read, on_change)) = &self.sort else {
            return EventResult::Ignored;
        };
        if !self.sorts_by(column) {
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
        EventResult::Emit(on_change(request))
    }
    /// The top-row offset that shows `cursor`, moving the view as little as it
    /// must, or keeps it for `None`.
    fn top_showing(&self, state: &S, cursor: Option<usize>) -> usize {
        let requested = self.scroll.as_ref().map(|(read, _)| read(state));
        self.viewport
            .offset_for_cursor(self.body, self.items.len(), requested, cursor)
    }
    fn move_to(&self, state: &S, index: usize) -> EventResult<M> {
        let Some((_, on_change)) = &self.focused else {
            return EventResult::Ignored;
        };
        EventResult::Emit(on_change(
            TableCursor::Row(self.items[index].value().clone()),
            self.top_showing(state, Some(index)),
        ))
    }
    /// Move the column cursor to the wholly visible cell under `x` in the
    /// cursor row, without scrolling, so a pointer resting on a clipped edge
    /// cannot walk the view along.
    fn point_column(&self, state: &S, index: usize, x: u16) -> Option<EventResult<M>> {
        let (_, on_change) = self.column.as_ref()?;
        if self.items[index].is_disabled() {
            return None;
        }
        let viewport = &self.col_viewport;
        let column = viewport.column_at(x).filter(|&i| viewport.fully_shown(i))?;
        (self.active_column(state) != Some(column))
            .then(|| EventResult::Emit(on_change(column, viewport.current_offset())))
    }
    const fn has_selection(&self) -> bool {
        self.selected.is_some() || self.selected_many.is_some()
    }
    fn select(&self, index: usize) -> EventResult<M> {
        if let Some((_, on_select)) = &self.selected {
            return EventResult::Emit(on_select(self.items[index].value().clone()));
        }
        if let Some((_, on_toggle)) = &self.selected_many {
            return EventResult::Emit(on_toggle(self.items[index].value().clone()));
        }
        EventResult::Ignored
    }
    /// The column the sort key sorts: the header cursor's on the header, the
    /// column cursor's when it reads one, otherwise the sorted column or the
    /// first sortable heading.
    fn sort_column(&self, state: &S) -> Option<usize> {
        if self.on_header(state) {
            return self.header_column(state);
        }
        self.active_column(state)
            .or_else(|| self.header_column(state))
    }
    fn key(&self, key: KeyEvent, state: &S) -> EventResult<M> {
        if self.sort_key.is_some_and(|chord| chord.matches(&key)) {
            return self
                .sort_column(state)
                .map_or(EventResult::Ignored, |column| self.sort_by(state, column));
        }
        let on_header = self.on_header(state);
        if !key.modifiers.any()
            && matches!(
                key.code,
                KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l')
            )
            && let Some((_, on_change)) = &self.column
        {
            let columns = if on_header {
                self.header_columns()
            } else {
                self.widths.len()
            };
            let requested = self.horizontal_scroll.as_ref().map(|(read, _)| read(state));
            return match linear_nav::nav_key_target(
                key,
                Axis::Horizontal,
                columns,
                self.current_column(state),
                1,
                |_| false,
            ) {
                Some(NavOutcome::Move(i)) => {
                    EventResult::Emit(on_change(i, self.col_viewport.offset_showing(requested, i)))
                }
                Some(NavOutcome::Stay) => EventResult::Consumed,
                None => EventResult::Ignored,
            };
        }
        let Some((_, on_change)) = &self.focused else {
            return EventResult::Ignored;
        };
        if on_header {
            return self.header_key(key, state);
        }
        let climb = || {
            EventResult::Emit(on_change(
                TableCursor::Header,
                self.top_showing(state, None),
            ))
        };
        let step = linear_nav::step_key(key, Axis::Vertical);
        match list_core::key_intent(
            key,
            Axis::Vertical,
            &self.items,
            self.row_cursor(state),
            self.viewport.visible_items(self.body),
        ) {
            Some(KeyIntent::Move(i)) => self.move_to(state, i),
            Some(KeyIntent::Commit(i)) => self.select(i),
            // A step up from the first row climbs onto the header.
            Some(KeyIntent::Stay) if step == Some(Step::Backward) && self.header_is_a_place() => {
                climb()
            }
            Some(KeyIntent::Stay) => EventResult::Consumed,
            // With no enabled row to land on, a step lands on the header.
            None if step.is_some() && self.header_is_a_place() => climb(),
            None => EventResult::Ignored,
        }
    }
    /// A key while the cursor is on the header: commit keys sort, keys that go
    /// down come into the rows, and keys that go up have nowhere to go.
    fn header_key(&self, key: KeyEvent, state: &S) -> EventResult<M> {
        if !key.modifiers.any() && matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
            return match self.header_column(state).map(|i| self.sort_by(state, i)) {
                Some(EventResult::Emit(message)) => EventResult::Emit(message),
                _ => EventResult::Consumed,
            };
        }
        // The header is the place above the first row: place 0, with row `i`
        // at place `i + 1`, so the shared key map answers from there.
        match linear_nav::nav_key_target(
            key,
            Axis::Vertical,
            self.items.len() + 1,
            Some(0),
            self.viewport.visible_items(self.body).max(1),
            |place| place > 0 && self.items[place - 1].is_disabled(),
        ) {
            Some(NavOutcome::Move(place)) => self.move_to(state, place - 1),
            Some(NavOutcome::Stay) => EventResult::Consumed,
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
        is_selected: impl Fn(&T) -> bool,
    ) -> Vec<Row<'static>> {
        let rows = self.visible_rows();
        let start = rows.start;
        self.items[rows]
            .iter()
            .enumerate()
            .map(|(offset, item)| {
                let index = start + offset;
                let row_state = ListItemState {
                    index,
                    value: item.value(),
                    label: item.label(),
                    focused: cursor == Some(index),
                    selected: is_selected(item.value()),
                    disabled: self.disabled || item.is_disabled(),
                };
                let mut role = style.resolve_row(
                    row_state.focused,
                    row_state.selected,
                    row_state.disabled,
                    surface,
                );
                if row_state.focused && band {
                    // The selection's fill is the cursor cell's; the rest of
                    // the row is band, lettered as a selected row.
                    role = role.bg(style.focused_band_background);
                    if row_state.selected {
                        role = role.fg(style.selected_foreground);
                    }
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
        self.col_viewport
            .columns
            .iter()
            .position(|r| r.contains(point))
            .filter(|&i| self.sorts_by(i))
    }

    /// What every band of one frame is painted with.
    fn look(&self, ctx: &PaintCtx<'_, S>) -> Look {
        let style = resolve_style(self.style.as_deref(), ctx.theme, TableStyle::from_theme);
        // One cursor, shown while the table is focused or under the pointer,
        // exactly as a list shows its own.
        let interactive = !self.disabled && !self.widths.is_empty();
        let hovered = interactive && ctx.hovered();
        let cursor_visible = interactive && (ctx.focused() || hovered);
        let (widths, flex) = self.col_viewport.render_layout(&self.widths, self.flex);
        let surface = style.resolve_surface(cursor_visible, hovered, self.disabled);
        let (text, divider) = if self.disabled {
            (style.disabled_foreground, style.disabled_foreground)
        } else {
            (style.foreground, style.divider_foreground)
        };
        Look {
            style,
            widths,
            flex,
            interactive,
            hovered,
            cursor_visible,
            surface,
            base: Style::default().fg(text).bg(surface),
            divider,
        }
    }

    /// A native table over `rows`, themed and laid out as this frame paints.
    fn native(&self, look: &Look, rows: Vec<Row<'static>>) -> TableWidget<'static> {
        TableWidget::new(rows, look.widths.iter().copied())
            .style(look.style)
            .focused(look.cursor_visible)
            .hovered(look.hovered)
            .disabled(self.disabled)
    }

    /// Finish a themed widget with this table's flex and spacing.
    fn laid_out(&self, look: &Look, widget: TableWidget<'static>) -> NativeTable<'static> {
        widget
            .into_inner()
            .flex(look.flex)
            .column_spacing(self.spacing)
    }

    /// The body: visible rows, the cursor row or band, and the cursor cell.
    fn paint_body(&self, look: &Look, ctx: &mut PaintCtx<'_, S>) {
        let state = ctx.state();
        let style = &look.style;
        let active = self.active_column(state).filter(|_| look.cursor_visible);
        let cursor = self
            .row_cursor(state)
            .filter(|&i| look.cursor_visible && !self.items[i].is_disabled());
        let single_selected = self.selected.as_ref().and_then(|(read, _)| read(state));
        let is_selected = |value: &T| match &self.selected_many {
            Some((read, _)) => read(state, value),
            None => single_selected.as_ref() == Some(value),
        };
        let visible = self.visible_rows();
        let rows = self.body_rows(
            state,
            style,
            look.surface,
            cursor,
            active.is_some(),
            is_selected,
        );
        // The row already carries the cursor foreground; only swap the active
        // cell's fill so explicit span colors inside `paint_item` survive. The
        // selected cursor — the row, or its cell with a column cursor — is
        // lettered over them, to stay legible on its own fill.
        let selected_cursor = cursor.is_some_and(|i| is_selected(self.items[i].value()));
        let cursor_cell = style.cursor(selected_cursor);
        let (row_style, cell_style) = match (selected_cursor, active.is_some()) {
            (true, false) => (cursor_cell, Style::default()),
            (true, true) => (Style::default(), cursor_cell),
            (false, _) => (
                Style::default(),
                Style {
                    bg: cursor_cell.bg,
                    ..Style::new()
                },
            ),
        };
        let table = self
            .laid_out(look, self.native(look, rows))
            .row_highlight_style(row_style)
            .cell_highlight_style(cell_style);
        let mut native = TableState::default()
            .with_selected(
                cursor
                    .filter(|i| visible.contains(i))
                    .map(|i| i - visible.start),
            )
            .with_selected_column(active);
        self.render_band(table, Some(&mut native), self.body, look, ctx);
    }

    fn render_band(
        &self,
        table: NativeTable<'static>,
        native_state: Option<&mut TableState>,
        target: Rect,
        look: &Look,
        ctx: &mut PaintCtx<'_, S>,
    ) {
        if target.is_empty() {
            return;
        }
        if !self.col_viewport.overflows() {
            ctx.with_buffer(target, |r, b| render_native(table, r, b, native_state));
            return;
        }
        let canvas = Rect::new(0, 0, self.col_viewport.canvas_width, target.height);
        let mut scratch = Buffer::empty(canvas);
        scratch.set_style(canvas, look.base);
        render_native(table, canvas, &mut scratch, native_state);
        ctx.with_buffer(target, |r, b| {
            self.col_viewport
                .composite(&scratch, target, r, b, look.divider);
        });
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
        let cursor = if cursor_visible && self.on_header(state) {
            self.header_column(state)
        } else {
            None
        };
        let sorted = self.sorted(state);
        Row::new(headings.iter().enumerate().map(|(i, heading)| {
            let mut heading = heading.clone();
            if let Some(sort) = sorted.filter(|s| s.column == i) {
                let col_width = self.col_viewport.column_width(i);
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
        }
        assert!(
            !(self.selected.is_some() && self.selected_many.is_some()),
            "Table::selection(...) and Table::multi_selection(...) cannot be used together; choose one selection mode"
        );
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

        let canvas = self.canvas_widths(self.head.width);
        self.col_viewport.lay_out(
            &self.widths,
            canvas,
            self.flex,
            self.spacing,
            self.frozen_columns,
            self.head,
        );
        let column = self.current_column(ctx.state());
        let requested_col = self
            .horizontal_scroll
            .as_ref()
            .map(|(read, _)| read(ctx.state()));
        ctx.transient::<ColWheelHold>()
            .settle(&mut self.col_viewport, column, requested_col);
        self.col_viewport.place_columns(self.head);

        let cursor = self.row_cursor(ctx.state());
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
        let look = self.look(ctx);
        let area = ctx.area();
        ctx.with_buffer(area, |a, b| b.set_style(a, look.base));
        if let Some(block) = &self.block {
            ctx.widget(block, area);
        }
        let hover = ctx
            .hover_position()
            .and_then(|p| self.header_at(p))
            .filter(|_| look.interactive);
        if let Some(headings) = &self.header {
            let row = self.header_row(headings, state, &look.style, hover, look.cursor_visible);
            let table = self.laid_out(&look, self.native(&look, Vec::new()).header(row));
            self.render_band(table, None, self.head, &look, ctx);
        }
        if let Some(cells) = &self.footer {
            let footer = Row::new(cells.clone());
            let table = self.laid_out(&look, self.native(&look, Vec::new()).footer(footer));
            self.render_band(table, None, self.foot, &look, ctx);
        }
        self.paint_body(&look, ctx);
        if look.interactive
            && (hover.is_some()
                || self.has_selection()
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
                let cursor = self.row_cursor(state);
                if let MouseKind::Scroll(direction) = m.kind {
                    if matches!(direction, ScrollDirection::Left | ScrollDirection::Right) {
                        let column = self.current_column(state);
                        return self
                            .col_viewport
                            .wheel(direction, column, ctx)
                            .map_or(EventResult::Ignored, |next| {
                                emit_scroll(self.horizontal_scroll.as_ref(), state, next)
                            });
                    }
                    if self.body.is_empty() {
                        return EventResult::Ignored;
                    }
                    return self
                        .viewport
                        .wheel(direction, &self.items, cursor, self.body, ctx)
                        .map_or(EventResult::Ignored, |next| {
                            emit_scroll(self.scroll.as_ref(), state, next)
                        });
                }
                let header = self.header_at(Position::new(m.column, m.row));
                let mut pressed_here = true;
                let row = self
                    .viewport
                    .row_at(self.body, self.items.len(), m.column, m.row);
                if m.kind == MouseKind::Moved
                    && let Some(index) = row.filter(|&i| cursor == Some(i))
                    && let Some(moved) = self.point_column(state, index, m.column)
                {
                    return moved;
                }
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
                match list_core::row_intent(m.kind, &self.items, row, cursor, self.has_selection())
                {
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
                && (self.column.is_some()
                    || self.header_is_a_place()
                    || self.sort_key.is_some() && self.has_sortable_heading()
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
    use crate::runtime::{FocusState, Ratcn};
    use crate::test_support::{Driver, key, key_with, mouse};
    use crate::{Button, List, ListStyle};

    #[derive(Default)]
    struct State {
        focus: FocusState,
        cursor: Option<TableCursor<u8>>,
        scroll: usize,
        selected: Option<u8>,
        column: usize,
        sort: Option<TableSort>,
    }
    #[derive(Debug, PartialEq)]
    enum Msg {
        Focus(FocusState),
        Move(u8, usize),
        Header(usize),
        Select(u8),
        Column(usize, usize),
        Scroll(usize),
        Sort(TableSort),
        Button,
    }
    /// The cursor message, split so assertions read as rows and header.
    fn on_cursor(cursor: TableCursor<u8>, top: usize) -> Msg {
        match cursor {
            TableCursor::Row(id) => Msg::Move(id, top),
            TableCursor::Header => Msg::Header(top),
        }
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
        .item_focus(|s: &State| s.cursor, on_cursor)
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
            cursor: Some(TableCursor::Row(2)),
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
            cursor: Some(TableCursor::Row(2)),
            ..State::default()
        };
        frame(&mut driver, &state, 25, false);
        assert_eq!(
            driver.event(key(KeyCode::Up), &state),
            EventResult::Emit(Msg::Move(0, 0)),
            "the disabled row is stepped over"
        );
        state.cursor = Some(TableCursor::Row(0));
        assert_eq!(
            driver.event(key(KeyCode::Up), &state),
            EventResult::Emit(Msg::Header(1)),
            "the view stays where it was painted"
        );
        assert_eq!(
            driver.event(key(KeyCode::Char('s')), &state),
            EventResult::Ignored,
            "a single letter is the app's hotkey, as on a list"
        );
        state.cursor = Some(TableCursor::Header);
        frame(&mut driver, &state, 25, false);
        assert_eq!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Consumed,
            "column 0 is not sortable"
        );
        assert_eq!(
            driver.event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Column(1, 0))
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
            cursor: Some(TableCursor::Row(3)),
            ..State::default()
        };
        frame(&mut driver, &state, 25, false);
        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 22, 3), &state),
            EventResult::Emit(Msg::Move(2, 2)),
            "over another row, in another column, the row moves first"
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
            cursor: Some(TableCursor::Row(0)),
            ..State::default()
        };
        frame(&mut driver, &state, 25, false);
        driver.event(mouse(MouseKind::Moved, 22, 2), &state);
        frame(&mut driver, &state, 25, false);
        let painted = |driver: &Driver<State, Msg>, x| {
            let cell = driver.cell(x, 2);
            (cell.fg, cell.bg)
        };
        let active = painted(&driver, 2);
        assert_eq!(
            active,
            (style.active_header_foreground, style.hovered_background)
        );
        let hovered = painted(&driver, 22);
        assert_eq!(
            hovered,
            (
                style.hovered_header_foreground,
                style.hovered_header_background
            )
        );
        assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Pointer);
        // The cursor row is a band, and only the active cell is the cursor.
        assert_eq!(driver.cell(2, 3).bg, style.focused_row_background);
        assert_eq!(driver.cell(22, 3).bg, style.focused_band_background);

        state.cursor = Some(TableCursor::Header);
        frame(&mut driver, &state, 25, false);
        let cursor = painted(&driver, 2);
        assert_eq!(
            cursor,
            (style.focused_foreground, style.focused_row_background)
        );
        assert_eq!(
            painted(&driver, 22),
            hovered,
            "the pointer still marks its own"
        );
        assert_eq!(
            driver.cell(2, 3).bg,
            style.hovered_background,
            "no row cursor"
        );
        let all = [active, hovered, cursor];
        assert!(
            all.iter()
                .enumerate()
                .all(|(i, a)| all[i + 1..].iter().all(|b| a.1 != b.1)),
            "three headings, three painted fills: {all:?}"
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
                        .item_focus(
                            |_: &Pair| Some(TableCursor::Row(1)),
                            |_, _| FocusState::default(),
                        )
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
                .item_focus(|s: &State| s.cursor, on_cursor)
                .column_focus(|s| Some(s.column), Msg::Column)
                .sort([1], |s| s.sort, Msg::Sort);
                ctx.component("table", empty, Rect::new(0, 0, 20, 4));
            });
        };
        render(&mut driver, &state);
        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Header(0)),
            "entering an empty table with header_focus lands on the header"
        );
        state.cursor = Some(TableCursor::Header);
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
    /// click can sort is not one; a column cursor alone is, since Left and
    /// Right move it.
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
            headed().sort_key('s').scope_options(&state).focusable,
            "a sort key acts there"
        );
        let columns = || {
            Table::<u8, State, Msg>::new(
                [ListItem::new(0u8, "a")],
                [Constraint::Length(4), Constraint::Length(4)],
            )
            .column_focus(|s| Some(s.column), Msg::Column)
        };
        assert!(columns().scope_options(&state).focusable);
        assert!(!columns().disabled(true).scope_options(&state).focusable);
        let state = State {
            column: 0,
            ..State::default()
        };
        let mut driver = focused_driver();
        driver.render(&state, |ctx| {
            ctx.component("table", columns(), Rect::new(0, 0, 12, 2));
        });
        assert_eq!(
            driver.event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Column(1, 0)),
            "a column cursor alone takes Left and Right"
        );
        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Ignored,
            "with no row cursor, vertical keys are not the table's"
        );
        let navigable = headed().item_focus(|s| s.cursor, on_cursor);
        assert!(navigable.scope_options(&state).focusable);
    }

    /// Default state starts with no cursor — matching `List` — even when the
    /// header is a place; once on the header, the cursor is always on a
    /// heading: with no column cursor yet, the first sortable one.
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

        state.cursor = Some(TableCursor::Header);
        render(&mut driver, &state);
        assert_ne!(driver.cell(2, 2).bg, style.focused_row_background);
        assert_eq!(driver.cell(22, 2).bg, style.focused_row_background);
        assert_eq!(
            driver.event(key(KeyCode::Left), &state),
            EventResult::Emit(Msg::Column(0, 0)),
            "Left steps from the column the cursor is painted on"
        );
        assert_eq!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::Sort(TableSort {
                column: 1,
                direction: SortDirection::Ascending
            }))
        );
        // A stale row value while not on the header is no cursor.
        state.cursor = Some(TableCursor::Row(9));
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
            cursor: Some(TableCursor::Row(0)),
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
            .item_focus(|s: &State| s.cursor, on_cursor)
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
            cursor: Some(TableCursor::Header),
            ..State::default()
        };
        driver.render(&state, |ctx| {
            let table = Table::new([ListItem::new(7u8, "only")], [Constraint::Fill(1)])
                .header(["Name"])
                .item_focus(|s: &State| s.cursor, on_cursor)
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

    /// Frozen columns stay pinned while `column_focus` scrolls the remaining
    /// columns into view, keeping header, body and footer aligned and showing
    /// the divider rule only once columns are scrolled behind the frozen edge.
    #[test]
    fn frozen_columns_stay_pinned_while_column_focus_scrolls_horizontally() {
        let mut driver = focused_driver();
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                let table = Table::new(
                    [ListItem::new(0u8, "row0")],
                    [
                        Constraint::Length(6),
                        Constraint::Length(8),
                        Constraint::Length(8),
                        Constraint::Length(8),
                    ],
                )
                .column_spacing(2)
                .frozen_columns(1)
                .header(["Date", "Payee", "Kind", "Amount"])
                .footer(["Sum", "f1", "f2", "$99"])
                .paint_item(|_, _| Row::new(["Nov 24", "Grocer", "Food", "-$62"]))
                .item_focus(|s: &State| s.cursor, on_cursor)
                .column_focus(|s| Some(s.column), Msg::Column)
                .sort([0, 3], |s| s.sort, Msg::Sort);
                ctx.component("table", table, Rect::new(0, 0, 22, 3));
            });
        };

        let mut state = State {
            cursor: Some(TableCursor::Row(0)),
            column: 0,
            ..State::default()
        };
        render(&mut driver, &state);
        assert!(driver.row(0).starts_with("Date    Payee"));
        assert!(!driver.row(0).contains('│'), "no divider at offset 0");
        assert!(
            !driver.row(0).contains("Amount"),
            "Amount starts off-screen"
        );

        // Moving column_focus to the last column scrolls it into view while
        // Date remains pinned on the left with a divider across all bands.
        state.column = 3;
        render(&mut driver, &state);
        assert!(driver.row(0).starts_with("Date   │"));
        assert!(driver.row(0).contains("Amount"));
        assert!(driver.row(1).starts_with("Nov 24 │"));
        assert!(driver.row(1).contains("-$62"));
        assert!(driver.row(2).starts_with("Sum    │"));
        assert!(driver.row(2).contains("$99"));

        // Both the frozen heading (col 0) and the scrolled-in heading (col 3)
        // hit-test to their own column indices.
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 1, 0), &state),
            EventResult::Emit(Msg::Sort(TableSort {
                column: 0,
                direction: SortDirection::Ascending,
            }))
        );
        let amount_x = (0..22)
            .find(|&x| driver.cell(x, 0).symbol() == "A")
            .expect("Amount heading visible after scroll");
        assert_eq!(
            driver.event(
                mouse(MouseKind::Click(MouseButton::Left), amount_x, 0),
                &state
            ),
            EventResult::Emit(Msg::Sort(TableSort {
                column: 3,
                direction: SortDirection::Ascending,
            }))
        );

        // Moving back onto column 1 pulls the scroll offset back to 0.
        state.column = 1;
        render(&mut driver, &state);
        assert!(driver.row(0).starts_with("Date    Payee"));
        assert!(!driver.row(0).contains('│'));
    }

    /// Horizontal wheel scrolls columns independently of `column_focus`,
    /// bubbles at both ends, and emits through `horizontal_scroll` when bound.
    #[test]
    fn horizontal_wheel_scrolls_holds_and_bubbles_at_edges() {
        #[derive(Default)]
        struct HState {
            focus: FocusState,
            col: usize,
            scroll_x: usize,
        }
        #[derive(Debug, PartialEq)]
        enum HMsg {
            Focus(FocusState),
            Col(usize, usize),
            ScrollX(usize),
        }
        let mut driver: Driver<HState, HMsg> = Driver::with(
            Ratcn::new().focus(|s: &HState| &s.focus, HMsg::Focus),
            24,
            3,
        );
        let render = |driver: &mut Driver<HState, HMsg>, state: &HState| {
            driver.render(state, |ctx| {
                let table = Table::new(
                    [ListItem::new(0u8, "r")],
                    [
                        Constraint::Length(6),
                        Constraint::Length(8),
                        Constraint::Length(8),
                        Constraint::Length(8),
                    ],
                )
                .frozen_columns(1)
                .header(["ID", "Col1", "Col2", "Col3"])
                .column_focus(|s: &HState| Some(s.col), HMsg::Col)
                .horizontal_scroll(|s| s.scroll_x, HMsg::ScrollX);
                ctx.component("t", table, Rect::new(0, 0, 20, 2));
            });
        };

        let mut state = HState::default();
        render(&mut driver, &state);
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Left), 2, 0),
                &state
            ),
            EventResult::Ignored,
            "left wheel at offset 0 bubbles"
        );
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Right), 2, 0),
                &state
            ),
            EventResult::Emit(HMsg::ScrollX(1))
        );
        state.scroll_x = 1;
        render(&mut driver, &state);
        assert!(driver.row(0).contains("Col2"));
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Right), 2, 0),
                &state
            ),
            EventResult::Emit(HMsg::ScrollX(2))
        );
        state.scroll_x = 2;
        render(&mut driver, &state);
        assert!(driver.row(0).contains("Col3"));
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Right), 2, 0),
                &state
            ),
            EventResult::Ignored,
            "right wheel at max offset bubbles"
        );
    }

    /// Frozen columns wider than the viewport and wide CJK glyphs on the clip
    /// boundary render safely without panicking or bleeding past the clip edge.
    #[test]
    fn horizontal_clipping_handles_wide_glyphs_and_oversized_frozen_columns() {
        let mut driver = Driver::<State, Msg>::new(12, 3);
        let state = State::default();
        driver.render(&state, |ctx| {
            let table = Table::new(
                [ListItem::new(0u8, "r")],
                [Constraint::Length(5), Constraint::Length(8)],
            )
            .frozen_columns(1)
            .paint_item(|_, _| Row::new(["ab中d", "ef中gh"]))
            .header(["H1", "H2"]);
            ctx.component("t", table, Rect::new(0, 0, 9, 2));
        });
        // Every cell at the right clip edge has display width <= 1.
        for y in 0..2 {
            assert!(display_width(driver.cell(8, y).symbol()) <= 1);
        }
        // Oversized frozen count (more frozen columns than viewport width).
        driver.render(&state, |ctx| {
            let table = Table::new(
                [ListItem::new(0u8, "r")],
                [Constraint::Length(10), Constraint::Length(10)],
            )
            .frozen_columns(5)
            .header(["LongHeader", "Second"]);
            ctx.component("t", table, Rect::new(0, 0, 6, 2));
        });
        assert_eq!(driver.row(0).trim_end(), "LongHe");
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
            .footer(footer.style(Style::default().fg(style.footer_foreground)))
            .row_highlight_style(
                Style::default()
                    .fg(style.focused_foreground)
                    .bg(style.focused_row_background),
            );
        StatefulWidget::render(raw, area, &mut expected, &mut state.clone());
        assert_eq!(got, expected);
    }

    /// A wide ledger: one frozen column and five that scroll, 36 cells on a
    /// virtual canvas shown 20 at a time (7 frozen, 13 scrolling).
    #[derive(Default)]
    struct Wide {
        focus: FocusState,
        cursor: Option<TableCursor<u8>>,
        column: usize,
        scroll_x: usize,
        sort: Option<TableSort>,
    }
    #[derive(Debug, PartialEq)]
    enum WideMsg {
        Focus(FocusState),
        Cursor(TableCursor<u8>, usize),
        Column(usize, usize),
        ScrollX(usize),
        Sort(TableSort),
    }
    impl Wide {
        fn update(&mut self, msg: WideMsg) {
            match msg {
                WideMsg::Focus(focus) => self.focus = focus,
                WideMsg::Cursor(cursor, _) => self.cursor = Some(cursor),
                // Column and offset stored in one update, as documented.
                WideMsg::Column(column, offset) => {
                    self.column = column;
                    self.scroll_x = offset;
                }
                WideMsg::ScrollX(offset) => self.scroll_x = offset,
                WideMsg::Sort(sort) => self.sort = Some(sort),
            }
        }
    }
    fn wide() -> Table<u8, Wide, WideMsg> {
        Table::new(
            [ListItem::new(0u8, "r")],
            [6, 5, 5, 5, 5, 5].map(Constraint::Length),
        )
        .frozen_columns(1)
        .header(["c0", "c1", "c2", "c3", "c4", "c5"])
        .paint_item(|_, _| Row::new(["r0", "r1", "r2", "r3", "r4", "r5"]))
        .item_focus(|s: &Wide| s.cursor, WideMsg::Cursor)
        .sort(0..6, |s| s.sort, WideMsg::Sort)
    }
    /// The wide ledger with a column cursor, and its offset bound or not.
    fn wide_columns(bound: bool) -> Table<u8, Wide, WideMsg> {
        let table = wide().column_focus(|s| Some(s.column), WideMsg::Column);
        if bound {
            table.horizontal_scroll(|s| s.scroll_x, WideMsg::ScrollX)
        } else {
            table
        }
    }
    fn wide_driver() -> Driver<Wide, WideMsg> {
        Driver::with(
            Ratcn::new().focus(|s: &Wide| &s.focus, WideMsg::Focus),
            40,
            3,
        )
    }
    fn draw(
        driver: &mut Driver<Wide, WideMsg>,
        state: &Wide,
        width: u16,
        table: impl FnOnce() -> Table<u8, Wide, WideMsg>,
    ) {
        let mut table = Some(table);
        driver.render(state, |ctx| {
            let table = table.take().expect("declared once per frame")();
            ctx.component("t", table, Rect::new(0, 0, width, 2));
        });
    }
    /// Send `event`, apply what it emits, and paint the next frame.
    fn send(
        driver: &mut Driver<Wide, WideMsg>,
        state: &mut Wide,
        event: Event,
        bound: bool,
    ) -> Option<(usize, usize)> {
        let emitted = match driver.event(event, state) {
            EventResult::Emit(WideMsg::Column(column, offset)) => {
                state.update(WideMsg::Column(column, offset));
                Some((column, offset))
            }
            EventResult::Emit(msg) => {
                state.update(msg);
                None
            }
            _ => None,
        };
        draw(driver, state, 20, || wide_columns(bound));
        emitted
    }
    fn header(driver: &Driver<Wide, WideMsg>) -> String {
        driver.row(0)[..20].trim_end().to_owned()
    }

    /// Stepping off the right edge and back reports the offset each step
    /// leaves the view at, moving it only as far as the new column needs.
    #[test]
    fn column_focus_reports_the_minimal_offset_both_ways() {
        let mut driver = wide_driver();
        let mut state = Wide {
            column: 1,
            ..Wide::default()
        };
        draw(&mut driver, &state, 20, || wide_columns(false));
        let mut steps = Vec::new();
        for code in [KeyCode::Right; 5].into_iter().chain([KeyCode::Left; 4]) {
            steps.extend(send(&mut driver, &mut state, key(code), false));
        }
        assert_eq!(
            steps,
            [
                (2, 0),
                (3, 1),
                (4, 2),
                (5, 3),
                // Back left: the view holds until the column leaves it.
                (4, 3),
                (3, 2),
                (2, 1),
                (1, 0),
            ]
        );
    }

    /// The reviewer's scenario: with the app storing column and offset in one
    /// update, stepping back from the last column does not move a view that
    /// already shows the new column — bound or not, painting the same.
    #[test]
    fn stepping_back_inside_the_view_does_not_jump_bound_or_unbound() {
        let mut frames = Vec::new();
        for bound in [true, false] {
            let mut driver = wide_driver();
            let mut state = Wide {
                column: 1,
                ..Wide::default()
            };
            draw(&mut driver, &state, 20, || wide_columns(bound));
            for _ in 1..5 {
                send(&mut driver, &mut state, key(KeyCode::Right), bound);
            }
            assert!(header(&driver).ends_with("c4    c5"), "{bound}");
            send(&mut driver, &mut state, key(KeyCode::Left), bound);
            assert_eq!(state.column, 4);
            assert!(
                header(&driver).ends_with("c4    c5"),
                "bound {bound}: column 4 was in view, so nothing moves"
            );
            frames.push((driver.row(0), driver.row(1)));
        }
        assert_eq!(frames[0], frames[1], "bound and unbound paint the same");
    }

    /// Unbound, the wheel can push the active column off-screen and the view
    /// stays there across frames; moving the column brings it back.
    #[test]
    fn an_unbound_wheel_holds_the_view_until_the_column_moves() {
        let mut driver = wide_driver();
        let mut state = Wide {
            column: 3,
            ..Wide::default()
        };
        draw(&mut driver, &state, 20, || wide_columns(false));
        assert!(header(&driver).contains("c3"));
        let right = mouse(MouseKind::Scroll(ScrollDirection::Right), 10, 0);
        for _ in 0..2 {
            assert_eq!(driver.event(right.clone(), &state), EventResult::Consumed);
            draw(&mut driver, &state, 20, || wide_columns(false));
        }
        assert!(header(&driver).ends_with("c4    c5"));
        draw(&mut driver, &state, 20, || wide_columns(false));
        assert!(
            !header(&driver).contains("c3"),
            "the wheeled view holds with the active column off-screen"
        );
        // A click lands on the column painted there, not the one held before.
        let x = u16::try_from(driver.row(0).find("c4").expect("c4 painted")).expect("x");
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), x, 0), &state),
            EventResult::Emit(WideMsg::Sort(TableSort {
                column: 4,
                direction: SortDirection::Ascending
            }))
        );
        assert_eq!(
            send(&mut driver, &mut state, key(KeyCode::Left), false),
            Some((2, 1))
        );
        assert!(header(&driver).contains("c2"), "the new column snaps in");
    }

    /// The divider lives in the gap after the frozen columns; with no gap it
    /// is not drawn, and the frozen text keeps every glyph.
    #[test]
    fn no_divider_without_column_spacing() {
        let mut driver = wide_driver();
        let state = Wide {
            column: 5,
            ..Wide::default()
        };
        for (spacing, divided) in [(1, true), (0, false)] {
            draw(&mut driver, &state, 12, || {
                wide_columns(false)
                    .column_spacing(spacing)
                    .paint_item(|_, _| Row::new(["frozen", "r1", "r2", "r3", "r4", "r5"]))
            });
            assert_eq!(driver.row(1).contains('│'), divided, "spacing {spacing}");
            if divided {
                let rule = TableStyle::from_theme(&Theme::default_dark()).divider_foreground;
                assert_eq!(
                    driver.cell(6, 1).fg,
                    rule,
                    "the rule is a rule, not row text"
                );
            }
            assert!(
                driver
                    .row(1)
                    .starts_with(if divided { "frozen│" } else { "frozen" }),
                "spacing {spacing}: {:?}",
                driver.row(1)
            );
        }
    }

    /// The header column the cursor is on scrolls into view like the column
    /// cursor does, though nothing but the header names it.
    #[test]
    fn the_header_cursor_scrolls_its_column_into_view() {
        let style = TableStyle::from_theme(&Theme::default_dark());
        let mut driver = wide_driver();
        let mut state = Wide {
            cursor: Some(TableCursor::Row(0)),
            sort: Some(TableSort {
                column: 5,
                direction: SortDirection::Ascending,
            }),
            ..Wide::default()
        };
        draw(&mut driver, &state, 20, wide);
        assert!(!header(&driver).contains("c5"), "off the header, no column");
        state.cursor = Some(TableCursor::Header);
        draw(&mut driver, &state, 20, wide);
        let x = u16::try_from(driver.row(0).find("c5").expect("c5 scrolled in")).expect("x");
        assert_eq!(driver.cell(x, 0).bg, style.focused_row_background);
        assert_eq!(
            driver.event(key(KeyCode::Left), &state),
            EventResult::Ignored,
            "without a column cursor the header takes no Left or Right"
        );
    }

    /// Columns that stop fitting start scrolling, and scroll back out of it
    /// once they fit again, with hit-testing following the painted layout.
    #[test]
    fn resizing_between_overflow_and_fit() {
        let mut driver = wide_driver();
        let state = Wide {
            column: 5,
            ..Wide::default()
        };
        draw(&mut driver, &state, 20, || wide_columns(false));
        assert!(header(&driver).ends_with("c4    c5"));
        draw(&mut driver, &state, 40, || wide_columns(false));
        assert_eq!(
            driver.row(0).trim_end(),
            "c0     c1    c2    c3    c4    c5",
            "everything fits: no scroll, no divider"
        );
        let click = |driver: &mut Driver<Wide, WideMsg>, x| {
            driver.event(mouse(MouseKind::Click(MouseButton::Left), x, 0), &state)
        };
        assert_eq!(
            click(&mut driver, 13),
            EventResult::Emit(WideMsg::Sort(TableSort {
                column: 2,
                direction: SortDirection::Ascending
            }))
        );
        draw(&mut driver, &state, 20, || wide_columns(false));
        assert!(header(&driver).ends_with("c4    c5"));
        assert_eq!(
            click(&mut driver, 8),
            EventResult::Emit(WideMsg::Sort(TableSort {
                column: 4,
                direction: SortDirection::Ascending
            }))
        );
    }

    /// A `Fill` column has no width of its own on a scrolling canvas, so it
    /// takes the wider of its heading and footer rather than vanishing.
    #[test]
    fn a_fill_column_keeps_its_heading_width_when_overflowing() {
        let mut driver = wide_driver();
        let state = Wide {
            column: 1,
            ..Wide::default()
        };
        // A one-row body between header and footer.
        driver.render(&state, |ctx| {
            let table = Table::new(
                [ListItem::new(0u8, "r")],
                [
                    Constraint::Length(8),
                    Constraint::Fill(1),
                    Constraint::Length(8),
                ],
            )
            .header(["Name", "Notes", "Amount"])
            .footer(["", "Subtotal", ""])
            .column_focus(|s: &Wide| Some(s.column), WideMsg::Column);
            ctx.component("t", table, Rect::new(0, 0, 12, 3));
        });
        assert!(driver.row(0).starts_with("Notes"), "{:?}", driver.row(0));
        assert!(driver.row(2).starts_with("Subtotal"), "{:?}", driver.row(2));
    }

    /// More frozen columns than there are, wider than the view: nothing
    /// scrolls, an active column off-screen stays off-screen, and keys and the
    /// wheel neither panic nor pretend to scroll.
    #[test]
    fn frozen_past_the_column_count_with_an_off_screen_column() {
        let mut driver = wide_driver();
        let mut state = Wide {
            column: 1,
            ..Wide::default()
        };
        let table = || {
            Table::new(
                [ListItem::new(0u8, "r")],
                [Constraint::Length(10), Constraint::Length(10)],
            )
            .frozen_columns(5)
            .header(["LongHeader", "Second"])
            .column_focus(|s: &Wide| Some(s.column), WideMsg::Column)
        };
        draw(&mut driver, &state, 6, table);
        assert_eq!(driver.row(0).trim_end(), "LongHe");
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Right), 2, 0),
                &state
            ),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(key(KeyCode::Left), &state),
            EventResult::Emit(WideMsg::Column(0, 0))
        );
        state.column = 0;
        draw(&mut driver, &state, 6, table);
        assert_eq!(
            driver.event(key(KeyCode::Right), &state),
            EventResult::Emit(WideMsg::Column(1, 0))
        );
    }

    /// On the header, Left and Right move among the columns that have
    /// headings; below it, among every column.
    #[test]
    fn header_left_right_stays_within_the_headed_columns() {
        let mut driver = focused_driver();
        let mut state = State {
            cursor: Some(TableCursor::Header),
            column: 1,
            ..State::default()
        };
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                let table = Table::new([ListItem::new(0u8, "a")], [Constraint::Length(4); 3])
                    .header(["A", "B"])
                    .item_focus(|s: &State| s.cursor, on_cursor)
                    .column_focus(|s| Some(s.column), Msg::Column)
                    .sort([0, 1], |s| s.sort, Msg::Sort);
                ctx.component("table", table, Rect::new(0, 0, 16, 3));
            });
        };
        render(&mut driver, &state);
        assert_eq!(
            driver.event(key(KeyCode::Right), &state),
            EventResult::Consumed,
            "column 2 has no heading to stand on"
        );
        state.cursor = Some(TableCursor::Row(0));
        render(&mut driver, &state);
        assert_eq!(
            driver.event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Column(2, 0))
        );
    }

    /// A header with nothing to sort is not a place: Up at the top stays, as
    /// on a list, and a `Header` read there is no cursor at all.
    #[test]
    fn an_unsortable_header_is_not_a_place() {
        let mut driver = focused_driver();
        let mut state = State {
            cursor: Some(TableCursor::Row(0)),
            ..State::default()
        };
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                let table = Table::new(
                    [ListItem::new(0u8, "a"), ListItem::new(1, "b")],
                    [Constraint::Length(4), Constraint::Length(4)],
                )
                // Column 1 sorts, but has no heading.
                .header(["A"])
                .item_focus(|s: &State| s.cursor, on_cursor)
                .sort([1], |s| s.sort, Msg::Sort);
                ctx.component("table", table, Rect::new(0, 0, 16, 4));
            });
        };
        render(&mut driver, &state);
        assert_eq!(
            driver.event(key(KeyCode::Up), &state),
            EventResult::Consumed
        );
        state.cursor = Some(TableCursor::Header);
        render(&mut driver, &state);
        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Move(0, 0)),
            "no cursor yet, so the first key lands on the first row"
        );
    }

    /// The pointer moves the one cursor: over a row while on the header, it
    /// leaves the header for that row.
    #[test]
    fn pointer_motion_over_a_row_leaves_the_header() {
        let mut driver = focused_driver();
        let state = State {
            cursor: Some(TableCursor::Header),
            ..State::default()
        };
        frame(&mut driver, &state, 25, false);
        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 4, 3), &state),
            EventResult::Emit(Msg::Move(0, 0))
        );
    }

    /// Draw a one-row table at `width` and report whether the columns scroll:
    /// a horizontal wheel moves only a scrolling layout.
    fn fits(driver: &mut Driver<Wide, WideMsg>, width: u16, widths: [Constraint; 2]) -> bool {
        let state = Wide::default();
        draw(driver, &state, width, || {
            Table::new([ListItem::new(0u8, "r")], widths)
                .header(["Left", "Right"])
                .paint_item(|_, _| Row::new(["l", "r"]))
        });
        driver.event(
            mouse(MouseKind::Scroll(ScrollDirection::Right), 1, 0),
            &state,
        ) == EventResult::Ignored
    }

    /// Percentages, ratios and maximums are Ratatui's to divide, so a table
    /// of them fits wherever Ratatui would fit it, laid out exactly the same.
    #[test]
    fn flexible_widths_fit_as_ratatui_lays_them_out() {
        let mut driver = wide_driver();
        for widths in [
            [Constraint::Percentage(30), Constraint::Percentage(70)],
            [Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)],
            [Constraint::Length(10), Constraint::Max(40)],
        ] {
            let width = if matches!(widths[1], Constraint::Max(_)) {
                30
            } else {
                20
            };
            assert!(fits(&mut driver, width, widths), "{widths:?} scrolled");
            let mut native = Buffer::empty(Rect::new(0, 0, width, 1));
            Widget::render(
                NativeTable::new(Vec::<Row>::new(), widths).header(Row::new(["Left", "Right"])),
                native.area,
                &mut native,
            );
            let expected: String = (0..width)
                .map(|x| native.cell((x, 0)).expect("cell").symbol())
                .collect();
            assert_eq!(driver.row(0)[..usize::from(width)], expected, "{widths:?}");
        }
        assert!(
            !fits(&mut driver, 12, [Constraint::Length(8), Constraint::Min(8)]),
            "rigid widths past the view still scroll"
        );
    }

    /// A sorted `Fill` column keeps room for its mark on a scrolling canvas.
    #[test]
    fn a_sorted_fill_column_shows_its_mark_while_overflowing() {
        let mut driver = wide_driver();
        let state = Wide {
            column: 1,
            sort: Some(TableSort {
                column: 1,
                direction: SortDirection::Ascending,
            }),
            ..Wide::default()
        };
        draw(&mut driver, &state, 12, || {
            Table::new(
                [ListItem::new(0u8, "r")],
                [
                    Constraint::Length(10),
                    Constraint::Fill(1),
                    Constraint::Length(10),
                ],
            )
            .header(["A", "Notes", "B"])
            .sort([1], |s: &Wide| s.sort, WideMsg::Sort)
            .column_focus(|s| Some(s.column), WideMsg::Column)
        });
        assert!(driver.row(0).starts_with("Notes ▲"), "{:?}", driver.row(0));
    }

    /// A wheeled view belongs to the layout it was wheeled in: once the
    /// columns fit, there is nothing left to hold, and narrowing again brings
    /// the active column back into view.
    #[test]
    fn fitting_releases_the_wheel_hold() {
        let mut driver = wide_driver();
        let state = Wide {
            column: 5,
            ..Wide::default()
        };
        draw(&mut driver, &state, 20, || wide_columns(false));
        let left = mouse(MouseKind::Scroll(ScrollDirection::Left), 10, 0);
        for _ in 0..2 {
            assert_eq!(driver.event(left.clone(), &state), EventResult::Consumed);
            draw(&mut driver, &state, 20, || wide_columns(false));
        }
        assert!(!header(&driver).contains("c5"), "wheeled off-screen");
        draw(&mut driver, &state, 40, || wide_columns(false));
        draw(&mut driver, &state, 20, || wide_columns(false));
        assert!(
            header(&driver).ends_with("c4    c5"),
            "{:?}",
            header(&driver)
        );
    }

    /// The pointer moves the column cursor within the cursor row, mapping
    /// frozen and scrolled cells to their own columns, and says nothing when
    /// the cell is already the active one or is a gap.
    #[test]
    fn pointer_motion_within_the_cursor_row_moves_the_column() {
        let mut driver = wide_driver();
        let state = Wide {
            cursor: Some(TableCursor::Row(0)),
            column: 5,
            ..Wide::default()
        };
        draw(&mut driver, &state, 20, || wide_columns(true));
        assert_eq!(header(&driver), "c0    │c4    c5");
        let moved = |driver: &mut Driver<Wide, WideMsg>, x| {
            driver.event(mouse(MouseKind::Moved, x, 1), &state)
        };
        assert_eq!(
            moved(&mut driver, 2),
            EventResult::Emit(WideMsg::Column(0, 3))
        );
        assert_eq!(
            moved(&mut driver, 8),
            EventResult::Emit(WideMsg::Column(4, 3))
        );
        assert_eq!(
            moved(&mut driver, 14),
            EventResult::Consumed,
            "already active"
        );
        assert_eq!(
            moved(&mut driver, 6),
            EventResult::Consumed,
            "the divider gap"
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 8, 0), &state),
            EventResult::Consumed,
            "a heading is a hover, not a column move"
        );
    }

    /// A row has no selection marker, so the selected row under the cursor
    /// paints differently from the plain cursor row, and its cursor cell
    /// differently from the plain cursor cell — lettered over explicit span
    /// colors in both modes, so a primary marker stays legible.
    #[test]
    fn a_selected_cursor_row_paints_apart_from_the_cursor() {
        let style = TableStyle::from_theme(&Theme::default_dark());
        let mut driver = focused_driver();
        let paint = |driver: &mut Driver<State, Msg>, state: &State, columns: bool| {
            driver.render(state, |ctx| {
                let table = Table::new(
                    [ListItem::new(0u8, "a"), ListItem::new(1, "b")],
                    [Constraint::Length(4), Constraint::Length(4)],
                )
                // A marker in the theme's primary, as the docs suggest: on the
                // primary fill it would vanish unless the cursor letters it.
                .paint_item(|_, _| {
                    let marker = Style::new().fg(Theme::default_dark().primary);
                    Row::new([Line::from(Span::styled("x", marker)), Line::from("y")])
                })
                .item_focus(|s: &State| s.cursor, on_cursor)
                .selection(|s| s.selected, Msg::Select);
                let table = if columns {
                    table.column_focus(|s| Some(s.column), Msg::Column)
                } else {
                    table
                };
                ctx.component("table", table, Rect::new(0, 0, 10, 2));
            });
            let (cell, rest) = (driver.cell(0, 0), driver.cell(5, 0));
            ((cell.fg, cell.bg), (rest.fg, rest.bg))
        };
        let mut state = State {
            cursor: Some(TableCursor::Row(0)),
            ..State::default()
        };
        for columns in [false, true] {
            state.selected = None;
            let plain = paint(&mut driver, &state, columns);
            state.selected = Some(0);
            let chosen = paint(&mut driver, &state, columns);
            assert_ne!(plain.0, chosen.0, "columns {columns}: the cursor cell");
            assert_eq!(
                chosen.0,
                (
                    style.selected_focused_foreground,
                    style.selected_focused_background
                ),
                "columns {columns}"
            );
            if columns {
                assert_eq!(chosen.1.1, style.focused_band_background, "the band stays");
            }
        }
    }

    /// A cursor set from outside, with the app's offset left behind, paints
    /// scrolled to show it; the next key builds on that painted view.
    #[test]
    fn a_key_move_builds_on_the_painted_offset() {
        let mut driver = focused_driver();
        let state = State {
            cursor: Some(TableCursor::Row(9)),
            ..State::default()
        };
        driver.render(&state, |ctx| {
            let table = Table::new(
                (0u8..20).map(|i| ListItem::new(i, i.to_string())),
                [Constraint::Length(4)],
            )
            .item_focus(|s: &State| s.cursor, on_cursor)
            .scroll(|s| s.scroll, Msg::Scroll);
            ctx.component("table", table, Rect::new(0, 0, 8, 3));
        });
        assert_eq!(driver.row(0).trim_end(), "7");
        assert_eq!(
            driver.event(key(KeyCode::Up), &state),
            EventResult::Emit(Msg::Move(8, 7))
        );
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 1, 1),
                &state
            ),
            EventResult::Emit(Msg::Scroll(10)),
            "a bound wheel emits the offset one notch (three rows) from the painted view"
        );
    }

    /// On the header with the column cursor on an unheaded column, the header
    /// cursor stands on the first sortable heading and steps from there.
    #[test]
    fn header_keys_from_a_column_without_a_heading() {
        let mut driver = focused_driver();
        let state = State {
            cursor: Some(TableCursor::Header),
            column: 2,
            ..State::default()
        };
        driver.render(&state, |ctx| {
            let table = Table::new([ListItem::new(0u8, "a")], [Constraint::Length(4); 3])
                .header(["A", "B"])
                .item_focus(|s: &State| s.cursor, on_cursor)
                .column_focus(|s| Some(s.column), Msg::Column)
                .sort([0, 1], |s| s.sort, Msg::Sort);
            ctx.component("table", table, Rect::new(0, 0, 16, 3));
        });
        assert_eq!(
            driver.event(key(KeyCode::Left), &state),
            EventResult::Consumed
        );
        assert_eq!(
            driver.event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Column(1, 0))
        );
    }

    /// The cursor on a disabled first row still climbs to the header.
    #[test]
    fn climbing_to_the_header_from_a_disabled_cursor_row() {
        let mut driver = focused_driver();
        let state = State {
            cursor: Some(TableCursor::Row(0)),
            ..State::default()
        };
        driver.render(&state, |ctx| {
            let table = Table::new(
                [
                    ListItem::new(0u8, "a").disabled(true),
                    ListItem::new(1, "b"),
                ],
                [Constraint::Length(4)],
            )
            .header(["A"])
            .item_focus(|s: &State| s.cursor, on_cursor)
            .sort([0], |s| s.sort, Msg::Sort);
            ctx.component("table", table, Rect::new(0, 0, 8, 4));
        });
        assert_eq!(
            driver.event(key(KeyCode::Up), &state),
            EventResult::Emit(Msg::Header(0))
        );
    }

    /// The developer's sort key sorts the column the cursor is on, from the
    /// rows or the header, toggling as a click does; it outranks the table's
    /// own keys and bubbles where there is nothing to sort.
    #[test]
    fn the_sort_key_sorts_the_cursors_column() {
        let mut driver = focused_driver();
        let mut state = State {
            cursor: Some(TableCursor::Row(2)),
            column: 1,
            ..State::default()
        };
        let render = |driver: &mut Driver<State, Msg>, state: &State, chord: Option<KeyChord>| {
            driver.render(state, |ctx| {
                let table = table(state, false);
                let table = match chord {
                    Some(chord) => table.sort_key(chord),
                    None => table,
                };
                ctx.component("table", table, Rect::new(1, 1, 25, 8));
            });
        };
        let ascending = TableSort {
            column: 1,
            direction: SortDirection::Ascending,
        };
        render(&mut driver, &state, None);
        assert_eq!(
            driver.event(key(KeyCode::Char('s')), &state),
            EventResult::Ignored,
            "no sort key, no sorting: the letter is the app's"
        );
        render(&mut driver, &state, Some('s'.into()));
        assert_eq!(
            driver.event(key(KeyCode::Char('s')), &state),
            EventResult::Emit(Msg::Sort(ascending))
        );
        state.sort = Some(ascending);
        render(&mut driver, &state, Some('s'.into()));
        assert_eq!(
            driver.event(key(KeyCode::Char('s')), &state),
            EventResult::Emit(Msg::Sort(TableSort {
                column: 1,
                direction: SortDirection::Descending
            })),
            "again reverses"
        );
        state.sort = None;
        state.cursor = Some(TableCursor::Header);
        render(&mut driver, &state, Some('s'.into()));
        assert_eq!(
            driver.event(key(KeyCode::Char('s')), &state),
            EventResult::Emit(Msg::Sort(ascending)),
            "from the header too"
        );
        state.cursor = Some(TableCursor::Row(2));
        state.column = 0;
        render(&mut driver, &state, Some('s'.into()));
        assert_eq!(
            driver.event(key(KeyCode::Char('s')), &state),
            EventResult::Ignored,
            "column 0 does not sort, so the key bubbles"
        );
        state.column = 1;
        render(&mut driver, &state, Some('j'.into()));
        assert_eq!(
            driver.event(key(KeyCode::Char('j')), &state),
            EventResult::Emit(Msg::Sort(ascending)),
            "the developer's key outranks the table's own"
        );
        render(&mut driver, &state, Some(KeyChord::from('s').ctrl()));
        assert_eq!(
            driver.event(key(KeyCode::Char('s')), &state),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(
                key_with(
                    KeyCode::Char('s'),
                    crate::runtime::Modifiers {
                        ctrl: true,
                        ..crate::runtime::Modifiers::NONE
                    }
                ),
                &state
            ),
            EventResult::Emit(Msg::Sort(ascending))
        );
    }

    /// Without a column cursor, the sort key sorts the sorted column again.
    #[test]
    fn the_sort_key_without_a_column_cursor_sorts_the_sorted_column() {
        let mut driver = focused_driver();
        let ascending = TableSort {
            column: 1,
            direction: SortDirection::Ascending,
        };
        let descending = TableSort {
            column: 1,
            direction: SortDirection::Descending,
        };
        let state = State {
            cursor: Some(TableCursor::Row(0)),
            sort: Some(ascending),
            ..State::default()
        };
        // Unbound, or bound but reading no column yet: the same fallback.
        for bound in [false, true] {
            driver.render(&state, |ctx| {
                let table = Table::new(
                    [ListItem::new(0u8, "a")],
                    [Constraint::Length(4), Constraint::Length(4)],
                )
                .header(["A", "B"])
                .item_focus(|s: &State| s.cursor, on_cursor)
                .sort([0, 1], |s| s.sort, Msg::Sort)
                .sort_key('s');
                let table = if bound {
                    table.column_focus(|_| None, Msg::Column)
                } else {
                    table
                };
                ctx.component("table", table, Rect::new(0, 0, 16, 3));
            });
            assert_eq!(
                driver.event(key(KeyCode::Char('s')), &state),
                EventResult::Emit(Msg::Sort(descending)),
                "column_focus bound: {bound}"
            );
        }
    }

    /// A pointer resting or wiggling on a clipped column at the right edge
    /// neither moves the column nor scrolls the view; motion only reaches
    /// columns wholly on screen.
    #[test]
    fn pointer_motion_at_a_clipped_edge_does_not_pan() {
        let mut driver = wide_driver();
        let mut state = Wide {
            cursor: Some(TableCursor::Row(0)),
            column: 1,
            ..Wide::default()
        };
        draw(&mut driver, &state, 20, || wide_columns(false));
        let before = header(&driver);
        assert!(before.ends_with("c2    c"), "c3 is clipped: {before:?}");
        for x in [19, 18, 19, 18, 19] {
            let result = driver.event(mouse(MouseKind::Moved, x, 1), &state);
            assert!(!matches!(result, EventResult::Emit(_)), "x {x}: {result:?}");
            draw(&mut driver, &state, 20, || wide_columns(false));
        }
        assert_eq!(header(&driver), before, "the view never moved");
        assert_eq!(
            send(
                &mut driver,
                &mut state,
                mouse(MouseKind::Moved, 14, 1),
                false
            ),
            Some((2, 0)),
            "a whole column is reached, at the offset already shown"
        );
    }

    /// A primary press never emits from the table itself: it leaves focus and
    /// popup dismissal to the runtime, and only the click commits the row.
    #[test]
    fn a_press_in_the_cursor_row_leaves_down_to_the_runtime_and_dismisses_popups() {
        #[derive(Debug, PartialEq)]
        enum PopMsg {
            Focus(FocusState),
            Column(usize, usize),
            Select(u8),
            Dismiss,
        }
        let state = State {
            cursor: Some(TableCursor::Row(0)),
            ..State::default()
        };
        let mut driver = Driver::with(
            Ratcn::new().focus(|s: &State| &s.focus, PopMsg::Focus),
            30,
            8,
        );
        driver.render(&state, |ctx| {
            let table = Table::new(
                [ListItem::new(0u8, "a"), ListItem::new(1, "b")],
                [Constraint::Length(10), Constraint::Length(10)],
            )
            .item_focus(|s: &State| s.cursor, |_, _| PopMsg::Select(0))
            .selection(|s| s.selected, PopMsg::Select)
            .column_focus(|s| Some(s.column), PopMsg::Column);
            ctx.component("table", table, Rect::new(0, 0, 25, 4));
            ctx.popup(
                "menu",
                Rect::new(0, 5, 10, 2),
                crate::runtime::PopupOptions::default().on_dismiss(|| PopMsg::Dismiss),
                |_| {},
            );
        });
        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 15, 0), &state),
            EventResult::Emit(PopMsg::Dismiss),
            "pressing another cell on the focused table's cursor row still dismisses an open popup"
        );

        // Without a popup, press focuses an unfocused table and click selects.
        let mut driver = focused_driver();
        let state = State {
            focus: FocusState::intent(["next"]),
            cursor: Some(TableCursor::Row(0)),
            ..State::default()
        };
        frame(&mut driver, &state, 25, false);
        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 22, 3), &state),
            EventResult::Emit(Msg::Focus(FocusState::intent(["table"])))
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 22, 3), &state),
            EventResult::Emit(Msg::Select(0))
        );
    }

    /// Consecutive key moves before a redraw build on their own scroll offset,
    /// both vertically and horizontally.
    #[test]
    fn batched_key_moves_before_a_redraw_keep_their_own_offsets() {
        let mut driver = focused_driver();
        let mut state = State {
            cursor: Some(TableCursor::Row(19)),
            scroll: 17,
            ..State::default()
        };
        driver.render(&state, |ctx| {
            let table = Table::new(
                (0u8..20).map(|i| ListItem::new(i, i.to_string())),
                [Constraint::Length(4)],
            )
            .item_focus(|s: &State| s.cursor, on_cursor)
            .scroll(|s| s.scroll, Msg::Scroll);
            ctx.component("table", table, Rect::new(0, 0, 8, 3));
        });
        assert_eq!(
            driver.event(key(KeyCode::Home), &state),
            EventResult::Emit(Msg::Move(0, 0))
        );
        state.cursor = Some(TableCursor::Row(0));
        state.scroll = 0;
        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Move(1, 0)),
            "row 1 stays at offset 0 after Home"
        );

        let mut wide_drv = wide_driver();
        let mut wstate = Wide {
            cursor: Some(TableCursor::Row(0)),
            column: 4,
            scroll_x: 2,
            ..Wide::default()
        };
        draw(&mut wide_drv, &wstate, 20, || wide_columns(true));
        // Step left onto c1 (offset 0), then step right to c2 before redraw.
        for (k, expected) in [
            (KeyCode::Left, (3, 2)),
            (KeyCode::Left, (2, 1)),
            (KeyCode::Left, (1, 0)),
            (KeyCode::Right, (2, 0)),
        ] {
            assert_eq!(
                wide_drv.event(key(k), &wstate),
                EventResult::Emit(WideMsg::Column(expected.0, expected.1)),
                "after {k:?}"
            );
            wstate.column = expected.0;
            wstate.scroll_x = expected.1;
        }
    }

    /// A key that follows a wheel before the next frame builds on where the
    /// wheel left the view, not on the frame before it.
    #[test]
    fn a_key_after_an_unpainted_wheel_builds_on_the_wheeled_view() {
        let mut driver = focused_driver();
        let state = State {
            cursor: Some(TableCursor::Row(0)),
            ..State::default()
        };
        driver.render(&state, |ctx| {
            let table = Table::new(
                (0u8..20).map(|i| ListItem::new(i, i.to_string())),
                [Constraint::Length(4)],
            )
            .item_focus(|s: &State| s.cursor, on_cursor);
            ctx.component("table", table, Rect::new(0, 0, 8, 3));
        });
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 1, 1),
                &state
            ),
            EventResult::Consumed
        );
        // The wheeled view starts at 3; the cursor's next row is above it.
        assert_eq!(
            driver.event(key(KeyCode::Down), &state),
            EventResult::Emit(Msg::Move(1, 1))
        );
    }

    /// Multi-selection toggles rows via Enter, Space, and click, asks its
    /// predicate only for visible rows, and paints every selected row.
    #[test]
    fn multi_selection_toggles_rows_and_queries_only_visible_rows() {
        use std::cell::Cell;

        #[derive(Default)]
        struct MultiState {
            focus: FocusState,
            cursor: Option<TableCursor<u8>>,
            chosen: Vec<u8>,
        }
        #[derive(Debug, PartialEq)]
        enum MultiMsg {
            Focus(FocusState),
            Move(TableCursor<u8>),
            Toggle(u8),
        }
        let mut driver = Driver::with(
            Ratcn::new().focus(|s: &MultiState| &s.focus, MultiMsg::Focus),
            24,
            4,
        );
        let state = MultiState {
            cursor: Some(TableCursor::Row(0)),
            chosen: vec![0, 2],
            ..MultiState::default()
        };
        let probes = Rc::new(Cell::new(0usize));
        let draw_multi = |driver: &mut Driver<MultiState, MultiMsg>,
                          state: &MultiState,
                          probes: &Rc<Cell<usize>>| {
            driver.render(state, |ctx| {
                let probes = Rc::clone(probes);
                let table = Table::new(
                    (0u8..50).map(|i| ListItem::new(i, format!("row-{i}")).disabled(i == 1)),
                    [Constraint::Length(12)],
                )
                .paint_item(|_, row| {
                    let mark = if row.selected { "[x]" } else { "[ ]" };
                    Row::new([format!("{mark} {}", row.label)])
                })
                .item_focus(|s: &MultiState| s.cursor, |c, _| MultiMsg::Move(c))
                .multi_selection(
                    move |s, id| {
                        probes.set(probes.get() + 1);
                        s.chosen.contains(id)
                    },
                    MultiMsg::Toggle,
                );
                ctx.component("table", table, Rect::new(0, 0, 24, 3));
            });
        };

        draw_multi(&mut driver, &state, &probes);
        assert_eq!(driver.row(0).trim_end(), "[x] row-0");
        assert_eq!(driver.row(1).trim_end(), "[ ] row-1");
        assert_eq!(driver.row(2).trim_end(), "[x] row-2");
        assert!(
            probes.get() <= 4,
            "predicate is queried only for visible rows, got {} calls for 50 rows",
            probes.get()
        );
        let style = TableStyle::from_theme(&Theme::default_dark());
        assert_eq!(driver.cell(0, 0).bg, style.selected_focused_background);
        assert_eq!(driver.cell(0, 2).fg, style.selected_foreground);

        // Enter and Space toggle the cursor row; click toggles another enabled row.
        assert_eq!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Emit(MultiMsg::Toggle(0))
        );
        assert_eq!(
            driver.event(key(KeyCode::Char(' ')), &state),
            EventResult::Emit(MultiMsg::Toggle(0))
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 2, 2), &state),
            EventResult::Emit(MultiMsg::Toggle(2))
        );
        // Hovering an enabled selectable row asks for a hand cursor.
        driver.event(mouse(MouseKind::Moved, 2, 2), &state);
        draw_multi(&mut driver, &state, &probes);
        assert_eq!(driver.ratcn.pointer_shape(), PointerShape::Pointer);
        // Disabled row is skipped on click.
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 2, 1), &state),
            EventResult::Ignored
        );
    }

    #[test]
    #[should_panic(expected = "Table::selection(...) and Table::multi_selection(...)")]
    fn selection_and_multi_selection_together_panic() {
        let mut driver = focused_driver();
        let state = State::default();
        driver.render(&state, |ctx| {
            let table = Table::new([ListItem::new(0u8, "a")], [Constraint::Length(6)])
                .selection(|s: &State| s.selected, Msg::Select)
                .multi_selection(|s: &State, id| s.selected == Some(*id), Msg::Select);
            ctx.component("table", table, Rect::new(0, 0, 10, 3));
        });
    }
}
