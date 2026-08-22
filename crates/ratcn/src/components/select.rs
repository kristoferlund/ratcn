//! A trigger and popup panel for choosing one value.
//!
//! This module uses three related nouns consistently:
//!
//! - An **item** is the value-keyed [`ListItem`] shared with [`List`](crate::List),
//!   and what every index-addressed builder names. Item values provide stable
//!   identity across filtering and reordering.
//! - An **option** is one item as the user meets it: a choice in the panel.
//! - A **row** is terminal geometry: the one or more screen rows used to paint
//!   an option.
//!
//! For example, [`SelectWidget::visible_item_rows`] accepts the screen rows for
//! the items on screen, while [`Select::paint_item`] receives the shared item
//! state used by List.

use std::{fmt, rc::Rc};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Text,
    widgets::{Block, BorderType, Borders, Widget},
};

use crate::Theme;
use crate::color::{
    DISABLED_DIM, FIELD_FOCUS_SHIFT, FIELD_HOVER_SHIFT, ROW_FOCUS_SHIFT, away_from, blendable, dim,
};
use crate::linear_nav::{self, NavOutcome, ScrollStep};
use crate::list_core::{
    self, ListItem, ListItemState, RowIntent, RowViewport, SCROLL_STEP, WheelHold,
};
use crate::runtime::{
    Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, KeyEvent, MouseButton,
    MouseEvent, MouseKind, PaintCtx, PopupOptions, ScrollDirection,
};
use crate::selection_indicator;
use crate::theme::resolve_style;

/// The trigger height in terminal rows.
const TRIGGER_HEIGHT: u16 = 1;

/// How many options a Select shows before the panel scrolls.
const DEFAULT_MAX_VISIBLE_ITEMS: u16 = 8;

const INDICATOR_CLOSED: &str = "∨";
const INDICATOR_OPEN: &str = "∧";

/// Every color a select can paint.
///
/// A select has two parts, and the names keep them apart:
///
/// - **The trigger** — the one-row control showing the chosen value (or the
///   placeholder). Its fill responds to the control's interaction state:
///   `focused_trigger_background` while the select has focus,
///   `hovered_trigger_background` while it is hovered (hover wins over
///   focus), `trigger_background` at rest.
/// - **The panel** — the popup listing the options, filled with
///   `panel_background` inside a `border`. Option rows are then colored by
///   two independent facts, whether the cursor is on the option and whether
///   it is the chosen one, giving four combinations
///   (`option_foreground` → `focused_option_*` → `selected_*` →
///   `selected_focused_*`). Disabled overrides all of them, with
///   `selected_disabled_*` keeping a previously chosen option recognizable.
///
/// There is one cursor: keys and pointer hover move the same focused option,
/// and Enter commits it.
///
/// [`from_theme`](Self::from_theme) derives all of this from a [`Theme`];
/// build one by hand only for colors the theme cannot express, and pass it via
/// [`Select::style`] or [`SelectWidget::style`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectStyle {
    /// Text color of the chosen value on the trigger.
    pub value_foreground: Color,
    /// Text color of the placeholder shown while nothing is chosen.
    pub placeholder_foreground: Color,
    /// Trigger fill at rest.
    pub trigger_background: Color,
    /// Trigger fill while focused.
    pub focused_trigger_background: Color,
    /// Trigger fill while hovered.
    pub hovered_trigger_background: Color,
    /// Trigger indicator color.
    pub indicator: Color,
    /// Panel border color.
    pub border: Color,
    /// Panel fill.
    pub panel_background: Color,
    /// Ordinary option text color.
    pub option_foreground: Color,
    /// Cursor option text color.
    pub focused_option_foreground: Color,
    /// Cursor option fill.
    pub focused_option_background: Color,
    /// Chosen option text color.
    pub selected_foreground: Color,
    /// Chosen option text color while it is also the cursor option.
    pub selected_focused_foreground: Color,
    /// Chosen option fill while it is also the cursor option.
    pub selected_focused_background: Color,
    /// Chosen option marker color.
    pub selected_marker: Color,
    /// Unchosen option marker color.
    pub unselected_marker: Color,
    /// Disabled text and marker color.
    pub disabled_foreground: Color,
    /// Disabled trigger and option fill.
    pub disabled_background: Color,
    /// Chosen, disabled option text color.
    pub selected_disabled_foreground: Color,
    /// Chosen, disabled option fill.
    pub selected_disabled_background: Color,
}

impl SelectStyle {
    /// A neutral style using plain ANSI colors.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            value_foreground: Color::White,
            placeholder_foreground: Color::DarkGray,
            trigger_background: Color::Reset,
            focused_trigger_background: Color::Reset,
            hovered_trigger_background: Color::DarkGray,
            indicator: Color::DarkGray,
            border: Color::DarkGray,
            panel_background: Color::Reset,
            option_foreground: Color::Reset,
            focused_option_foreground: Color::Black,
            focused_option_background: Color::Cyan,
            selected_foreground: Color::White,
            selected_focused_foreground: Color::Black,
            selected_focused_background: Color::Cyan,
            selected_marker: Color::LightGreen,
            unselected_marker: Color::DarkGray,
            disabled_foreground: Color::DarkGray,
            disabled_background: Color::Reset,
            selected_disabled_foreground: Color::DarkGray,
            selected_disabled_background: Color::Reset,
        }
    }

    /// Derive every select color from `theme`.
    #[must_use]
    pub fn from_theme(theme: &Theme) -> Self {
        // Disabled dims toward the background, or the surface when the theme
        // leaves its background to the terminal — the same answer `ListStyle`
        // gives, so a select and a list cannot disagree about disabled.
        let backdrop = blendable(theme.background, theme.surface);
        let away = away_from(theme.background);
        let panel_background = dim(theme.field, away, FIELD_FOCUS_SHIFT);
        Self {
            value_foreground: theme.foreground,
            placeholder_foreground: theme.muted_foreground,
            trigger_background: theme.field,
            focused_trigger_background: panel_background,
            hovered_trigger_background: dim(theme.field, away, FIELD_HOVER_SHIFT),
            indicator: theme.muted_foreground,
            border: theme.border,
            panel_background,
            option_foreground: theme.muted_foreground,
            focused_option_foreground: theme.foreground,
            focused_option_background: dim(panel_background, away, ROW_FOCUS_SHIFT),
            selected_foreground: theme.foreground,
            selected_focused_foreground: theme.foreground,
            selected_focused_background: dim(panel_background, away, ROW_FOCUS_SHIFT),
            selected_marker: theme.primary,
            unselected_marker: theme.muted_foreground,
            disabled_foreground: dim(theme.muted_foreground, backdrop, DISABLED_DIM),
            disabled_background: dim(theme.field, backdrop, DISABLED_DIM),
            selected_disabled_foreground: dim(theme.muted_foreground, backdrop, DISABLED_DIM),
            selected_disabled_background: dim(theme.field, backdrop, DISABLED_DIM),
        }
    }

    const fn resolve_surface(self, focused: bool, hovered: bool, disabled: bool) -> Color {
        if disabled {
            self.disabled_background
        } else if hovered {
            self.hovered_trigger_background
        } else if focused {
            self.focused_trigger_background
        } else {
            self.trigger_background
        }
    }

    const fn resolve_row(self, focused: bool, selected: bool, disabled: bool) -> Style {
        let (foreground, background) = if selected && disabled {
            (
                self.selected_disabled_foreground,
                self.selected_disabled_background,
            )
        } else if disabled {
            (self.disabled_foreground, self.disabled_background)
        } else if selected && focused {
            (
                self.selected_focused_foreground,
                self.selected_focused_background,
            )
        } else if focused {
            (
                self.focused_option_foreground,
                self.focused_option_background,
            )
        } else if selected {
            (self.selected_foreground, self.panel_background)
        } else {
            (self.option_foreground, self.panel_background)
        };
        Style::new().fg(foreground).bg(background)
    }
}

/// A select that only draws, with no focus, events, or app state.
///
/// **Usable in any ratatui app.** Nothing here depends on
/// [`Ratcn`](crate::runtime::Ratcn) or the component layer: render it directly
/// and keep driving the open state, cursor, and selection however you already
/// do.
///
/// It paints a one-row trigger and, when opened, a bordered panel immediately
/// below it within the supplied area. The interactive [`Select`] paints the
/// same parts separately so its panel can live in a popup layer.
#[expect(clippy::struct_excessive_bools, reason = "independent paint states")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectWidget<'a> {
    value: Option<&'a str>,
    placeholder: &'a str,
    open: bool,
    options: &'a [&'a str],
    item_rows: Option<&'a [Text<'static>]>,
    row_height: u16,
    focused_item: Option<usize>,
    selected_item: Option<usize>,
    disabled_items: &'a [bool],
    first_item: usize,
    focused: bool,
    hovered: bool,
    disabled: bool,
    style: SelectStyle,
}

impl<'a> SelectWidget<'a> {
    /// Construct a closed select showing `value`, or an empty placeholder.
    #[must_use]
    pub const fn new(value: Option<&'a str>) -> Self {
        Self {
            value,
            placeholder: "",
            open: false,
            options: &[],
            item_rows: None,
            row_height: 1,
            focused_item: None,
            selected_item: None,
            disabled_items: &[],
            first_item: 0,
            focused: false,
            hovered: false,
            disabled: false,
            style: SelectStyle::fallback(),
        }
    }

    /// Take colors from `theme`.
    #[must_use]
    pub fn themed(mut self, theme: &Theme) -> Self {
        self.style = SelectStyle::from_theme(theme);
        self
    }

    /// Use these exact colors.
    #[must_use]
    pub const fn style(mut self, style: SelectStyle) -> Self {
        self.style = style;
        self
    }

    /// Set the text shown while no value is chosen.
    #[must_use]
    pub const fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Open the panel and show `options`.
    #[must_use]
    pub const fn open(mut self, options: &'a [&'a str]) -> Self {
        self.open = true;
        self.options = options;
        self
    }

    /// Pre-rendered screen rows to paint instead of the default
    /// marker-and-label line — one per *painted* option, in paint order,
    /// starting at [`first_item`](Self::first_item).
    ///
    /// [`open`](Self::open) still takes every option, because the panel's
    /// height is measured from their count; these are only the rows that
    /// appear, so a long option list costs a long list's worth of [`Text`] only
    /// if you build one.
    ///
    /// Each `Text` may span several lines; pair this with
    /// [`row_height`](Self::row_height) so every option occupies the same
    /// number of rows. The [`SelectStyle`] option-state colors are painted
    /// beneath the supplied text, so unstyled text inherits them while
    /// explicit colors remain intact.
    #[must_use]
    pub const fn visible_item_rows(mut self, item_rows: &'a [Text<'static>]) -> Self {
        self.item_rows = Some(item_rows);
        self
    }

    /// How many terminal rows each option occupies. Defaults to 1; 0 is
    /// treated as 1.
    ///
    /// Every option gets the same height, which is what keeps the panel's
    /// height math and a caller's hit-testing exact.
    #[must_use]
    pub const fn row_height(mut self, rows: u16) -> Self {
        self.row_height = if rows == 0 { 1 } else { rows };
        self
    }

    /// Set the cursor option by index.
    #[must_use]
    pub const fn focused_item(mut self, focused: Option<usize>) -> Self {
        self.focused_item = focused;
        self
    }

    /// Set the chosen option by index.
    #[must_use]
    pub const fn selected_item(mut self, selected: Option<usize>) -> Self {
        self.selected_item = selected;
        self
    }

    /// Set the disabled mask, positionally matched to the options.
    #[must_use]
    pub const fn disabled_items(mut self, disabled: &'a [bool]) -> Self {
        self.disabled_items = disabled;
        self
    }

    /// Set the index of the first visible item.
    #[must_use]
    pub const fn first_item(mut self, first: usize) -> Self {
        self.first_item = first;
        self
    }

    /// Paint the focused trigger state.
    #[must_use]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Paint the hovered trigger state.
    #[must_use]
    pub const fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Paint the disabled state.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Total height this widget would paint inside `area_height`, capped at
    /// `max_visible` options.
    ///
    /// A closed or disabled select occupies one row. An enabled, open one adds
    /// a two-row panel border and as many options as fit, each
    /// [`row_height`](Self::row_height) rows tall. Openness, option count, and
    /// row height are read from the instance, so build the widget first and
    /// measure the same value that will paint.
    #[must_use]
    pub const fn height(&self, max_visible: u16, area_height: u16) -> u16 {
        let visible = self.visible_items(max_visible, area_height);
        if area_height == 0 {
            0
        } else if visible == 0 {
            TRIGGER_HEIGHT
        } else {
            TRIGGER_HEIGHT + visible * self.row_height + 2
        }
    }

    /// Number of whole options painted below the trigger inside `area_height`,
    /// capped at `max_visible`. Zero while closed or disabled. Counts options,
    /// not rows — multiply by [`row_height`](Self::row_height) for rows.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the option count is clamped before conversion"
    )]
    pub const fn visible_items(&self, max_visible: u16, area_height: u16) -> u16 {
        if !self.open || self.disabled {
            return 0;
        }
        let count = if self.options.len() > u16::MAX as usize {
            u16::MAX
        } else {
            self.options.len() as u16
        };
        min_u16(
            count,
            min_u16(
                max_visible,
                area_height.saturating_sub(TRIGGER_HEIGHT + 2) / self.row_height,
            ),
        )
    }
}

const fn min_u16(left: u16, right: u16) -> u16 {
    if left < right { left } else { right }
}

impl Widget for SelectWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        paint_trigger(self, area, buf);
        // The standalone widget reserves one trigger row and two border rows.
        let visible = self.visible_items(u16::MAX, area.height);
        if self.open && !self.disabled && visible > 0 {
            let panel = Rect::new(
                area.x,
                area.y + 1,
                area.width,
                visible * self.row_height + 2,
            );
            paint_panel(self, panel, buf);
        }
    }
}

fn paint_trigger(widget: SelectWidget<'_>, area: Rect, buf: &mut Buffer) {
    let area = Rect { height: 1, ..area }.intersection(buf.area);
    let background = widget
        .style
        .resolve_surface(widget.focused, widget.hovered, widget.disabled);
    let (text, foreground) = if widget.disabled {
        (
            widget.value.unwrap_or(widget.placeholder),
            widget.style.disabled_foreground,
        )
    } else {
        widget.value.map_or(
            (widget.placeholder, widget.style.placeholder_foreground),
            |value| (value, widget.style.value_foreground),
        )
    };
    buf.set_style(area, Style::new().bg(background));
    buf.set_stringn(
        area.x.saturating_add(1),
        area.y,
        text,
        usize::from(area.width.saturating_sub(4)),
        Style::new().fg(foreground),
    );
    if area.width >= 2 {
        let indicator = if widget.open && !widget.disabled {
            INDICATOR_OPEN
        } else {
            INDICATOR_CLOSED
        };
        let color = if widget.disabled {
            widget.style.disabled_foreground
        } else {
            widget.style.indicator
        };
        buf.set_string(area.right() - 2, area.y, indicator, Style::new().fg(color));
    }
}

fn paint_panel(widget: SelectWidget<'_>, area: Rect, buf: &mut Buffer) {
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(
            Style::new()
                .fg(widget.style.border)
                .bg(widget.style.panel_background),
        )
        .style(Style::new().bg(widget.style.panel_background));
    let inner = block.inner(area);
    block.render(area, buf);
    let row_height = widget.row_height.max(1);
    for (row, index) in (widget.first_item..widget.options.len())
        .take(usize::from(inner.height / row_height))
        .enumerate()
    {
        let disabled = widget.disabled_items.get(index).copied().unwrap_or(false);
        let selected = widget.selected_item == Some(index);
        let row_area = Rect::new(
            inner.x,
            inner.y + u16::try_from(row).expect("visible rows fit in u16") * row_height,
            inner.width,
            row_height,
        );
        buf.set_style(
            row_area,
            widget
                .style
                .resolve_row(widget.focused_item == Some(index), selected, disabled),
        );
        if let Some(rows) = widget.item_rows {
            if let Some(text) = rows.get(row) {
                text.render(row_area, buf);
            }
            continue;
        }
        selection_indicator::marker_line(
            widget.options[index],
            selected,
            false,
            disabled,
            selection_indicator::MarkerColors {
                disabled: widget.style.disabled_foreground,
                selected: widget.style.selected_marker,
                unselected: widget.style.unselected_marker,
            },
        )
        .render(row_area, buf);
    }
}

struct SelectPanelWidget<'a>(SelectWidget<'a>);

impl Widget for SelectPanelWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        paint_panel(self.0, area, buf);
    }
}

type ReadFn<S, T> = Rc<dyn Fn(&S) -> Option<T>>;
type ReadOpenFn<S> = Rc<dyn Fn(&S) -> bool>;
type OnOpenChangeFn<M> = Rc<dyn Fn(bool) -> M>;
type OpenBinding<S, M> = (ReadOpenFn<S>, OnOpenChangeFn<M>);
type OnChangeFn<T, M> = Rc<dyn Fn(T) -> M>;
type PaintItemFn<S, T> = Rc<dyn for<'a> Fn(&S, ListItemState<'a, T>) -> Text<'static>>;
type StyleFn = Rc<dyn Fn(&Theme) -> SelectStyle>;

fn bound_index<T: PartialEq, S>(
    items: &[ListItem<T>],
    state: &S,
    read: Option<&ReadFn<S, T>>,
) -> Option<usize> {
    list_core::index_of(items, &(read?)(state)?)
}

fn resolved_cursor_index<T: PartialEq, S>(
    items: &[ListItem<T>],
    state: &S,
    focused: Option<&ReadFn<S, T>>,
    selected: Option<&ReadFn<S, T>>,
) -> Option<usize> {
    // A bound value is never second-guessed: a cursor resting on a disabled
    // option stays there and is painted there, exactly as in `List`, and the
    // commit path refuses it. Only an absent value falls back.
    bound_index(items, state, focused)
        .or_else(|| bound_index(items, state, selected))
        .or_else(|| linear_nav::first_enabled(items.len(), |i| list_core::disabled_at(items, i)))
}

fn emit_item<T: Clone, M>(
    handler: Option<&OnChangeFn<T, M>>,
    items: &[ListItem<T>],
    index: usize,
) -> EventResult<M> {
    handler.map_or(EventResult::Ignored, |handler| {
        EventResult::Emit(handler(items[index].value().clone()))
    })
}

/// A one-row select trigger with an option panel declared as a popup layer.
///
/// The panel is a child popup anchored at the Select's identity. It paints over
/// the trigger with its top border one row above the trigger, so the first
/// option covers the trigger row. Near a frame edge it shifts just far enough
/// to remain visible. Moving the option cursor never moves the popup itself.
/// The same placement works when the Select is inside a modal.
///
/// Vocabulary: the panel *closes* on Esc, Tab-out, or a trigger click, and is
/// *dismissed* by a primary-button press outside it (the popup layer's dismiss
/// gesture, which emits the close message without consuming the press). All of
/// these arrive through the [`open`](Self::open) binding as
/// `on_open_change(false)`.
///
/// Open state, item focus (the option cursor), and selection (the committed
/// choice) are app-owned controlled bindings. Each option is backed by a
/// value-keyed [`ListItem`]; its item value is the stable identity shared with
/// [`List`](crate::List). The update handling a selection should store the value
/// as both item focus and selection and close the panel; one message then keeps
/// all three values synchronized between redraws.
///
/// Keyboard operation requires [`open`](Self::open),
/// [`item_focus`](Self::item_focus), and [`selection`](Self::selection). Partial
/// binding combinations remain valid for paint-only or pointer-only use, but do
/// not make the Select a keyboard focus stop or consume keyboard input.
///
/// Item values must be unique. Enter, Space, Up, or Down opens a closed Select.
/// While open, navigation moves item focus, Enter or Space selects, Esc closes,
/// and the first Tab or `BackTab` closes and consumes that traversal key. A
/// following Tab can move focus after the app applies the close message. The
/// wheel scrolls the panel and leaves item focus where it is, so the cursor
/// may scroll out of sight until the cursor or the options move — the same
/// wheel behavior as [`List`](crate::List), held under the rule
/// [`WheelHold`](crate::list_core::WheelHold) states.
/// Other modified keys are ignored so app shortcuts can handle
/// them after they bubble through the popup. Paste events bubble for the same
/// reason; Select has no text-editing behavior.
#[expect(
    clippy::struct_field_names,
    reason = "on_select matches the public selection binding vocabulary"
)]
pub struct Select<T, S, M> {
    items: Rc<[ListItem<T>]>,
    placeholder: String,
    open: Option<OpenBinding<S, M>>,
    focused_item: Option<ReadFn<S, T>>,
    on_focus_change: Option<OnChangeFn<T, M>>,
    selected: Option<ReadFn<S, T>>,
    on_select: Option<OnChangeFn<T, M>>,
    max_visible: u16,
    disabled: bool,
    paint_item: Option<PaintItemFn<S, T>>,
    row_height: u16,
    style: Option<StyleFn>,
    resolved_open: bool,
    page_size: usize,
}

impl<T: fmt::Debug, S, M> fmt::Debug for Select<T, S, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Select")
            .field("items", &self.items)
            .field("open", &self.open.is_some())
            .field("item_focus", &self.focused_item.is_some())
            .field("selection", &self.selected.is_some())
            .field("disabled", &self.disabled)
            .finish_non_exhaustive()
    }
}

impl<T, S, M> Select<T, S, M> {
    /// Construct a Select from value-keyed options.
    ///
    /// Accepts anything convertible to [`ListItem`], including plain strings.
    /// Values must be unique within this declaration.
    #[must_use]
    pub fn new(items: impl IntoIterator<Item = impl Into<ListItem<T>>>) -> Self {
        Self {
            items: items.into_iter().map(Into::into).collect(),
            placeholder: String::new(),
            open: None,
            focused_item: None,
            on_focus_change: None,
            selected: None,
            on_select: None,
            max_visible: DEFAULT_MAX_VISIBLE_ITEMS,
            disabled: false,
            paint_item: None,
            row_height: 1,
            style: None,
            resolved_open: false,
            page_size: 1,
        }
    }

    /// Set the trigger placeholder.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Bind the panel's open state and the message that changes it.
    ///
    /// `read` runs against current app state during rendering and event
    /// handling. `on_open_change` receives each requested state — the open flag
    /// is a continuously tracked value, like a cursor, not a one-shot commit:
    /// `true` when the user opens the trigger, `false` when the panel should
    /// close. Without this binding the Select is not focusable and cannot open.
    ///
    /// This binding is also the component's dismiss channel: Esc, Tab-out, a
    /// trigger click while open, and a press outside the panel all arrive as
    /// `on_open_change(false)`. It plays the role
    /// [`Dialog::on_dismiss`](crate::Dialog::on_dismiss) and
    /// [`PopupOptions::on_dismiss`] play elsewhere — one message closes the
    /// panel no matter which gesture asked for it.
    #[must_use]
    pub fn open(
        mut self,
        read: impl Fn(&S) -> bool + 'static,
        on_open_change: impl Fn(bool) -> M + 'static,
    ) -> Self {
        self.open = Some((Rc::new(read), Rc::new(on_open_change)));
        self
    }

    /// Bind the option cursor and the message that moves it.
    ///
    /// `read` returns the focused option value. When it returns `None`, an open
    /// panel starts from the selected value or the first enabled option.
    /// `on_change` receives each value reached by keyboard or pointer movement;
    /// moving the cursor does not commit a selection.
    ///
    /// Unlike [`List::item_focus`](crate::List::item_focus), `on_change`
    /// carries no scroll-offset payload: the panel owns its scroll and keeps
    /// the cursor visible itself, so there is no app-held offset to keep in
    /// sync.
    #[must_use]
    pub fn item_focus(
        mut self,
        read: impl Fn(&S) -> Option<T> + 'static,
        on_change: impl Fn(T) -> M + 'static,
    ) -> Self {
        self.focused_item = Some(Rc::new(read));
        self.on_focus_change = Some(Rc::new(on_change));
        self
    }

    /// Bind the committed value and the message that selects it.
    ///
    /// `read` returns the value shown on the trigger. `on_select` receives the
    /// option committed by Enter, Space, or a primary-button click. Its update
    /// should also align item focus and close the panel so the complete change
    /// is atomic.
    #[must_use]
    pub fn selection(
        mut self,
        read: impl Fn(&S) -> Option<T> + 'static,
        on_select: impl Fn(T) -> M + 'static,
    ) -> Self {
        self.selected = Some(Rc::new(read));
        self.on_select = Some(Rc::new(on_select));
        self
    }

    /// Set the maximum visible option count. Additional options scroll.
    ///
    /// Defaults to eight. Zero is treated as one.
    #[must_use]
    pub const fn max_visible_items(mut self, max_visible: u16) -> Self {
        self.max_visible = if max_visible == 0 { 1 } else { max_visible };
        self
    }

    /// Draw each option yourself instead of using the default marker-and-label
    /// line.
    ///
    /// The closure gets app state and a [`ListItemState`] describing the
    /// option, and returns what to paint — the same contract as
    /// [`List::paint_item`](crate::List::paint_item). Use it for columns,
    /// secondary text, per-option icons — anything the default cannot express.
    /// The resolved [`SelectStyle`] is painted beneath the returned text, so
    /// unstyled text inherits option-state colors while explicit `Text`, `Line`,
    /// and `Span` colors remain intact.
    ///
    /// Return a [`Line`](ratatui::text::Line) for the usual one-row option, or a [`Text`] for a
    /// taller one — a name above a subtitle, say. A multi-line option also
    /// needs [`row_height`](Self::row_height) set to match, since every option
    /// must be the same height for clicks to land on the right one.
    #[must_use]
    pub fn paint_item<R: Into<Text<'static>>>(
        mut self,
        f: impl for<'a> Fn(&S, ListItemState<'a, T>) -> R + 'static,
    ) -> Self {
        self.paint_item = Some(Rc::new(move |state, row| f(state, row).into()));
        self
    }

    /// How many terminal rows each option occupies. Defaults to 1.
    ///
    /// Raise it when [`paint_item`](Self::paint_item) returns more than one
    /// line. Every option gets the same height, which is what keeps clicking,
    /// paging, and scrolling exact: a returned [`Text`] is padded with blank
    /// lines or truncated to fit, so the option the user clicks is always the
    /// one the runtime thinks it is. The panel's height grows accordingly:
    /// each visible option costs this many rows inside the panel border.
    ///
    /// A height of 0 is treated as 1.
    #[must_use]
    pub const fn row_height(mut self, rows: u16) -> Self {
        self.row_height = if rows == 0 { 1 } else { rows };
        self
    }

    /// Replace the theme-derived style.
    ///
    /// The closure receives the active theme each render, so its result follows
    /// runtime theme changes.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> SelectStyle + 'static) -> Self {
        self.style = Some(Rc::new(style));
        self
    }

    /// Dim the whole Select and disable all interaction.
    ///
    /// A disabled Select, an empty Select, or one whose options are all disabled
    /// is excluded from focus traversal. Disable one option with
    /// [`ListItem::disabled`].
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// The one row of `area` the trigger occupies.
///
/// A Select is one row tall whether it is open or closed — the panel floats in a
/// popup layer — so declaration, paint, and hit-testing all read the same top
/// row of whatever area the layout gave, and the rows below belong to whatever
/// else is declared there. Not [`fixed_height`](crate::geometry::fixed_height):
/// this crops to at most a row rather than requiring one, so a zero-height
/// allocation stays a zero-height trigger instead of becoming an empty rect at
/// the origin.
const fn trigger_area(area: Rect) -> Rect {
    Rect {
        height: min_u16(area.height, TRIGGER_HEIGHT),
        ..area
    }
}

/// Tab or `BackTab`: the keys that move focus out of the Select entirely. Ctrl
/// and Alt variants belong to the app, so they are not traversal here.
fn is_traversal(key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Tab => !key.modifiers.any(),
        KeyCode::BackTab => !key.modifiers.ctrl && !key.modifiers.alt,
        _ => false,
    }
}

impl<T: Clone + PartialEq, S, M> Select<T, S, M> {
    fn disabled_at(&self, index: usize) -> bool {
        list_core::disabled_at(&self.items, index)
    }

    fn selected_index(&self, state: &S) -> Option<usize> {
        bound_index(&self.items, state, self.selected.as_ref())
    }

    fn cursor_index(&self, state: &S) -> Option<usize> {
        resolved_cursor_index(
            &self.items,
            state,
            self.focused_item.as_ref(),
            self.selected.as_ref(),
        )
    }

    fn is_open(&self, state: &S) -> bool {
        self.open.as_ref().is_some_and(|(read, _)| read(state))
    }

    fn toggle(&self, open: bool) -> EventResult<M> {
        self.open
            .as_ref()
            .map_or(EventResult::Ignored, |(_, toggle)| {
                EventResult::Emit(toggle(open))
            })
    }

    fn move_cursor(&self, index: usize) -> EventResult<M> {
        emit_item(self.on_focus_change.as_ref(), &self.items, index)
    }

    fn select(&self, index: usize) -> EventResult<M> {
        emit_item(self.on_select.as_ref(), &self.items, index)
    }

    fn keyboard_enabled(&self) -> bool {
        self.open.is_some() && self.focused_item.is_some() && self.selected.is_some()
    }

    /// Is there any option a cursor could land on? A panel of nothing but
    /// disabled options is not worth opening and cannot be navigated.
    fn has_enabled_item(&self) -> bool {
        linear_nav::has_enabled(self.items.len(), |i| self.disabled_at(i))
    }

    /// Route a key to whichever of the two key maps is in force. A closed
    /// Select and an open one answer to almost disjoint sets of keys, so they
    /// are separate functions rather than one sequence of `open` tests.
    ///
    /// Only the two checks here span both, and their order is load-bearing.
    /// Traversal comes first because an open panel must not travel with the
    /// focus that is leaving — and it has to precede the modifier rejection,
    /// since `BackTab` carries Shift and would be rejected by it.
    fn handle_key(&self, key: KeyEvent, state: &S, page_size: usize) -> EventResult<M> {
        if !self.keyboard_enabled() {
            return EventResult::Ignored;
        }
        let open = self.is_open(state);
        if open && is_traversal(key) {
            return self.toggle(false);
        }
        if open {
            self.handle_open_key(key, state, page_size)
        } else {
            self.handle_closed_key(key)
        }
    }

    /// The keys a closed Select answers: the ones that open it, and nothing
    /// else. Everything unrecognized bubbles so the app keeps its hotkeys.
    fn handle_closed_key(&self, key: KeyEvent) -> EventResult<M> {
        if !self.has_enabled_item() {
            return EventResult::Ignored;
        }
        // A key that would move the cursor reveals the cursor instead. Asking
        // the shared key map keeps that true of the Ctrl chords as well as the
        // arrows.
        if linear_nav::is_step_key(key) {
            return self.toggle(true);
        }
        if linear_nav::has_reserved_modifier(key) {
            return EventResult::Ignored;
        }
        match key.code {
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle(true),
            _ => EventResult::Ignored,
        }
    }

    /// The keys an open panel answers: dismiss, navigation, commit.
    fn handle_open_key(&self, key: KeyEvent, state: &S, page_size: usize) -> EventResult<M> {
        // Closing comes before the emptiness check: a panel with nothing to
        // operate must still be dismissable.
        if key.code == KeyCode::Esc && !linear_nav::has_reserved_modifier(key) {
            return self.toggle(false);
        }
        if !self.has_enabled_item() {
            return EventResult::Ignored;
        }
        let cursor = self.cursor_index(state);
        // Navigation is asked first because it owns the Ctrl chords that the
        // modifier gate below rejects.
        if let Some(outcome) =
            linear_nav::nav_key_target(key, self.items.len(), cursor, page_size.max(1), |i| {
                self.disabled_at(i)
            })
        {
            return match outcome {
                NavOutcome::Move(index) => self.move_cursor(index),
                NavOutcome::Stay => EventResult::Consumed,
            };
        }
        if linear_nav::has_reserved_modifier(key) {
            return EventResult::Ignored;
        }
        if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
            // A cursor resting on a disabled option commits nothing, as in
            // `List`, and the key bubbles rather than vanishing.
            return match cursor {
                Some(index) if !self.disabled_at(index) => self.select(index),
                _ => EventResult::Ignored,
            };
        }
        // Anything else bubbles through to the app: the popup is not modal, so
        // an unhandled key is not the panel's to swallow.
        EventResult::Ignored
    }
}

impl<T: Clone + PartialEq + 'static, S: 'static, M: 'static> Component<S, M> for Select<T, S, M> {
    fn prepare(&mut self, state: &S) {
        // Quadratic in the option count and re-derived on every frame's fresh
        // instance, so a release build takes the items on trust.
        if cfg!(debug_assertions) {
            list_core::assert_unique_values(self.items.iter().map(ListItem::value), "Select");
        }
        self.resolved_open = !self.disabled && self.is_open(state);
    }

    fn declare(&mut self, ctx: &mut DeclareCtx<'_, S, M>) {
        let area = trigger_area(ctx.area());
        let style = resolve_style(self.style.as_deref(), ctx.theme, SelectStyle::from_theme);
        let Some((panel_area, viewport)) = self
            .resolved_open
            .then(|| {
                panel_layout(
                    area,
                    ctx.frame_area(),
                    self.items.len(),
                    self.max_visible,
                    self.row_height,
                )
            })
            .flatten()
        else {
            return;
        };
        let inner = Block::new().borders(Borders::ALL).inner(panel_area);
        self.page_size = viewport.visible_items(inner).max(1);
        let Some((open, on_open_change)) = &self.open else {
            return;
        };
        let panel = SelectPanel {
            items: Rc::clone(&self.items),
            open: Rc::clone(open),
            focused_item: self.focused_item.clone(),
            on_focus_change: self.on_focus_change.clone(),
            selected: self.selected.clone(),
            on_select: self.on_select.clone(),
            paint_item: self.paint_item.clone(),
            style,
            panel_area,
            inner,
            viewport,
        };
        let on_open_change = Rc::clone(on_open_change);
        ctx.popup(
            "panel",
            panel_area,
            PopupOptions::default().on_dismiss(move || on_open_change(false)),
            move |ctx| ctx.component("options", panel, panel_area),
        );
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, S>) {
        let area = trigger_area(ctx.area());
        let state = ctx.state();
        let style = resolve_style(self.style.as_deref(), ctx.theme, SelectStyle::from_theme);
        let selected = self.selected_index(state);
        let value = selected.map(|index| self.items[index].label());
        let labels: Vec<&str> = self.items.iter().map(ListItem::label).collect();
        let mut trigger = SelectWidget::new(value)
            .placeholder(&self.placeholder)
            .focused(ctx.focused)
            .hovered(ctx.hovered)
            .disabled(self.disabled)
            .style(style);
        if self.resolved_open {
            trigger = trigger.open(&labels);
        }
        ctx.widget(trigger, area);
    }

    fn handle_event(
        &mut self,
        event: &Event,
        state: &S,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<M> {
        if self.disabled || self.items.is_empty() {
            return EventResult::Ignored;
        }
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseKind::Click(MouseButton::Left) => self.toggle(!self.is_open(state)),
                _ => EventResult::Ignored,
            },
            Event::Key(key) => self.handle_key(*key, state, self.page_size),
            _ => EventResult::Ignored,
        }
    }

    fn is_focusable(&self) -> bool {
        self.keyboard_enabled() && !self.disabled && self.has_enabled_item()
    }

    fn interaction_area(&self, area: Rect) -> Rect {
        trigger_area(area)
    }
}

struct SelectPanel<T, S, M> {
    items: Rc<[ListItem<T>]>,
    open: ReadOpenFn<S>,
    focused_item: Option<ReadFn<S, T>>,
    on_focus_change: Option<OnChangeFn<T, M>>,
    selected: Option<ReadFn<S, T>>,
    on_select: Option<OnChangeFn<T, M>>,
    paint_item: Option<PaintItemFn<S, T>>,
    style: SelectStyle,
    panel_area: Rect,
    /// `panel_area` inside its border: the rows the options themselves occupy.
    /// Carried rather than re-derived so declaration, paint, hit-testing, and
    /// wheel arithmetic all measure the panel's capacity against one rect.
    inner: Rect,
    viewport: RowViewport,
}

impl<T: Clone + PartialEq, S, M> SelectPanel<T, S, M> {
    fn cursor(&self, state: &S) -> Option<usize> {
        resolved_cursor_index(
            &self.items,
            state,
            self.focused_item.as_ref(),
            self.selected.as_ref(),
        )
    }

    fn option_at(&self, mouse: &MouseEvent) -> Option<usize> {
        self.viewport
            .row_at(self.inner, self.items.len(), mouse.column, mouse.row)
    }
}

impl<T: Clone + PartialEq + 'static, S, M> Component<S, M> for SelectPanel<T, S, M> {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, S, M>) {
        let state = ctx.state();
        let cursor = self.cursor(state);
        // The panel owns its scrolling, so the hold supplies the offset: the
        // view stays where the wheel left it — cursor visible or not — until
        // the cursor or the options move, and the hold dies with the popup.
        WheelHold::settle_transient(
            ctx,
            &self.items,
            cursor,
            None,
            &mut self.viewport,
            self.inner,
        );
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, S>) {
        let state = ctx.state();
        let labels: Vec<&str> = self.items.iter().map(ListItem::label).collect();
        let disabled: Vec<bool> = self.items.iter().map(ListItem::is_disabled).collect();
        let cursor = self.cursor(state);
        let selected = bound_index(&self.items, state, self.selected.as_ref());
        let rows_per_item = self.viewport.rows_per_item();
        // Only the options on screen are built, however many the select has.
        // The panel is laid out to a whole number of rows, so there is no
        // clipped trailing option to account for. Indices stay global, as a
        // custom `paint_item` and the panel's hit-testing both read them.
        let first_option = self.viewport.painted_offset().min(self.items.len());
        let last_option = first_option
            .saturating_add(self.viewport.visible_items(self.inner))
            .min(self.items.len());
        let rows: Option<Vec<Text<'static>>> = self.paint_item.as_ref().map(|paint_item| {
            list_core::windowed_rows(
                &self.items,
                first_option..last_option,
                rows_per_item,
                |index, item| {
                    let row = ListItemState {
                        index,
                        value: item.value(),
                        label: item.label(),
                        focused: cursor == Some(index),
                        selected: selected == Some(index),
                        disabled: item.is_disabled(),
                    };
                    paint_item(state, row)
                },
            )
        });
        let mut widget = SelectWidget::new(None)
            .open(&labels)
            .row_height(rows_per_item)
            .focused_item(cursor)
            .selected_item(selected)
            .disabled_items(&disabled)
            .first_item(self.viewport.painted_offset())
            .style(self.style);
        if let Some(rows) = &rows {
            widget = widget.visible_item_rows(rows);
        }
        ctx.widget(SelectPanelWidget(widget), self.panel_area);
    }

    fn handle_event(&mut self, event: &Event, state: &S, ctx: &mut EventCtx<'_>) -> EventResult<M> {
        if !(self.open)(state) {
            return EventResult::Ignored;
        }
        let Event::Mouse(mouse) = event else {
            return EventResult::Ignored;
        };
        let cursor = self.cursor(state);
        let option = self.option_at(mouse);
        match mouse.kind {
            MouseKind::Scroll(direction) => {
                let step = match direction {
                    ScrollDirection::Up => ScrollStep::Up,
                    ScrollDirection::Down => ScrollStep::Down,
                    ScrollDirection::Left | ScrollDirection::Right => {
                        return EventResult::Ignored;
                    }
                };
                let offset = linear_nav::wheel_offset(
                    self.items.len(),
                    self.viewport.visible_items(self.inner),
                    self.viewport.painted_offset(),
                    step,
                    SCROLL_STEP,
                );
                // Hold the view in the transient store against the options as
                // they stand: render honors it until the cursor or the options
                // move, and it survives redraws because it lives on the panel's
                // identity, not on this per-frame instance. The cursor itself
                // never moves on wheel.
                ctx.transient::<WheelHold<T>>()
                    .hold(offset, &self.items, cursor);
                // Keep this retained instance's hit-testing aligned with the
                // offset the next paint will use.
                self.viewport.record_painted_offset(offset);
                EventResult::Consumed
            }
            kind => match list_core::row_intent(
                kind,
                &self.items,
                option,
                cursor,
                self.on_select.is_some(),
            ) {
                RowIntent::BlockPress => EventResult::Consumed,
                RowIntent::Commit(index) => emit_item(self.on_select.as_ref(), &self.items, index),
                // Motion means nothing without a cursor to move: a panel bound
                // for pointer selection alone lets it through to whatever is
                // under it, rather than swallowing every drift over an option.
                RowIntent::Focus(index) if self.focused_item.is_some() => {
                    emit_item(self.on_focus_change.as_ref(), &self.items, index)
                }
                RowIntent::Stay if self.focused_item.is_some() => EventResult::Consumed,
                // The popup occludes exactly its own footprint, so a click
                // inside it that landed on no option is swallowed here rather
                // than reaching the control the panel covers.
                RowIntent::Bubble
                    if option.is_none() && kind == MouseKind::Click(MouseButton::Left) =>
                {
                    EventResult::Consumed
                }
                RowIntent::Focus(_) | RowIntent::Stay | RowIntent::Bubble => EventResult::Ignored,
            },
        }
    }
}

fn visible_count(len: usize, max_visible: u16, available: u16) -> u16 {
    u16::try_from(len)
        .unwrap_or(u16::MAX)
        .min(max_visible)
        .min(available)
}

fn panel_layout(
    trigger: Rect,
    bounds: Rect,
    len: usize,
    max_visible: u16,
    row_height: u16,
) -> Option<(Rect, RowViewport)> {
    let row_height = row_height.max(1);
    // Popup bounds contain only the bordered panel; the trigger paints elsewhere.
    let visible = visible_count(
        len,
        max_visible,
        bounds.height.saturating_sub(2) / row_height,
    );
    if visible == 0 || trigger.width == 0 {
        return None;
    }
    let height = visible.saturating_mul(row_height).saturating_add(2);
    let desired_y = trigger.y.saturating_sub(1);
    let max_y = bounds.bottom().saturating_sub(height);
    let y = desired_y.clamp(bounds.y, max_y);
    // The scroll offset is not decided here: the panel resolves it each frame
    // from the wheel-held transient and the cursor, then records it.
    Some((
        Rect::new(trigger.x, y, trigger.width, height),
        RowViewport::new(row_height),
    ))
}

#[cfg(test)]
const fn frame_area() -> Rect {
    Rect::new(0, 0, 20, 8)
}

#[cfg(test)]
mod tests {
    use ratatui::{
        Terminal,
        backend::TestBackend,
        text::{Line, Span},
    };

    use super::*;
    use crate::runtime::{FocusState, Modifiers, Ratcn, ScopeOptions};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Fruit {
        Mango,
        Papaya,
        Lychee,
        Durian,
        Guava,
    }

    #[derive(Debug, PartialEq)]
    enum Msg {
        Open(bool),
        Focused(Fruit),
        Selected(Fruit),
        Focus(FocusState),
    }

    #[derive(Default)]
    struct State {
        open: bool,
        cursor: Option<Fruit>,
        selected: Option<Fruit>,
        focus: FocusState,
    }

    fn items() -> Vec<ListItem<Fruit>> {
        [
            (Fruit::Mango, "Mango"),
            (Fruit::Papaya, "Papaya"),
            (Fruit::Lychee, "Lychee"),
            (Fruit::Durian, "Durian"),
        ]
        .into_iter()
        .map(|(value, label)| ListItem::new(value, label))
        .collect()
    }

    #[test]
    fn compound_option_states_use_their_exact_style_fields() {
        let mut style = SelectStyle::fallback();
        style.selected_focused_foreground = Color::Yellow;
        style.selected_focused_background = Color::Blue;
        style.selected_disabled_foreground = Color::Magenta;
        style.selected_disabled_background = Color::Green;

        assert_eq!(
            style.resolve_row(true, true, false),
            Style::new().fg(Color::Yellow).bg(Color::Blue)
        );
        assert_eq!(
            style.resolve_row(true, true, true),
            Style::new().fg(Color::Magenta).bg(Color::Green),
            "disabled wins over focus while preserving selected identity"
        );
    }

    #[test]
    fn disabled_open_widget_measures_and_paints_only_the_closed_trigger() {
        let options = ["Mango", "Papaya"];
        let widget = SelectWidget::new(None).open(&options).disabled(true);
        let area = Rect::new(0, 0, 12, 5);
        let mut buffer = Buffer::empty(area);

        assert_eq!(widget.visible_items(8, area.height), 0);
        assert_eq!(widget.height(8, area.height), TRIGGER_HEIGHT);
        widget.render(area, &mut buffer);

        assert_eq!(
            buffer.cell((10, 0)).expect("indicator").symbol(),
            INDICATOR_CLOSED
        );
        assert!(
            (1..area.height).all(|row| buffer
                .cell((0, row))
                .is_some_and(|cell| cell.symbol() == " ")),
            "a disabled open widget must not paint panel rows"
        );
    }

    fn select(items: Vec<ListItem<Fruit>>) -> Select<Fruit, State, Msg> {
        Select::new(items)
            .placeholder("Pick a fruit")
            .open(|state: &State| state.open, Msg::Open)
            .item_focus(|state: &State| state.cursor, Msg::Focused)
            .selection(|state: &State| state.selected, Msg::Selected)
    }

    struct Driver {
        terminal: Terminal<TestBackend>,
        ratcn: Ratcn<State, Msg>,
    }

    impl Driver {
        fn new(width: u16, height: u16) -> Self {
            Self {
                terminal: Terminal::new(TestBackend::new(width, height)).expect("terminal"),
                ratcn: Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
            }
        }

        fn render(&mut self, state: &State, area: Rect, items: &[ListItem<Fruit>]) {
            let theme = Theme::default_dark();
            self.terminal
                .draw(|frame| {
                    self.ratcn.render(frame, state, &theme, |ctx| {
                        ctx.component("fruit", select(items.to_vec()), area);
                        ctx.paint(|ctx| {
                            ctx.widget(Line::from("later sibling"), Rect::new(0, 4, 20, 1));
                        });
                    });
                })
                .expect("draw");
        }

        fn event(&mut self, event: Event, state: &State) -> EventResult<Msg> {
            self.ratcn.handle_event(event, state)
        }

        fn row(&self, row: u16) -> String {
            let buffer = self.terminal.backend().buffer();
            (0..buffer.area.width)
                .map(|column| buffer.cell((column, row)).expect("cell").symbol())
                .collect()
        }
    }

    fn mouse(kind: MouseKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: Modifiers::NONE,
        })
    }

    #[test]
    fn popup_starts_above_the_trigger_and_layers_above_later_paint() {
        let mut driver = Driver::new(20, 10);
        let state = State {
            open: true,
            selected: Some(Fruit::Papaya),
            ..State::default()
        };
        driver.render(&state, Rect::new(0, 4, 20, 1), &items());
        assert!(driver.row(3).contains("╭"), "{}", driver.row(3));
        assert!(driver.row(4).contains("Mango"), "{}", driver.row(4));
        assert!(!driver.row(4).contains("later sibling"));
        assert!(driver.row(5).contains("Papaya"));
    }

    #[test]
    fn popup_composites_only_the_panel_area() {
        let mut terminal = Terminal::new(TestBackend::new(20, 8)).expect("terminal");
        let mut ratcn = Ratcn::<State, Msg>::new();
        let state = State {
            open: true,
            ..State::default()
        };
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.component("fruit", select(items()), Rect::new(0, 0, 20, 1));
                    ctx.paint(|ctx| {
                        ctx.widget(Line::from("outside panel"), Rect::new(0, 7, 20, 1));
                    });
                });
            })
            .expect("draw");
        let row: String = (0..20)
            .map(|column| {
                terminal
                    .backend()
                    .buffer()
                    .cell((column, 7))
                    .expect("cell")
                    .symbol()
            })
            .collect();
        assert!(row.contains("outside panel"), "{row}");
    }

    #[test]
    fn panel_shifts_inside_the_frame_when_default_position_does_not_fit() {
        let mut driver = Driver::new(20, 8);
        let state = State {
            open: true,
            ..State::default()
        };
        driver.render(&state, Rect::new(0, 7, 20, 1), &items());
        assert!(driver.row(3).contains("Mango"), "{}", driver.row(3));
        assert!(driver.row(7).contains("╯"), "{}", driver.row(7));
    }

    #[test]
    fn panel_position_depends_only_on_geometry() {
        let trigger = Rect::new(0, 4, 20, 1);
        let bounds = Rect::new(0, 0, 20, 10);
        // The cursor is not an input: neither moving it nor scrolling can
        // move the popup.
        let (panel, viewport) = panel_layout(trigger, bounds, 10, 4, 1).expect("panel");
        assert_eq!(panel.y, trigger.y - 1);
        assert_eq!(viewport.painted_offset(), 0, "the panel resolves scrolling");
    }

    #[test]
    fn keys_navigate_select_and_first_tab_closes() {
        let mut driver = Driver::new(20, 8);
        let state = State {
            open: true,
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        driver.render(&state, Rect::new(0, 0, 20, 1), &items());
        assert_eq!(
            driver.event(Event::Key(KeyEvent::new(KeyCode::Down)), &state),
            EventResult::Emit(Msg::Focused(Fruit::Papaya))
        );
        assert_eq!(
            driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(Msg::Open(false))
        );
    }

    #[test]
    fn open_close_commit_and_modified_key_contracts_are_explicit() {
        let closed = State::default();
        for code in [
            KeyCode::Enter,
            KeyCode::Char(' '),
            KeyCode::Up,
            KeyCode::Down,
        ] {
            let mut component = select(items());
            component.prepare(&closed);
            assert_eq!(
                component.handle_event(
                    &Event::Key(KeyEvent::new(code)),
                    &closed,
                    &mut EventCtx::default(),
                ),
                EventResult::Emit(Msg::Open(true)),
                "{code:?} opens a closed Select"
            );
        }

        let open = State {
            open: true,
            cursor: Some(Fruit::Papaya),
            ..State::default()
        };
        let mut component = select(items());
        component.prepare(&open);
        assert_eq!(
            component.handle_event(
                &Event::Key(KeyEvent::new(KeyCode::Esc)),
                &open,
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Open(false))
        );
        assert_eq!(
            component.handle_event(
                &Event::Key(KeyEvent::new(KeyCode::Char(' '))),
                &open,
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Selected(Fruit::Papaya))
        );
        assert_eq!(
            component.handle_event(
                &Event::Key(KeyEvent {
                    code: KeyCode::Char('k'),
                    modifiers: Modifiers {
                        ctrl: true,
                        ..Modifiers::NONE
                    },
                }),
                &open,
                &mut EventCtx::default(),
            ),
            EventResult::Ignored
        );
        assert_eq!(
            component.handle_event(
                &Event::Paste("fruit".to_owned()),
                &open,
                &mut EventCtx::default(),
            ),
            EventResult::Ignored
        );
        assert_eq!(
            component.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), 0, 0),
                &closed,
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Open(true))
        );
    }

    #[test]
    fn paging_moves_the_cursor_by_the_painted_viewport() {
        let mut driver = Driver::new(20, 4);
        let state = State {
            open: true,
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        driver.render(&state, Rect::new(0, 0, 20, 1), &items());

        assert_eq!(
            driver.event(Event::Key(KeyEvent::new(KeyCode::PageDown)), &state),
            EventResult::Emit(Msg::Focused(Fruit::Lychee)),
            "only two option rows fit in the painted panel"
        );
    }

    #[test]
    fn wheel_scrolls_the_view_and_the_offset_survives_redraw() {
        let mut driver = Driver::new(20, 4);
        let mut state = State {
            open: true,
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        driver.render(&state, Rect::new(0, 0, 20, 1), &items());
        assert!(driver.row(1).contains("Mango"), "{}", driver.row(1));

        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 3, 1),
                &state
            ),
            EventResult::Consumed,
            "the wheel scrolls the view without emitting a cursor move"
        );
        driver.render(&state, Rect::new(0, 0, 20, 1), &items());
        assert!(
            driver.row(1).contains("Lychee") && driver.row(2).contains("Durian"),
            "the wheeled offset survives the redraw, cursor off-screen: {} / {}",
            driver.row(1),
            driver.row(2)
        );

        // Keyboard navigation still starts from the cursor, and the next
        // redraw scrolls it back into view from the wheeled offset.
        assert_eq!(
            driver.event(Event::Key(KeyEvent::new(KeyCode::Down)), &state),
            EventResult::Emit(Msg::Focused(Fruit::Papaya))
        );
        state.cursor = Some(Fruit::Papaya);
        driver.render(&state, Rect::new(0, 0, 20, 1), &items());
        assert!(
            driver.row(1).contains("Papaya"),
            "the cursor scrolls back into view: {}",
            driver.row(1)
        );
    }

    #[test]
    fn hover_moves_the_cursor_and_reopening_reanchors_the_view() {
        let mut driver = Driver::new(20, 4);
        let mut state = State {
            open: true,
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        let area = Rect::new(0, 0, 20, 1);
        driver.render(&state, area, &items());

        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 3, 2), &state),
            EventResult::Emit(Msg::Focused(Fruit::Papaya)),
            "hover moves the one cursor, as in List"
        );

        // Hold the view away from the cursor, then close and reopen: the
        // held offset dies with the panel's identity and the view anchors
        // to the cursor again.
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 3, 1),
                &state
            ),
            EventResult::Consumed
        );
        driver.render(&state, area, &items());
        assert!(driver.row(1).contains("Lychee"), "{}", driver.row(1));
        state.open = false;
        driver.render(&state, area, &items());
        state.open = true;
        driver.render(&state, area, &items());
        assert!(
            driver.row(1).contains("Mango"),
            "a reopened panel anchors to the cursor: {}",
            driver.row(1)
        );
    }

    #[test]
    fn custom_option_rows_preserve_explicit_span_colors() {
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).expect("terminal");
        let mut ratcn = Ratcn::<State, Msg>::new();
        let state = State {
            open: true,
            selected: Some(Fruit::Mango),
            ..State::default()
        };
        let theme = Theme::default_dark();
        let mut style = SelectStyle::fallback();
        style.option_foreground = Color::Yellow;
        style.panel_background = Color::Blue;

        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.component(
                        "fruit",
                        select(items())
                            .paint_item(|_: &State, row| {
                                if row.selected {
                                    Text::from(Span::styled(
                                        row.label.to_string(),
                                        Style::new().fg(Color::Magenta).bg(Color::Green),
                                    ))
                                } else {
                                    Text::from(row.label.to_string())
                                }
                            })
                            .style(move |_| style),
                        Rect::new(0, 0, 20, 1),
                    );
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let explicit = buffer.cell((1, 1)).expect("explicitly colored span");
        assert_eq!(explicit.fg, Color::Magenta, "explicit colors win");
        assert_eq!(explicit.bg, Color::Green);
        let inherited = buffer.cell((1, 2)).expect("unstyled span");
        assert_eq!(
            inherited.fg,
            Color::Yellow,
            "unstyled text inherits the option-state colors"
        );
        assert_eq!(inherited.bg, Color::Blue);
    }

    /// The panel builds custom rows for the options it paints and no others, so
    /// a scrolled panel's rows still belong to the options beneath them.
    #[test]
    fn a_scrolled_panel_builds_custom_rows_for_the_options_on_screen() {
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        let mut ratcn = Ratcn::<State, Msg>::new();
        let state = State {
            open: true,
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        let theme = Theme::default_dark();
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        let area = Rect::new(0, 0, 20, 1);
        let draw = |terminal: &mut Terminal<TestBackend>,
                    ratcn: &mut Ratcn<State, Msg>,
                    recorded: Rc<std::cell::RefCell<Vec<usize>>>| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.component(
                            "fruit",
                            select(items()).paint_item(
                                move |_: &State, row: ListItemState<'_, Fruit>| {
                                    recorded.borrow_mut().push(row.index);
                                    Text::from(row.label.to_string())
                                },
                            ),
                            area,
                        );
                    });
                })
                .expect("draw");
        };

        draw(&mut terminal, &mut ratcn, Rc::clone(&seen));
        assert_eq!(*seen.borrow(), vec![0, 1], "two options fit in the panel");

        assert_eq!(
            ratcn.handle_event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 3, 1),
                &state
            ),
            EventResult::Consumed
        );
        seen.borrow_mut().clear();
        draw(&mut terminal, &mut ratcn, Rc::clone(&seen));

        assert_eq!(
            *seen.borrow(),
            vec![2, 3],
            "the scrolled window keeps the options' own indices"
        );
        let row: String = (0..20)
            .map(|column| {
                terminal
                    .backend()
                    .buffer()
                    .cell((column, 1))
                    .expect("cell")
                    .symbol()
            })
            .collect();
        assert!(row.contains("Lychee"), "{row}");
    }

    /// Returning the cursor to where the wheel left it must not revive the
    /// held view — the same rule `List` follows.
    #[test]
    fn a_released_wheel_hold_never_revives() {
        let mut driver = Driver::new(20, 4);
        let mut state = State {
            open: true,
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        let area = Rect::new(0, 0, 20, 1);
        driver.render(&state, area, &items());
        driver.event(
            mouse(MouseKind::Scroll(ScrollDirection::Down), 3, 1),
            &state,
        );
        driver.render(&state, area, &items());
        assert!(driver.row(1).contains("Lychee"), "{}", driver.row(1));

        assert_eq!(
            driver.event(Event::Key(KeyEvent::new(KeyCode::Down)), &state),
            EventResult::Emit(Msg::Focused(Fruit::Papaya))
        );
        state.cursor = Some(Fruit::Papaya);
        driver.render(&state, area, &items());
        assert!(driver.row(1).contains("Papaya"), "{}", driver.row(1));

        assert_eq!(
            driver.event(Event::Key(KeyEvent::new(KeyCode::Up)), &state),
            EventResult::Emit(Msg::Focused(Fruit::Mango))
        );
        state.cursor = Some(Fruit::Mango);
        driver.render(&state, area, &items());
        assert!(
            driver.row(1).contains("Mango"),
            "returning to the wheel anchor must not re-take the hold: {}",
            driver.row(1)
        );
    }

    /// The panel's hold is anchored to the option the cursor sits on, not to
    /// its row — `List`'s rule. Swapping that option for another at the same
    /// index puts a different option under the cursor, so the hold ends.
    #[test]
    fn replacing_the_anchored_option_releases_the_wheel_hold() {
        let mut driver = Driver::new(20, 4);
        let mut state = State {
            open: true,
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        let area = Rect::new(0, 0, 20, 1);
        driver.render(&state, area, &items());
        driver.event(
            mouse(MouseKind::Scroll(ScrollDirection::Down), 3, 1),
            &state,
        );
        driver.render(&state, area, &items());
        assert!(
            driver.row(1).contains("Lychee"),
            "the wheel held the view away from the cursor: {}",
            driver.row(1)
        );

        let mut swapped = items();
        swapped[0] = ListItem::new(Fruit::Guava, "Guava");
        state.cursor = Some(Fruit::Guava);
        driver.render(&state, area, &swapped);

        assert!(
            driver.row(1).contains("Guava"),
            "a hold anchored by row would still be showing Lychee on screen: {}",
            driver.row(1)
        );
    }

    /// A bound cursor on a disabled option is never silently moved elsewhere,
    /// and committing it is refused — `List`'s contract.
    #[test]
    fn a_disabled_cursor_is_neither_retargeted_nor_committed() {
        let mut items = items();
        items[1] = ListItem::new(Fruit::Papaya, "Papaya").disabled(true);
        let state = State {
            open: true,
            cursor: Some(Fruit::Papaya),
            ..State::default()
        };
        let mut component = select(items);
        component.prepare(&state);

        // The cursor really is on the disabled option: moving up from it
        // reaches Mango. A retargeted cursor would already be on Mango and
        // would have nowhere to go.
        assert_eq!(
            component.handle_event(
                &Event::Key(KeyEvent::new(KeyCode::Up)),
                &state,
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Focused(Fruit::Mango))
        );
        for code in [KeyCode::Enter, KeyCode::Char(' ')] {
            assert_eq!(
                component.handle_event(
                    &Event::Key(KeyEvent::new(code)),
                    &state,
                    &mut EventCtx::default(),
                ),
                EventResult::Ignored,
                "{code:?} must not commit a disabled option"
            );
        }
    }

    /// An all-disabled Select is inert from the keyboard, as a `List` is: it
    /// does not even open onto a panel nothing can be chosen from.
    #[test]
    fn an_all_disabled_select_ignores_keys_instead_of_opening() {
        let items: Vec<ListItem<Fruit>> = items()
            .into_iter()
            .map(|item| {
                let value = *item.value();
                ListItem::new(value, item.label()).disabled(true)
            })
            .collect();
        let closed = State::default();
        let mut component = select(items);
        component.prepare(&closed);

        for code in [
            KeyCode::Enter,
            KeyCode::Char(' '),
            KeyCode::Up,
            KeyCode::Down,
        ] {
            assert_eq!(
                component.handle_event(
                    &Event::Key(KeyEvent::new(code)),
                    &closed,
                    &mut EventCtx::default(),
                ),
                EventResult::Ignored,
                "{code:?} must not open a Select with nothing to choose"
            );
        }
    }

    /// With nothing to commit to, a click still moves the cursor — `List`'s
    /// behavior for the same binding combination.
    #[test]
    fn clicking_without_a_selection_binding_moves_the_cursor() {
        let mut driver = Driver::new(20, 6);
        let state = State {
            open: true,
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        let theme = Theme::default_dark();
        let area = Rect::new(0, 0, 20, 1);
        driver
            .terminal
            .draw(|frame| {
                driver.ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.component(
                        "fruit",
                        Select::new(items())
                            .open(|state: &State| state.open, Msg::Open)
                            .item_focus(|state: &State| state.cursor, Msg::Focused),
                        area,
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 3, 2), &state),
            EventResult::Emit(Msg::Focused(Fruit::Papaya))
        );
    }

    /// The popup is not modal, so a key it has no use for reaches the app,
    /// exactly as it would over a `List`.
    #[test]
    fn an_open_select_does_not_swallow_app_hotkeys() {
        let state = State {
            open: true,
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        let mut component = select(items());
        component.prepare(&state);

        for code in [KeyCode::Delete, KeyCode::Backspace, KeyCode::F(5)] {
            assert_eq!(
                component.handle_event(
                    &Event::Key(KeyEvent::new(code)),
                    &state,
                    &mut EventCtx::default(),
                ),
                EventResult::Ignored,
                "{code:?} must bubble to the app"
            );
        }
    }

    #[test]
    fn panel_pointer_hit_testing_uses_painted_viewport() {
        let mut driver = Driver::new(20, 6);
        let state = State {
            open: true,
            cursor: Some(Fruit::Durian),
            ..State::default()
        };
        driver.render(&state, Rect::new(0, 0, 20, 1), &items());
        assert!(driver.row(2).contains("Papaya"), "{}", driver.row(2));
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 3, 2), &state),
            EventResult::Emit(Msg::Selected(Fruit::Papaya))
        );
    }

    #[test]
    fn outside_press_uses_the_open_bindings_close_message() {
        let mut driver = Driver::new(20, 8);
        let state = State {
            open: true,
            ..State::default()
        };
        driver.render(&state, Rect::new(0, 0, 20, 1), &items());
        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 19, 7), &state),
            EventResult::Emit(Msg::Open(false))
        );
    }

    #[test]
    fn clicking_the_first_option_selects_it_where_the_panel_overlays_the_trigger() {
        let mut driver = Driver::new(20, 12);
        let state = State {
            open: true,
            ..State::default()
        };
        driver.render(&state, Rect::new(0, 4, 20, 1), &items());

        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 3, 4), &state),
            EventResult::Consumed,
            "pressing the overlaid option must not dismiss the popup"
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 3, 4), &state),
            EventResult::Emit(Msg::Selected(Fruit::Mango))
        );
    }

    #[test]
    fn closing_selection_cannot_commit_twice_before_redraw() {
        let mut driver = Driver::new(20, 12);
        let mut state = State {
            open: true,
            ..State::default()
        };
        driver.render(&state, Rect::new(0, 4, 20, 1), &items());
        let click = mouse(MouseKind::Click(MouseButton::Left), 3, 4);

        assert_eq!(
            driver.event(click.clone(), &state),
            EventResult::Emit(Msg::Selected(Fruit::Mango))
        );
        state.open = false;
        assert_eq!(
            driver.event(click, &state),
            EventResult::Consumed,
            "the retained popup reads current open state before handling the second click"
        );
    }

    #[test]
    fn popup_layers_above_content_inside_a_modal() {
        let mut terminal = Terminal::new(TestBackend::new(20, 8)).expect("terminal");
        let mut ratcn = Ratcn::<State, Msg>::new();
        let state = State {
            open: true,
            ..State::default()
        };
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.modal_scope("modal", frame_area(), ScopeOptions::default(), |ctx| {
                        ctx.component("fruit", select(items()), Rect::new(0, 0, 20, 1));
                        ctx.paint(|ctx| {
                            ctx.widget(Line::from("modal sibling"), Rect::new(0, 2, 20, 1));
                        });
                    });
                });
            })
            .expect("draw");
        let row: String = (0..20)
            .map(|column| {
                terminal
                    .backend()
                    .buffer()
                    .cell((column, 2))
                    .expect("cell")
                    .symbol()
            })
            .collect();
        assert!(row.contains("Papaya"), "{row}");
        assert!(!row.contains("modal sibling"));
    }

    #[test]
    fn vim_and_readline_keys_move_item_focus_only_while_open() {
        let open = State {
            open: true,
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        let mut component = select(items());
        component.prepare(&open);
        for down in [
            Event::Key(KeyEvent::new(KeyCode::Char('j'))),
            Event::Key(KeyEvent {
                code: KeyCode::Char('n'),
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::NONE
                },
            }),
        ] {
            assert_eq!(
                component.handle_event(&down, &open, &mut EventCtx::default()),
                component.handle_event(
                    &Event::Key(KeyEvent::new(KeyCode::Down)),
                    &open,
                    &mut EventCtx::default(),
                ),
                "j and Ctrl+N do exactly what Down does"
            );
        }

        let closed = State {
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        let mut component = select(items());
        component.prepare(&closed);
        assert_eq!(
            component.handle_event(
                &Event::Key(KeyEvent::new(KeyCode::Char('j'))),
                &closed,
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Open(true)),
            "a step key on a closed Select opens it, as Down does"
        );
        assert_eq!(
            component.handle_event(
                &Event::Key(KeyEvent::new(KeyCode::Char('x'))),
                &closed,
                &mut EventCtx::default(),
            ),
            EventResult::Ignored,
            "any other letter bubbles: there is no typeahead"
        );
    }

    #[test]
    fn open_selection_without_item_focus_is_pointer_only() {
        let state = State::default();
        let mut component = Select::new(items())
            .open(|state: &State| state.open, Msg::Open)
            .selection(|state: &State| state.selected, Msg::Selected);
        component.prepare(&state);

        assert!(!component.is_focusable());
        assert_eq!(
            component.handle_event(
                &Event::Key(KeyEvent::new(KeyCode::Down)),
                &state,
                &mut EventCtx::default(),
            ),
            EventResult::Ignored
        );
        assert_eq!(
            component.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), 0, 0),
                &state,
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Open(true))
        );
    }

    #[test]
    fn incomplete_binding_combinations_never_claim_keyboard_focus() {
        let state = State::default();
        let mut component = Select::new(items())
            .open(|state: &State| state.open, Msg::Open)
            .item_focus(|state: &State| state.cursor, Msg::Focused);
        component.prepare(&state);

        assert!(!component.is_focusable());
        assert_eq!(
            component.handle_event(
                &Event::Key(KeyEvent::new(KeyCode::Down)),
                &state,
                &mut EventCtx::default(),
            ),
            EventResult::Ignored,
            "a Select without a selection commit channel must not consume keyboard navigation"
        );
    }

    fn tall_select(items: Vec<ListItem<Fruit>>) -> Select<Fruit, State, Msg> {
        select(items)
            .paint_item(|_state: &State, row| {
                Text::from(vec![
                    Line::from(row.label.to_string()),
                    Line::from(format!("  focused={}", row.focused)),
                ])
            })
            .row_height(2)
    }

    #[test]
    fn custom_multi_row_options_render_at_the_declared_row_height() {
        let mut terminal = Terminal::new(TestBackend::new(20, 12)).expect("terminal");
        let mut ratcn = Ratcn::<State, Msg>::new();
        let state = State {
            open: true,
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.component("fruit", tall_select(items()), Rect::new(0, 4, 20, 1));
                });
            })
            .expect("draw");

        let row = |y: u16| -> String {
            let buffer = terminal.backend().buffer();
            (0..20)
                .map(|x| buffer.cell((x, y)).expect("cell").symbol())
                .collect()
        };
        // Panel: border, then two rows per option.
        assert!(row(3).contains("Mango"), "{}", row(3));
        assert!(row(4).contains("focused=true"), "{}", row(4));
        assert!(row(5).contains("Papaya"), "{}", row(5));
        assert!(row(6).contains("focused=false"), "{}", row(6));
    }

    #[test]
    fn multi_row_click_hit_testing_divides_by_row_height() {
        let mut terminal = Terminal::new(TestBackend::new(20, 12)).expect("terminal");
        let mut ratcn = Ratcn::<State, Msg>::new();
        let state = State {
            open: true,
            cursor: Some(Fruit::Mango),
            ..State::default()
        };
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.component("fruit", tall_select(items()), Rect::new(0, 4, 20, 1));
                });
            })
            .expect("draw");

        // The first option occupies two rows; a click on its second row still
        // selects it, and the next option starts two rows down.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Click(MouseButton::Left), 3, 4), &state),
            EventResult::Emit(Msg::Selected(Fruit::Mango)),
            "the second line belongs to the first option"
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Click(MouseButton::Left), 3, 5), &state),
            EventResult::Emit(Msg::Selected(Fruit::Papaya))
        );
    }

    #[test]
    fn disabled_and_duplicate_item_behavior_is_preserved() {
        let state = State::default();
        let mut disabled = select(items()).disabled(true);
        disabled.prepare(&state);
        assert!(!disabled.is_focusable());
        assert_eq!(
            disabled.handle_event(
                &Event::Key(KeyEvent::new(KeyCode::Enter)),
                &state,
                &mut EventCtx::default()
            ),
            EventResult::Ignored
        );

        // The duplicate check is a debug-build assertion, so this half of the
        // contract only exists where debug assertions do.
        if cfg!(debug_assertions) {
            let mut duplicate = select(vec![
                ListItem::new(Fruit::Mango, "Mango"),
                ListItem::new(Fruit::Mango, "Again"),
            ]);
            assert!(
                std::panic::catch_unwind(
                    std::panic::AssertUnwindSafe(|| duplicate.prepare(&state))
                )
                .is_err()
            );
        }
    }
}
