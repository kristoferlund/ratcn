use std::fmt;

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect, Size},
    style::{Color, Style},
    text::Line,
    widgets::Widget,
};

use crate::Theme;
use crate::button_shape::{BOTTOM_CAP, TOP_CAP, cap_row, filled_middle, shape_width};
use crate::color::{
    DISABLED_DIM, FOCUS_DARKEN, FOCUS_LIGHTEN, HOVER_DARKEN, HOVER_LIGHTEN, darken, dim, lighten,
};
use crate::linear_nav;
use crate::list_core;
use crate::runtime::{
    Component, Event, EventCtx, EventResult, KeyCode, KeyEvent, MeasuredComponent, MouseButton,
    MouseKind, PaintCtx, RenderCtx, Step, fixed_height,
};
use crate::theme::resolve_style;

/// How tall the tab row is drawn. Matches [`ButtonSize`](crate::ButtonSize),
/// since a tab is painted as a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TabsSize {
    /// One row: labels only.
    #[default]
    Small,
    /// Three rows: labels with a fill cap above and below.
    Large,
}

/// Whether moving between tabs also switches to them.
///
/// The distinction matters when switching tabs is expensive or destructive —
/// loading a page, discarding a draft. Manual lets the user look before
/// committing; automatic is faster when there is nothing to lose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TabsActivation {
    /// Left/Right move a cursor between tabs; Enter or Space switches to the
    /// one under it. Two-step, and the default.
    #[default]
    Manual,
    /// Left/Right switch tabs directly, with no separate cursor. The
    /// [`tab_focus`](Tabs::tab_focus) binding is unused in this mode.
    Automatic,
}

impl TabsSize {
    /// Rows this size occupies — 1 for `Small`, 3 for `Large`.
    #[must_use]
    pub const fn height(self) -> u16 {
        match self {
            Self::Small => 1,
            Self::Large => 3,
        }
    }
}

/// Blank cells between adjacent tabs, so each reads as its own button.
const SPACING: u16 = 1;
/// Single-cell markers shown on a side that has hidden tabs.
const LEFT_MARKER: &str = "‹";
const RIGHT_MARKER: &str = "›";
const MARKER_WIDTH: u16 = 1;

/// Every color a tab row can paint.
///
/// A tab is drawn as a button, so these mirror [`ButtonStyle`](crate::ButtonStyle)
/// with one axis added: whether the tab is the selected one. The selected tab
/// looks like a `Default` button and the rest like `Secondary` buttons, which is
/// what makes the active tab read as the primary thing on the row.
///
/// Every state gives both a label color and a fill, so a row can express focus,
/// hover, and selection through text alone — set every `*_background` to the
/// surface the row sits on and the tabs lose their chrome without losing their
/// feedback.
///
/// When several states apply at once, disabled wins, then hovered, then focused,
/// then the resting colors — the same precedence [`ButtonStyle`](crate::ButtonStyle)
/// uses. Hover beating focus is what keeps pointing at an already-focused tab
/// visible.
///
/// "Focused" and "hovered" describe the tab under the cursor while the row has
/// keyboard focus or the pointer, not the whole row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabsStyle {
    /// An unselected tab's label (the secondary button's foreground).
    pub foreground: Color,
    /// An unselected tab's fill (the secondary button's background).
    pub background: Color,
    /// An unselected tab's label while it is the cursor tab and the row has
    /// keyboard focus.
    pub focused_foreground: Color,
    /// An unselected tab's fill while it is the cursor tab and the row has
    /// keyboard focus (the secondary button's focus lighten).
    pub focused_background: Color,
    /// An unselected tab's label while the pointer is over the row and this is
    /// the cursor tab.
    pub hovered_foreground: Color,
    /// An unselected tab's fill while hovered. Wins over focus, as on buttons.
    pub hovered_background: Color,
    /// The selected tab's label (the default button's foreground).
    pub selected_foreground: Color,
    /// The selected tab's fill (the default button's background).
    pub selected_background: Color,
    /// The selected tab's label while focused.
    pub selected_focused_foreground: Color,
    /// The selected tab's fill while focused (the default button's focus
    /// darken).
    pub selected_focused_background: Color,
    /// The selected tab's label while hovered.
    pub selected_hovered_foreground: Color,
    /// The selected tab's fill while hovered.
    pub selected_hovered_background: Color,
    /// A disabled tab's label.
    pub disabled_foreground: Color,
    /// A disabled tab's fill.
    pub disabled_background: Color,
    /// A selected, disabled tab's label.
    pub selected_disabled_foreground: Color,
    /// A selected, disabled tab's fill. This state keeps selected identity while
    /// disabledness suppresses interaction.
    pub selected_disabled_background: Color,
}

impl TabsStyle {
    /// A neutral style using plain ANSI colors, for painting without a
    /// [`Theme`]. Prefer [`from_theme`](Self::from_theme) when one is available.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            foreground: Color::Gray,
            background: Color::DarkGray,
            focused_foreground: Color::White,
            focused_background: Color::Gray,
            hovered_foreground: Color::White,
            hovered_background: Color::Gray,
            selected_foreground: Color::Black,
            selected_background: Color::Cyan,
            selected_focused_foreground: Color::Black,
            selected_focused_background: Color::Cyan,
            selected_hovered_foreground: Color::Black,
            selected_hovered_background: Color::LightCyan,
            disabled_foreground: Color::DarkGray,
            disabled_background: Color::Reset,
            selected_disabled_foreground: Color::DarkGray,
            selected_disabled_background: Color::Cyan,
        }
    }

    /// Derived from the theme exactly as the button variants are: the selected
    /// tab is a `Default` (primary) button, an unselected tab a `Secondary`
    /// button — including the same focus and hover shifts (a bright fill
    /// darkens, a dark fill lightens) and the disabled dim toward the surface.
    ///
    /// Label colors do not change with focus or hover here, since the fill
    /// already carries that. A style that flattens the fills should set the
    /// `*_foreground` slots instead.
    #[must_use]
    pub const fn from_theme(theme: &Theme) -> Self {
        Self {
            foreground: theme.secondary_foreground,
            background: theme.secondary,
            focused_foreground: theme.secondary_foreground,
            focused_background: lighten(theme.secondary, FOCUS_LIGHTEN),
            hovered_foreground: theme.secondary_foreground,
            hovered_background: lighten(theme.secondary, HOVER_LIGHTEN),
            selected_foreground: theme.primary_foreground,
            selected_background: theme.primary,
            selected_focused_foreground: theme.primary_foreground,
            selected_focused_background: darken(theme.primary, FOCUS_DARKEN),
            selected_hovered_foreground: theme.primary_foreground,
            selected_hovered_background: darken(theme.primary, HOVER_DARKEN),
            disabled_foreground: theme.muted_foreground,
            disabled_background: dim(theme.secondary, theme.surface, DISABLED_DIM),
            selected_disabled_foreground: theme.muted_foreground,
            selected_disabled_background: dim(theme.primary, theme.surface, DISABLED_DIM),
        }
    }

    /// The label and fill for one tab's paint — the single place a tab's state
    /// is turned into style.
    ///
    /// Selection picks the family: a selected tab paints like the default
    /// button, an unselected one like the secondary button. Within a family the
    /// precedence matches [`ButtonStyle`](crate::ButtonStyle) — disabled first,
    /// then hovered, then focused, then the resting colors. Hover beats focus so
    /// that pointing at an already-focused tab still changes something.
    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "the four independent tab states; call sites read from named fields"
    )]
    fn resolve(
        &self,
        selected: bool,
        focused: bool,
        hovered: bool,
        disabled: bool,
    ) -> ResolvedTabStyle {
        let (foreground, fill) = match (selected, disabled, hovered, focused) {
            (true, true, _, _) => (
                self.selected_disabled_foreground,
                self.selected_disabled_background,
            ),
            (false, true, _, _) => (self.disabled_foreground, self.disabled_background),
            (true, false, true, _) => (
                self.selected_hovered_foreground,
                self.selected_hovered_background,
            ),
            (true, false, false, true) => (
                self.selected_focused_foreground,
                self.selected_focused_background,
            ),
            (true, false, false, false) => (self.selected_foreground, self.selected_background),
            (false, false, true, _) => (self.hovered_foreground, self.hovered_background),
            (false, false, false, true) => (self.focused_foreground, self.focused_background),
            (false, false, false, false) => (self.foreground, self.background),
        };
        ResolvedTabStyle { foreground, fill }
    }
}

/// One tab's resolved paint (see [`TabsStyle::resolve`]): the label color and
/// the button fill (which is also the cap color).
#[derive(Clone, Copy)]
struct ResolvedTabStyle {
    foreground: Color,
    fill: Color,
}

/// A tab row that only draws — an ordinary ratatui [`Widget`] with no focus,
/// events, or state. The selected tab is styled as a default button and the rest
/// as secondary buttons.
///
/// **Usable in any ratatui app.** Nothing here depends on
/// [`Ratcn`](crate::runtime::Ratcn) or the component layer: render it directly
/// and keep tracking the selected tab however you already do. Use [`Tabs`] when
/// you want keyboard traversal and selection messages handled for you; it paints
/// through this widget internally.
///
/// Inputs are parallel to the labels by index — which tab is selected
/// ([`selected_tab`](Self::selected_tab)), which tab the cursor is on
/// ([`focused_tab`](Self::focused_tab)), and which tabs are disabled
/// ([`disabled_tabs`](Self::disabled_tabs)) — the same shape as
/// [`ListWidget`](crate::ListWidget). When the tabs are wider than the row, the
/// widget shows the group around the selected/focused tab and adds `‹`/`›`
/// markers on sides that have hidden tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabsWidget<'a> {
    labels: &'a [&'a str],
    selected_tab: Option<usize>,
    focused_tab: Option<usize>,
    disabled_tabs: &'a [bool],
    focused: bool,
    hovered_tab: Option<usize>,
    disabled: bool,
    size: TabsSize,
    style: TabsStyle,
    layout: Option<&'a TabLayout>,
}

impl<'a> TabsWidget<'a> {
    /// A row of `labels`, nothing selected or focused, using
    /// [`TabsStyle::fallback`].
    #[must_use]
    pub const fn new(labels: &'a [&'a str]) -> Self {
        Self {
            labels,
            selected_tab: None,
            focused_tab: None,
            disabled_tabs: &[],
            focused: false,
            hovered_tab: None,
            disabled: false,
            size: TabsSize::Small,
            style: TabsStyle::fallback(),
            layout: None,
        }
    }

    /// Paint this tab geometry instead of measuring the labels again.
    ///
    /// Left unset, the widget lays the row out itself from the labels, the size,
    /// and whichever tab must stay visible — all a standalone caller needs. Pass
    /// one when something outside the widget has already computed the same
    /// layout and has to agree with the paint cell for cell: [`Tabs`] hands over
    /// the layout it hit-tests clicks against, so the tab a click lands on is
    /// the tab drawn there by construction rather than because both sides ran
    /// the same arithmetic.
    ///
    /// Build one with [`tab_layout`]. A layout measured against a different area
    /// paints where that area was, since the rects are absolute.
    #[must_use]
    pub const fn layout(mut self, layout: &'a TabLayout) -> Self {
        self.layout = Some(layout);
        self
    }

    /// Take colors from `theme`.
    #[must_use]
    pub const fn themed(mut self, theme: &Theme) -> Self {
        self.style = TabsStyle::from_theme(theme);
        self
    }

    /// Use these exact colors, ignoring any theme.
    #[must_use]
    pub const fn style(mut self, style: TabsStyle) -> Self {
        self.style = style;
        self
    }

    /// The index of the selected (active) tab, filled as the default button.
    #[must_use]
    pub const fn selected_tab(mut self, selected_tab: Option<usize>) -> Self {
        self.selected_tab = selected_tab;
        self
    }

    /// The index of the cursor tab. It paints with the focus shift only while
    /// the row itself has focus ([`focused`](Self::focused)).
    #[must_use]
    pub const fn focused_tab(mut self, focused_tab: Option<usize>) -> Self {
        self.focused_tab = focused_tab;
        self
    }

    /// A disabled mask parallel to the labels; a missing entry reads as
    /// enabled. Matches [`ListWidget::disabled_rows`](crate::ListWidget::disabled_rows).
    #[must_use]
    pub const fn disabled_tabs(mut self, disabled_tabs: &'a [bool]) -> Self {
        self.disabled_tabs = disabled_tabs;
        self
    }

    /// Paint the whole row as disabled, overriding per-tab styling: every tab
    /// takes the style's disabled colors (the selected tab its
    /// selected-disabled ones). Purely visual here — a paint widget receives no
    /// events to suppress.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Whether the row has keyboard focus, so the cursor tab shows its focus
    /// colors.
    #[must_use]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// The tab under the pointer, by index. Hover wins over focus for this tab
    /// only; it does not alter the semantic cursor or selection.
    #[must_use]
    pub const fn hovered_tab(mut self, hovered_tab: Option<usize>) -> Self {
        self.hovered_tab = hovered_tab;
        self
    }

    /// Set the row height, one row or three. See [`TabsSize`].
    #[must_use]
    pub const fn size(mut self, size: TabsSize) -> Self {
        self.size = size;
        self
    }

    /// Return the row height for the configured size.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.size.height()
    }

    /// The total width the tabs want (every label plus its padding), for
    /// building layout constraints from the same labels that get painted.
    #[must_use]
    pub fn width(&self) -> u16 {
        row_width(self.labels.iter().copied())
    }
}

/// The width a full row of tab labels wants: each label plus padding, with the
/// standard gap between tabs.
fn row_width<'a>(labels: impl ExactSizeIterator<Item = &'a str>) -> u16 {
    let gap_count = u16::try_from(labels.len().saturating_sub(1)).unwrap_or(u16::MAX);
    labels
        .map(tab_width)
        .fold(0, u16::saturating_add)
        .saturating_add(gap_count.saturating_mul(SPACING))
}

impl Widget for TabsWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height < self.height() {
            return;
        }
        let Some(layout) = self.layout else {
            // Keep the focused tab visible when it exists; otherwise keep the
            // selected tab visible.
            let must_show = self.focused_tab.or(self.selected_tab).unwrap_or(0);
            self.paint_layout(&tab_layout(area, self.labels, self.size, must_show), buf);
            return;
        };
        self.paint_layout(layout, buf);
    }
}

impl TabsWidget<'_> {
    fn paint_layout(self, layout: &TabLayout, buf: &mut Buffer) {
        for &(index, rect) in &layout.tabs {
            let disabled = self.disabled || self.disabled_tabs.get(index).copied().unwrap_or(false);
            let cursor = self.focused_tab == Some(index);
            let resolved = self.style.resolve(
                self.selected_tab == Some(index),
                cursor && self.focused,
                self.hovered_tab == Some(index),
                disabled,
            );
            if self.size == TabsSize::Large {
                render_tab_cap(rect, resolved.fill, TOP_CAP, buf);
                render_tab_cap(
                    Rect::new(rect.x, rect.y + 2, rect.width, 1),
                    resolved.fill,
                    BOTTOM_CAP,
                    buf,
                );
            }
            Line::from(filled_middle(self.labels[index], rect.width as usize))
                .style(Style::default().fg(resolved.foreground).bg(resolved.fill))
                .render(
                    Rect::new(rect.x, rect.y + content_y_offset(self.size), rect.width, 1),
                    buf,
                );
        }
        for (marker, slot) in [(LEFT_MARKER, layout.left), (RIGHT_MARKER, layout.right)] {
            if let Some(rect) = slot {
                Line::from(marker)
                    .style(Style::default().fg(self.style.foreground))
                    .render(
                        Rect::new(
                            rect.x,
                            rect.y + content_y_offset(self.size),
                            MARKER_WIDTH,
                            1,
                        ),
                        buf,
                    );
            }
        }
    }
}

fn render_tab_cap(rect: Rect, fill: Color, symbol: &str, buf: &mut Buffer) {
    Line::from(cap_row(fill, symbol, rect.width as usize))
        .style(Style::default().fg(fill))
        .render(rect, buf);
}

const fn content_y_offset(size: TabsSize) -> u16 {
    match size {
        TabsSize::Small => 0,
        TabsSize::Large => 1,
    }
}

/// One tab: an identifying `value` and the `label` shown on it — the same
/// [`ListItem`](crate::ListItem) a list row is, under the name this component
/// uses for it.
///
/// A tab and a list row need exactly the same three things (a value, a label, a
/// disabled flag), so they are one type rather than two that must be kept in
/// step. `Tab::new(value, label)` and `.disabled(true)` read as before, and
/// `["One", "Two"]` still converts through the `From<&str>` impl.
///
/// Selection is recorded as the value, not the tab's position, so the tab row
/// can be built from data that changes without the selection sliding onto a
/// different tab. `value` is usually the same enum the app matches on to decide
/// what to draw below the row. Values must be unique within a [`Tabs`]
/// declaration, or selection and tab focus are ambiguous; a debug build panics
/// on duplicates during declaration.
pub type Tab<T> = list_core::ListItem<T>;

/// Reads a tab value (the selection or manual-mode tab focus) out of app
/// state; `None` means no tab holds that role yet.
type ReadFn<S, T> = Box<dyn Fn(&S) -> Option<T>>;
/// Builds the app's message from a tab value.
type OnChangeFn<T, M> = Box<dyn Fn(T) -> M>;
/// Resolves the tabs' style from the active theme (the style override).
type StyleFn = Box<dyn Fn(&Theme) -> TabsStyle>;

/// A focusable row of tabs, declared with
/// [`render_component`](crate::runtime::RenderCtx::render_component).
///
/// The horizontal counterpart of [`List`](crate::List), and it works the same
/// way: a cursor that moves and a selection that commits, both keyed by your own
/// values rather than by position.
///
/// The row draws only the tabs. What appears *below* them is the app's — match
/// on the selected value and render whatever that screen is. `Tabs` never owns
/// content.
///
/// # Cursor and selection
///
/// - [`tab_focus`](Self::tab_focus) is the cursor: which tab Left/Right and
///   Home/End are pointing at.
/// - [`selection`](Self::selection) is the active tab, the one whose content is
///   showing.
///
/// With the default [`TabsActivation::Manual`], the cursor moves freely and
/// Enter or Space commits it. With [`TabsActivation::Automatic`] there is no
/// separate cursor at all — arrows switch tabs directly, and the `tab_focus`
/// binding goes unused.
///
/// Manual activation is keyboard-focusable only when both `tab_focus` and
/// `selection` are bound. Automatic activation is keyboard-focusable only when
/// `selection` is bound. An incompletely bound row can still paint and handle
/// wired pointer actions, but it is not a focus stop and ignores keys.
///
/// A left click selects a tab. Right and middle clicks are ignored.
/// A row with zero width is not interactive. Large tabs also require all three
/// rows of height; a shorter area paints nothing and does not participate in
/// focus or pointer routing. Narrow nonzero rows remain interactive because the
/// selected or focused tab clips to the available width.
///
/// ```
/// use ratcn::{Tab, Tabs};
///
/// # #[derive(Clone, Copy, PartialEq)]
/// # enum Screen { Overview, Settings }
/// # struct AppState { focused_tab: Screen, screen: Screen }
/// # enum Msg { TabFocused(Screen), ScreenChanged(Screen) }
/// let _tabs = Tabs::new([
///     Tab::new(Screen::Overview, "Overview"),
///     Tab::new(Screen::Settings, "Settings"),
/// ])
/// .tab_focus(|s: &AppState| Some(s.focused_tab), Msg::TabFocused)
/// .selection(|s: &AppState| Some(s.screen), Msg::ScreenChanged);
/// ```
pub struct Tabs<T, S, M> {
    items: Vec<Tab<T>>,
    /// Current semantic state bindings; events re-read these between frames.
    focused_tab: Option<ReadFn<S, T>>,
    on_focus_change: Option<OnChangeFn<T, M>>,
    selected: Option<ReadFn<S, T>>,
    on_select: Option<OnChangeFn<T, M>>,
    activation: TabsActivation,
    size: TabsSize,
    style: Option<StyleFn>,
    disabled: bool,
    /// Per-tab geometry from the last render as `(label index, rect)`, for
    /// mapping a click to a tab. Hidden tabs mean a tab's screen slot is not
    /// always its label index.
    hits: TabLayout,
}

impl<T, S, M> fmt::Debug for Tabs<T, S, M>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tabs")
            .field("items", &self.items)
            .field("focused_tab", &self.focused_tab.is_some())
            .field("on_focus_change", &self.on_focus_change.is_some())
            .field("selected", &self.selected.is_some())
            .field("on_select", &self.on_select.is_some())
            .field("activation", &self.activation)
            .field("size", &self.size)
            .field("style", &self.style.is_some())
            .finish_non_exhaustive()
    }
}

impl<T, S, M> Tabs<T, S, M> {
    /// A row of `tabs`, with no bindings yet.
    ///
    /// Accepts anything that converts into a [`Tab`], so `["One", "Two"]` works
    /// for a quick row of strings — each label doubling as its value — and
    /// `[Tab::new(value, label), ...]` for anything keyed by a real value.
    #[must_use]
    pub fn new(tabs: impl IntoIterator<Item = impl Into<Tab<T>>>) -> Self {
        Self {
            items: tabs.into_iter().map(Into::into).collect(),
            focused_tab: None,
            on_focus_change: None,
            selected: None,
            on_select: None,
            activation: TabsActivation::Manual,
            size: TabsSize::Small,
            style: None,
            disabled: false,
            hits: TabLayout::default(),
        }
    }

    /// Bind the cursor: which tab is highlighted, and what message moves it.
    ///
    /// `read` returns the value of the tab the cursor is on, or `None` when it
    /// is nowhere yet — in which case the first arrow key moves it onto the
    /// first enabled tab. Left/Right and Home/End move it. Moving
    /// it does not switch tabs — [`selection`](Self::selection) does that, on
    /// Enter, Space, or a click.
    ///
    /// Named for tabs rather than items, unlike
    /// [`List::item_focus`](crate::List::item_focus) and
    /// [`Select::item_focus`](crate::Select::item_focus): those two share the
    /// [`ListItem`](crate::ListItem) vocabulary, while a tab is its own
    /// [`Tab`] type. The binding shape — a reader paired with its message
    /// constructor — is identical.
    ///
    /// Unlike those two, the pointer does *not* move this cursor: hovering a
    /// tab under [`TabsActivation::Automatic`] would switch the panel's
    /// content on the way past, so hover stays paint-only in both modes.
    ///
    /// Ignored under [`TabsActivation::Automatic`], which has no separate
    /// cursor.
    #[must_use]
    pub fn tab_focus(
        mut self,
        read: impl Fn(&S) -> Option<T> + 'static,
        on_change: impl Fn(T) -> M + 'static,
    ) -> Self {
        self.focused_tab = Some(Box::new(read));
        self.on_focus_change = Some(Box::new(on_change));
        self
    }

    /// Bind the active tab: which one is showing, and what message switches it.
    ///
    /// `read` returns the currently active value — the same one the app matches
    /// on to decide what to draw below the row — or `None` when no tab is
    /// active yet, in which case no tab paints as selected and the first arrow
    /// key targets the first enabled tab. `on_select` is called with the tab
    /// the user switched to.
    ///
    /// In manual activation, a pointer click can select a tab without a
    /// preceding pointer-move event. The update handling this message should
    /// store the selected value as both selection and tab focus so later
    /// keyboard input continues from the clicked tab.
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

    /// Dim the whole row and stop it responding.
    ///
    /// A disabled row is not focusable, so Tab skips it — as is a row that is
    /// empty or has every tab disabled, since there would be nothing for the
    /// cursor to land on. Disable individual tabs with [`Tab::disabled`].
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Whether arrow keys switch tabs directly or only move a cursor. See
    /// [`TabsActivation`]; defaults to `Manual`.
    #[must_use]
    pub const fn activation(mut self, activation: TabsActivation) -> Self {
        self.activation = activation;
        self
    }

    /// Set the row height, one row or three. See [`TabsSize`].
    #[must_use]
    pub const fn size(mut self, size: TabsSize) -> Self {
        self.size = size;
        self
    }

    /// Return the row height for the configured size.
    #[must_use]
    pub const fn height(&self) -> u16 {
        self.size.height()
    }

    /// The total width the row wants (every label plus its padding and the
    /// standard gaps), for building layout constraints from the same tabs that
    /// get declared — mirrors the paint widget's `width`.
    #[must_use]
    pub fn width(&self) -> u16 {
        row_width(self.items.iter().map(Tab::label))
    }

    /// Override the [`TabsStyle`], instead of the one derived from the active
    /// theme. Resolved from the theme at render time, so a style built from
    /// `theme` follows theme switches; a fixed style ignores the argument
    /// (`|_| STYLE`).
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> TabsStyle + 'static) -> Self {
        self.style = Some(Box::new(style));
        self
    }

    fn disabled_at(&self, index: usize) -> bool {
        list_core::disabled_at(&self.items, index)
    }

    /// The item index under a screen cell, from the last render's geometry.
    /// `None` if the cell is outside every visible tab.
    fn tab_at(&self, column: u16, row: u16) -> Option<usize> {
        let point = Position { x: column, y: row };
        self.hits
            .tabs
            .iter()
            .find(|(_, rect)| rect.contains(point))
            .map(|(index, _)| *index)
    }

    fn marker_at(&self, column: u16, row: u16) -> Option<Step> {
        let point = Position { x: column, y: row };
        if self.hits.left.is_some_and(|rect| rect.contains(point)) {
            Some(Step::Backward)
        } else if self.hits.right.is_some_and(|rect| rect.contains(point)) {
            Some(Step::Forward)
        } else {
            None
        }
    }
}

/// Which way a key steps along a tab strip, or `None` if it does not step.
///
/// The horizontal counterpart of the vertical map in
/// [`linear_nav`](crate::linear_nav): Left/Right for everyone, `h`/`l` for
/// `vi`, and Ctrl+P/Ctrl+N for readline. A tab strip is a row, so `h`/`l` are
/// its `vi` keys rather than `j`/`k`.
fn step_direction(key: KeyEvent) -> Option<Step> {
    if key.modifiers.alt || key.modifiers.shift {
        return None;
    }
    if key.modifiers.ctrl {
        return match key.code {
            KeyCode::Char('n') => Some(Step::Forward),
            KeyCode::Char('p') => Some(Step::Backward),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => Some(Step::Backward),
        KeyCode::Right | KeyCode::Char('l') => Some(Step::Forward),
        _ => None,
    }
}

impl<T, S, M> Tabs<T, S, M>
where
    T: Clone + PartialEq,
{
    fn index_of(&self, value: &T) -> Option<usize> {
        list_core::index_of(&self.items, value)
    }

    /// The index of the selected tab, or `None` when nothing is selected yet or
    /// the stored value matches no tab (render/route defensively).
    fn selected_index(&self, state: &S) -> Option<usize> {
        let value = (self.selected.as_ref()?)(state)?;
        self.index_of(&value)
    }

    /// The mode-correct tab-local cursor. Automatic activation uses selection;
    /// manual activation uses its persistent tab-focus binding.
    fn cursor_index(&self, state: &S) -> Option<usize> {
        match self.activation {
            TabsActivation::Automatic => self.selected_index(state),
            TabsActivation::Manual => self
                .focused_tab
                .as_ref()
                .and_then(|read| self.index_of(&read(state)?)),
        }
    }

    fn keyboard_enabled(&self) -> bool {
        self.selected.is_some()
            && match self.activation {
                TabsActivation::Manual => self.focused_tab.is_some(),
                TabsActivation::Automatic => true,
            }
    }

    /// Commit the tab at `index` as the selection when wired.
    fn select(&self, index: usize) -> EventResult<M> {
        match &self.on_select {
            Some(on_select) => EventResult::Emit(on_select(self.items[index].value().clone())),
            None => EventResult::Ignored,
        }
    }

    /// Move focus to the tab at `index`, or let the event bubble when no focus
    /// handler is wired.
    fn move_focus(&self, index: usize) -> EventResult<M> {
        match &self.on_focus_change {
            Some(on_focus_change) => {
                EventResult::Emit(on_focus_change(self.items[index].value().clone()))
            }
            None => EventResult::Ignored,
        }
    }

    /// Map one key against the tab row.
    ///
    /// The tabs row runs horizontally, so the key map is local rather than
    /// [`linear_nav::nav_key_target`]: that helper steps with Up and Down, and
    /// pages with `PageUp` and `PageDown`, neither of which suits a row. Here
    /// Left and Right step by one tab, and Home and End jump to the first and
    /// last enabled tab. There is no Page-key navigation on `Tabs` — `PageUp`
    /// and `PageDown` are ignored and bubble as app hotkeys. The index
    /// arithmetic underneath is still [`linear_nav`]'s, including the
    /// `first_enabled`/`last_enabled` that Home and End call.
    fn handle_key(&self, key: KeyEvent, state: &S) -> EventResult<M> {
        if !self.keyboard_enabled() {
            return EventResult::Ignored;
        }
        let len = self.items.len();
        if linear_nav::first_enabled(len, |index| self.disabled_at(index)).is_none() {
            return EventResult::Ignored;
        }
        let cursor = self.cursor_index(state);
        // Stepping is asked first because it owns the Ctrl chords that the
        // modifier gate below rejects.
        if let Some(direction) = step_direction(key) {
            let index = cursor.map_or_else(
                || linear_nav::first_enabled(len, |i| self.disabled_at(i)).expect("checked above"),
                |from| linear_nav::step_enabled(len, from, direction, |i| self.disabled_at(i)),
            );
            return self.apply_target(Some(index), cursor);
        }
        if linear_nav::has_reserved_modifier(key) {
            return EventResult::Ignored;
        }
        let next = match key.code {
            KeyCode::Home => linear_nav::first_enabled(len, |i| self.disabled_at(i)),
            KeyCode::End => linear_nav::last_enabled(len, |i| self.disabled_at(i)),
            KeyCode::Enter | KeyCode::Char(' ') => {
                return match cursor {
                    Some(index) if self.disabled_at(index) => EventResult::Ignored,
                    Some(index) => self.select(index),
                    None => EventResult::Ignored,
                };
            }
            // Anything else — including every letter but `h` and `l` — bubbles,
            // so the app keeps its single-key hotkeys while tabs have focus.
            _ => return EventResult::Ignored,
        };
        self.apply_target(next, cursor)
    }

    /// Commit a resolved navigation target under the configured activation
    /// mode: manual moves the cursor, automatic also selects.
    fn apply_target(&self, next: Option<usize>, cursor: Option<usize>) -> EventResult<M> {
        match next {
            Some(index) if Some(index) != cursor => match self.activation {
                TabsActivation::Manual => self.move_focus(index),
                TabsActivation::Automatic => self.select(index),
            },
            // A resolved-but-unchanged target (already at the edge, or Home/End
            // onto the current tab) is still ours to swallow.
            Some(_) => EventResult::Consumed,
            // Nothing to move to (no current focused tab, or every tab disabled).
            None => EventResult::Ignored,
        }
    }
}

impl<T, S, M> Component<S, M> for Tabs<T, S, M>
where
    T: Clone + PartialEq,
{
    fn prepare(&mut self, _state: &S) {
        // Quadratic in the tab count and re-derived on every frame's fresh
        // instance, so a release build takes the tabs on trust.
        if cfg!(debug_assertions) {
            list_core::assert_unique_values(self.items.iter().map(Tab::value), "Tabs");
        }
    }

    fn render(&mut self, ctx: &mut RenderCtx<'_, '_, S, M>) {
        let state = ctx.state();
        let selected = self.selected_index(state);
        let cursor = self.cursor_index(state);
        let labels: Vec<&str> = self.items.iter().map(Tab::label).collect();
        // The row is measured once, here: paint takes this same layout, so the
        // geometry a click hit-tests against is the geometry that was drawn.
        let must_show = cursor.or(selected).unwrap_or(0);
        self.hits = tab_layout(ctx.area(), &labels, self.size, must_show);
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, S>) {
        let area = ctx.area();
        let state = ctx.state();
        let selected = self.selected_index(state);
        let cursor = self.cursor_index(state);
        let labels: Vec<&str> = self.items.iter().map(Tab::label).collect();
        let disabled: Vec<bool> = (0..self.items.len()).map(|i| self.disabled_at(i)).collect();
        let hovered_tab = ctx.hover_position().and_then(|position| {
            self.hits
                .tabs
                .iter()
                .find(|(_, rect)| rect.contains(position))
                .map(|(index, _)| *index)
        });
        let style = resolve_style(self.style.as_deref(), ctx.theme, TabsStyle::from_theme);
        let widget = TabsWidget::new(&labels)
            .selected_tab(selected)
            .focused_tab(cursor)
            .disabled_tabs(&disabled)
            .focused(ctx.focused)
            .hovered_tab(hovered_tab)
            .disabled(self.disabled)
            .size(self.size)
            .style(style)
            .layout(&self.hits);
        ctx.render_widget(widget, area);
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
                // The runtime focuses an unhandled primary down on the hit
                // component. Consume disabled tabs so that fallback cannot
                // focus this row when its disabled content was clicked.
                MouseKind::Down(MouseButton::Left) => match self.tab_at(mouse.column, mouse.row) {
                    Some(index) if self.disabled_at(index) => EventResult::Consumed,
                    _ => EventResult::Ignored,
                },
                // Hover is paint-only here, unlike `List` and `Select`, where
                // it moves the cursor: under `TabsActivation::Automatic` the
                // cursor is the selection, so hovering would switch the
                // panel's content on the way past. A tabs row with nothing
                // bound lets the motion through, as its siblings do.
                MouseKind::Moved => {
                    if self.keyboard_enabled() && self.tab_at(mouse.column, mouse.row).is_some() {
                        EventResult::Consumed
                    } else {
                        EventResult::Ignored
                    }
                }
                // A click commits the clicked tab. Down is left for the runtime to
                // focus the row itself (the mouse counterpart of Tab).
                MouseKind::Click(MouseButton::Left) => {
                    if let Some(index) = self.tab_at(mouse.column, mouse.row) {
                        return if self.disabled_at(index) {
                            EventResult::Ignored
                        } else {
                            self.select(index)
                        };
                    }
                    let Some(direction) = self.marker_at(mouse.column, mouse.row) else {
                        return EventResult::Ignored;
                    };
                    let edge = match direction {
                        Step::Backward => self.hits.tabs.first(),
                        Step::Forward => self.hits.tabs.last(),
                    };
                    let Some((index, _)) = edge else {
                        return EventResult::Ignored;
                    };
                    let target =
                        linear_nav::step_enabled(self.items.len(), *index, direction, |index| {
                            self.disabled_at(index)
                        });
                    match self.activation {
                        TabsActivation::Manual => self.move_focus(target),
                        TabsActivation::Automatic => self.select(target),
                    }
                }
                _ => EventResult::Ignored,
            },
            Event::Key(key) => self.handle_key(*key, state),
            _ => EventResult::Ignored,
        }
    }

    fn interaction_area(&self, area: Rect) -> Rect {
        fixed_height(area, self.height())
    }

    fn is_focusable(&self, _state: &S) -> bool {
        !self.disabled
            && self.keyboard_enabled()
            && linear_nav::has_enabled(self.items.len(), |index| self.disabled_at(index))
    }
}

impl<T: Clone + PartialEq, S, M> MeasuredComponent<S, M> for Tabs<T, S, M> {
    fn measure(&self) -> Size {
        Size::new(self.width(), self.height())
    }
}

/// The width one tab occupies: its label plus the padding on each side (label +
/// 4, a button's width). Label width is counted in cells, matching the other
/// components.
fn tab_width(label: &str) -> u16 {
    shape_width(label)
}

/// Where the tabs of one row sit on screen: the outcome of measuring the labels
/// against an area, and the single source of tab geometry.
///
/// [`tab_layout`] produces one. [`TabsWidget::layout`] paints from one, and
/// [`Tabs`] hit-tests clicks against the same one it painted, so a click and the
/// tab under it cannot disagree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabLayout {
    /// Each visible tab as `(label index, rect)`. The index is the tab's original
    /// position. Hidden tabs mean this index may differ from the tab's screen slot.
    pub tabs: Vec<(usize, Rect)>,
    /// The `‹` marker's cell, present only when tabs are hidden to the left.
    pub left: Option<Rect>,
    /// The `›` marker's cell, present only when tabs are hidden to the right.
    pub right: Option<Rect>,
}

#[derive(Debug, Clone, Copy)]
struct TabWindow {
    lo: usize,
    hi: usize,
    width: u16,
}

/// Prefix sums of each tab's width plus the gap that follows it, one entry
/// longer than `widths`, so the width of a run of tabs is one subtraction.
/// Summed wider than `u16` so the sums themselves cannot wrap; a run is clamped
/// back to `u16` where it is read.
fn width_prefix_sums(widths: &[u16]) -> Vec<u64> {
    let mut sums = Vec::with_capacity(widths.len() + 1);
    let mut total = 0;
    sums.push(total);
    for &tab_width in widths {
        total += u64::from(tab_width) + u64::from(SPACING);
        sums.push(total);
    }
    sums
}

/// The width a run of tabs `lo..=hi` occupies: their widths plus the gaps
/// between them, from the prefix sums of [`width_prefix_sums`]. Saturates like
/// the rest of the row arithmetic: no term is negative, so clamping the exact
/// sum lands on the same width as adding the run up saturatingly.
fn window_width(sums: &[u64], lo: usize, hi: usize) -> u16 {
    // The run carries the gap after its last tab; the gaps *between* tabs are
    // one fewer.
    let run = sums[hi + 1] - sums[lo] - u64::from(SPACING);
    u16::try_from(run).unwrap_or(u16::MAX)
}

/// Choose the first and last visible tab indexes. The chosen group always
/// includes `must_show` (the selected/focused tab) and then adds neighboring
/// tabs while they fit, leaving room for `‹` or `›` markers when tabs are hidden.
fn tab_window(widths: &[u16], must_show: usize, avail: u16) -> TabWindow {
    let n = widths.len();
    let sums = width_prefix_sums(widths);
    let fits = |lo: usize, hi: usize| {
        let mut width = window_width(&sums, lo, hi);
        if lo > 0 {
            width = width.saturating_add(SPACING + MARKER_WIDTH);
        }
        if hi < n - 1 {
            width = width.saturating_add(SPACING + MARKER_WIDTH);
        }
        width <= avail
    };

    let (mut lo, mut hi) = (must_show, must_show);
    let mut prefer_right = true;
    loop {
        let grow_right = hi + 1 < n && fits(lo, hi + 1);
        let grow_left = lo > 0 && fits(lo - 1, hi);
        // Alternate sides so the visible tabs stay roughly centered on must_show;
        // fall through to the other side when the preferred one is exhausted.
        if prefer_right && grow_right {
            hi += 1;
        } else if !prefer_right && grow_left {
            lo -= 1;
        } else if grow_right {
            hi += 1;
        } else if grow_left {
            lo -= 1;
        } else {
            break;
        }
        prefer_right = !prefer_right;
    }
    TabWindow {
        lo,
        hi,
        width: window_width(&sums, lo, hi),
    }
}

/// Lay out the visible tabs left to right, with a [`SPACING`] gap between them.
/// Add a `‹` or `›` marker on any side with hidden tabs. If the row is too
/// narrow for the selected/focused tab and its markers, the tab wins and the
/// markers are dropped; if that tab is wider than the row, it clips.
///
/// Called only from [`tab_layout`], after it has checked that there is at least
/// one tab, the row is tall enough, and the indexes are in range.
fn place_tab_window(
    area: Rect,
    widths: &[u16],
    size: TabsSize,
    must_show: usize,
    window: TabWindow,
) -> TabLayout {
    let height = size.height();
    let n = widths.len();

    let mut left_marker = window.lo > 0;
    let mut right_marker = window.hi < n - 1;
    let mut needed = window.width;
    if left_marker {
        needed = needed.saturating_add(SPACING + MARKER_WIDTH);
    }
    if right_marker {
        needed = needed.saturating_add(SPACING + MARKER_WIDTH);
    }
    // The selected/focused tab takes priority over the markers when the row is
    // too narrow for both. Showing that tab is more useful than only a marker.
    if needed > area.width {
        left_marker = false;
        right_marker = false;
    }

    let end = area.x.saturating_add(area.width);
    let mut x = area.x;

    let left = left_marker.then(|| {
        let rect = Rect::new(x, area.y, MARKER_WIDTH, height);
        x = x.saturating_add(MARKER_WIDTH + SPACING);
        rect
    });

    let mut tabs = Vec::with_capacity(window.hi - window.lo + 1);
    for (index, &tab_width) in (window.lo..=window.hi).zip(&widths[window.lo..=window.hi]) {
        if x >= end {
            break;
        }
        let available = end - x;
        // Only the selected/focused tab may clip; another tab that no longer fits
        // ends the row.
        let width = if index == must_show {
            tab_width.min(available)
        } else if tab_width <= available {
            tab_width
        } else {
            break;
        };
        tabs.push((index, Rect::new(x, area.y, width, height)));
        x = x.saturating_add(width).saturating_add(SPACING);
    }

    let right = (right_marker && x.saturating_add(MARKER_WIDTH) <= end)
        .then(|| Rect::new(x, area.y, MARKER_WIDTH, height));

    TabLayout { tabs, left, right }
}

/// Measure the tabs, choose which indexes are visible around `must_show`, and
/// lay them out.
///
/// `must_show` is the tab that must stay on screen — the cursor tab, or the
/// selected one when there is no cursor. When the labels are wider than `area`,
/// the visible group is grown outward from it and `‹`/`›` markers mark the sides
/// with hidden tabs. An out-of-range `must_show` is clamped to the last tab.
///
/// An empty row, a zero-width area, or one shorter than `size` lays out nothing.
///
/// A standalone [`TabsWidget`] caller need not call this — the widget measures
/// its own row when given no layout. Call it when something else must agree with
/// the paint cell for cell, and hand the result to
/// [`TabsWidget::layout`](TabsWidget::layout).
#[must_use]
pub fn tab_layout(area: Rect, labels: &[&str], size: TabsSize, must_show: usize) -> TabLayout {
    let height = size.height();
    if labels.is_empty() || area.width == 0 || area.height < height {
        return TabLayout::default();
    }
    let widths: Vec<u16> = labels.iter().map(|label| tab_width(label)).collect();
    let must_show = must_show.min(widths.len() - 1);
    let window = tab_window(&widths, must_show, area.width);
    place_tab_window(area, &widths, size, must_show, window)
}

/// The visible tab rects (dropping the label indices), starting at the first
/// tab. Test-only convenience over [`tab_layout`].
#[cfg(test)]
fn tab_rects(area: Rect, labels: &[&str], size: TabsSize) -> Vec<Rect> {
    tab_layout(area, labels, size, 0)
        .tabs
        .into_iter()
        .map(|(_, rect)| rect)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::runtime::{ChildId, Modifiers, MouseEvent, Ratcn};
    use crate::text_width::display_width;

    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    enum Screen {
        #[default]
        A,
        B,
        C,
        D,
        E,
        F,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Msg {
        Focused(Screen),
        Selected(Screen),
    }

    #[derive(Default)]
    struct State {
        focused: Option<Screen>,
        selected: Screen,
        disable_b: bool,
    }

    /// Automatic activation: arrow keys select immediately. Middle tab disabled.
    fn automatic() -> Tabs<Screen, State, Msg> {
        Tabs::new([
            Tab::new(Screen::A, "A"),
            Tab::new(Screen::B, "B").disabled(true),
            Tab::new(Screen::C, "C"),
        ])
        .selection(|s: &State| Some(s.selected), Msg::Selected)
        .activation(TabsActivation::Automatic)
    }

    fn automatic_with_tab_focus() -> Tabs<Screen, State, Msg> {
        automatic().tab_focus(
            |state: &State| Some(state.focused.unwrap_or(state.selected)),
            Msg::Focused,
        )
    }

    /// Manual wiring: focused tab and selection are separate, as on `List`.
    fn manual() -> Tabs<Screen, State, Msg> {
        Tabs::new([
            Tab::new(Screen::A, "A"),
            Tab::new(Screen::B, "B").disabled(true),
            Tab::new(Screen::C, "C"),
        ])
        .tab_focus(
            |s: &State| Some(s.focused.unwrap_or(s.selected)),
            Msg::Focused,
        )
        .selection(|s: &State| Some(s.selected), Msg::Selected)
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code))
    }

    fn ctrl_key(ch: char) -> Event {
        Event::Key(KeyEvent {
            code: KeyCode::Char(ch),
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        })
    }

    fn mouse(kind: MouseKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: Modifiers::NONE,
        })
    }

    fn render_runtime(
        ratcn: &mut Ratcn<State, Msg>,
        terminal: &mut Terminal<TestBackend>,
        state: &State,
        fail: bool,
    ) {
        let theme = Theme::default_dark();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("tabs"),
                        Tabs::new([
                            Tab::new(Screen::A, "A"),
                            Tab::new(Screen::B, "B").disabled(state.disable_b),
                            Tab::new(Screen::C, "C"),
                        ])
                        .tab_focus(
                            |state: &State| Some(state.focused.unwrap_or(state.selected)),
                            Msg::Focused,
                        )
                        .selection(|state: &State| Some(state.selected), Msg::Selected),
                        area,
                    );
                    assert!(!fail, "failed pass");
                });
            })
            .expect("draw");
    }

    #[test]
    fn automatic_right_selects_the_next_enabled_tab_directly() {
        let mut tabs = automatic();
        let state = State::default(); // selected A
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Right), &state, &mut EventCtx::default(),),
            EventResult::Emit(Msg::Selected(Screen::C))
        );
    }

    #[test]
    fn automatic_keyboard_ignores_a_stale_manual_tab_focus() {
        let mut tabs = automatic_with_tab_focus();
        let state = State {
            focused: Some(Screen::C),
            selected: Screen::A,
            ..State::default()
        };

        assert_eq!(
            tabs.handle_event(&key(KeyCode::Enter), &state, &mut EventCtx::default()),
            EventResult::Emit(Msg::Selected(Screen::A))
        );
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Char(' ')), &state, &mut EventCtx::default()),
            EventResult::Emit(Msg::Selected(Screen::A))
        );
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Right), &state, &mut EventCtx::default()),
            EventResult::Emit(Msg::Selected(Screen::C))
        );
    }

    #[test]
    fn manual_without_focus_binding_is_not_focusable_and_ignores_keys() {
        let mut tabs = Tabs::new([
            Tab::new(Screen::A, "A"),
            Tab::new(Screen::B, "B").disabled(true),
            Tab::new(Screen::C, "C"),
        ])
        .selection(|s: &State| Some(s.selected), Msg::Selected);
        let state = State::default();
        assert!(!tabs.is_focusable(&state));
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Right), &state, &mut EventCtx::default(),),
            EventResult::Ignored
        );
    }

    #[test]
    fn manual_without_selection_binding_is_not_focusable_and_ignores_keys() {
        let mut tabs = Tabs::new([Tab::new(Screen::A, "A"), Tab::new(Screen::B, "B")])
            .tab_focus(|_: &State| Some(Screen::A), Msg::Focused);
        let state = State::default();

        assert!(!tabs.is_focusable(&state));
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Right), &state, &mut EventCtx::default()),
            EventResult::Ignored
        );
    }

    #[test]
    fn automatic_without_selection_is_not_focusable_and_ignores_keys() {
        let mut tabs: Tabs<Screen, State, Msg> =
            Tabs::new([Tab::new(Screen::A, "A"), Tab::new(Screen::B, "B")])
                .activation(TabsActivation::Automatic);
        let state = State::default();

        assert!(!tabs.is_focusable(&state));
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Right), &state, &mut EventCtx::default()),
            EventResult::Ignored
        );
    }

    #[test]
    fn manual_right_moves_focus_not_the_selection() {
        let mut tabs = manual();
        let state = State::default(); // focused A, selected A
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Right), &state, &mut EventCtx::default(),),
            EventResult::Emit(Msg::Focused(Screen::C))
        );
    }

    #[test]
    fn stale_manual_focus_recovers_to_first_enabled_tab_in_either_direction() {
        let state = State {
            focused: Some(Screen::B),
            ..State::default()
        };
        for code in [KeyCode::Left, KeyCode::Right] {
            let mut tabs = Tabs::new([
                Tab::new(Screen::C, "C").disabled(true),
                Tab::new(Screen::A, "A"),
                Tab::new(Screen::D, "D"),
            ])
            .tab_focus(
                |state: &State| Some(state.focused.unwrap_or(state.selected)),
                Msg::Focused,
            )
            .selection(|state: &State| Some(state.selected), Msg::Selected);

            assert_eq!(
                tabs.handle_event(&key(code), &state, &mut EventCtx::default()),
                EventResult::Emit(Msg::Focused(Screen::A))
            );
        }
    }

    #[test]
    fn stale_automatic_selection_recovers_to_first_enabled_tab_in_either_direction() {
        let state = State {
            selected: Screen::B,
            ..State::default()
        };
        for code in [KeyCode::Left, KeyCode::Right] {
            let mut tabs = Tabs::new([
                Tab::new(Screen::C, "C").disabled(true),
                Tab::new(Screen::A, "A"),
                Tab::new(Screen::D, "D"),
            ])
            .selection(|state: &State| Some(state.selected), Msg::Selected)
            .activation(TabsActivation::Automatic);

            assert_eq!(
                tabs.handle_event(&key(code), &state, &mut EventCtx::default()),
                EventResult::Emit(Msg::Selected(Screen::A))
            );
        }
    }

    #[test]
    fn enter_commits_the_focused_tab() {
        let mut tabs = manual();
        let state = State {
            focused: Some(Screen::C),
            selected: Screen::A,
            ..State::default()
        };
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Enter), &state, &mut EventCtx::default(),),
            EventResult::Emit(Msg::Selected(Screen::C))
        );
    }

    #[test]
    fn enter_on_a_disabled_focused_tab_is_ignored() {
        let mut tabs = manual();
        let state = State {
            focused: Some(Screen::B),
            selected: Screen::A,
            ..State::default()
        };

        assert_eq!(
            tabs.handle_event(&key(KeyCode::Enter), &state, &mut EventCtx::default(),),
            EventResult::Ignored
        );
    }

    #[test]
    fn hover_is_consumed_without_moving_focus() {
        let mut tabs = manual();
        tabs.hits = tab_layout(
            Rect::new(0, 0, 30, TabsSize::Small.height()),
            &["A", "B", "C"],
            TabsSize::Small,
            0,
        );
        let state = State::default();
        // Tabs are label+4 wide with a 1-cell gap: A 0..5, B 6..11, C 12..17.
        assert_eq!(
            tabs.handle_event(
                &mouse(MouseKind::Moved, 14, 0),
                &state,
                &mut EventCtx::default(),
            ),
            EventResult::Consumed
        );
    }

    #[test]
    fn automatic_hover_ignores_a_manual_tab_focus_binding() {
        let mut tabs = automatic_with_tab_focus();
        tabs.hits = tab_layout(
            Rect::new(0, 0, 30, TabsSize::Small.height()),
            &["A", "B", "C"],
            TabsSize::Small,
            0,
        );
        let state = State::default();
        // Hover must never move a separate cursor or switch content.
        assert_eq!(
            tabs.handle_event(
                &mouse(MouseKind::Moved, 14, 0),
                &state,
                &mut EventCtx::default(),
            ),
            EventResult::Consumed
        );
    }

    /// A tab strip is horizontal, so its `vi` keys are `h`/`l` rather than
    /// `j`/`k`. Ctrl+P/Ctrl+N read as previous/next either way.
    #[test]
    fn vim_and_readline_keys_step_along_the_strip() {
        let state = State::default(); // cursor on A
        for forward in [key(KeyCode::Char('l')), ctrl_key('n')] {
            assert_eq!(
                manual().handle_event(&forward, &state, &mut EventCtx::default()),
                EventResult::Emit(Msg::Focused(Screen::C)),
                "l and Ctrl+N step like Right, skipping the disabled B"
            );
            assert_eq!(
                automatic().handle_event(&forward, &state, &mut EventCtx::default()),
                EventResult::Emit(Msg::Selected(Screen::C)),
                "automatic activation commits, as its arrow keys do"
            );
        }
        assert_eq!(
            manual().handle_event(&key(KeyCode::Char('h')), &state, &mut EventCtx::default()),
            EventResult::Consumed,
            "h steps like Left, and the cursor is already at the first tab"
        );
        assert_eq!(
            manual().handle_event(&key(KeyCode::Char('c')), &state, &mut EventCtx::default()),
            EventResult::Ignored,
            "any other letter bubbles: there is no typeahead"
        );
    }

    #[test]
    fn a_letter_that_is_not_a_navigation_key_bubbles_as_an_app_hotkey() {
        let state = State::default();
        for ch in ['b', 'z'] {
            assert_eq!(
                manual().handle_event(&key(KeyCode::Char(ch)), &state, &mut EventCtx::default()),
                EventResult::Ignored,
                "'{ch}' is not a navigation key, so it bubbles as an app hotkey"
            );
        }
    }

    /// A tabs row with nothing bound lets pointer motion through, as `List`
    /// and `Select` do.
    #[test]
    fn hover_over_an_unbound_tabs_row_is_ignored() {
        let mut tabs: Tabs<Screen, State, Msg> = Tabs::new([
            Tab::new(Screen::A, "A"),
            Tab::new(Screen::B, "B"),
            Tab::new(Screen::C, "C"),
        ]);
        tabs.hits = tab_layout(
            Rect::new(0, 0, 30, TabsSize::Small.height()),
            &["A", "B", "C"],
            TabsSize::Small,
            0,
        );
        let state = State::default();
        assert_eq!(
            tabs.handle_event(
                &mouse(MouseKind::Moved, 14, 0),
                &state,
                &mut EventCtx::default(),
            ),
            EventResult::Ignored
        );
    }

    #[test]
    fn click_commits_the_clicked_tab() {
        let mut tabs = automatic();
        tabs.hits = tab_layout(
            Rect::new(0, 0, 30, TabsSize::Small.height()),
            &["A", "B", "C"],
            TabsSize::Small,
            0,
        );
        let state = State::default();
        // C occupies columns 12..17.
        assert_eq!(
            tabs.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), 14, 0),
                &state,
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Selected(Screen::C))
        );
    }

    #[test]
    fn right_and_middle_click_do_not_select_a_tab() {
        let mut tabs = automatic();
        tabs.hits = TabLayout {
            tabs: vec![(2, Rect::new(12, 0, 5, 1))],
            ..TabLayout::default()
        };

        for button in [MouseButton::Right, MouseButton::Middle] {
            assert_eq!(
                tabs.handle_event(
                    &mouse(MouseKind::Click(button), 14, 0),
                    &State::default(),
                    &mut EventCtx::default(),
                ),
                EventResult::Ignored
            );
        }
    }

    #[test]
    fn click_after_window_shift_selects_the_original_tab_index() {
        // Six tabs in 18 cells cannot all show, so the window shifts to keep
        // the selected E visible. The click must still resolve to F's *original*
        // index rather than its position within the visible window.
        let labels = ["A", "B", "C", "D", "E", "F"];
        let state = State {
            selected: Screen::E,
            ..State::default()
        };
        let theme = Theme::default_dark();
        let mut ratcn = Ratcn::new();
        let area = Rect::new(0, 0, 18, TabsSize::Small.height());
        let mut terminal =
            Terminal::new(TestBackend::new(area.width, area.height)).expect("terminal");

        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("tabs"),
                        Tabs::new([
                            Tab::new(Screen::A, "A"),
                            Tab::new(Screen::B, "B"),
                            Tab::new(Screen::C, "C"),
                            Tab::new(Screen::D, "D"),
                            Tab::new(Screen::E, "E"),
                            Tab::new(Screen::F, "F"),
                        ])
                        .selection(|s: &State| Some(s.selected), Msg::Selected),
                        area,
                    );
                });
            })
            .expect("draw");

        // The same layout the render just computed: `must_show` is the selected
        // tab when no manual cursor is bound.
        let hits = tab_layout(area, &labels, TabsSize::Small, 4);
        let (_, tab_f) = hits
            .tabs
            .iter()
            .find(|(index, _)| *index == 5)
            .expect("tab F should be visible after the window shifts");

        assert_eq!(
            ratcn.handle_event(
                mouse(
                    MouseKind::Click(MouseButton::Left),
                    tab_f.x + tab_f.width / 2,
                    tab_f.y,
                ),
                &state,
            ),
            EventResult::Emit(Msg::Selected(Screen::F))
        );
    }

    #[test]
    fn clicking_the_right_marker_steps_toward_the_hidden_tabs() {
        // Six 5-wide tabs in 18 cells around tab A: A and B are visible and the
        // rest hide behind a `›` marker. Clicking it steps the manual cursor
        // one tab past the last visible one, toward the hidden side.
        let mut tabs = Tabs::new([
            Tab::new(Screen::A, "A"),
            Tab::new(Screen::B, "B"),
            Tab::new(Screen::C, "C"),
            Tab::new(Screen::D, "D"),
            Tab::new(Screen::E, "E"),
            Tab::new(Screen::F, "F"),
        ])
        .tab_focus(
            |state: &State| Some(state.focused.unwrap_or(state.selected)),
            Msg::Focused,
        )
        .selection(|state: &State| Some(state.selected), Msg::Selected);
        tabs.hits = tab_layout(
            Rect::new(0, 0, 18, TabsSize::Small.height()),
            &["A", "B", "C", "D", "E", "F"],
            TabsSize::Small,
            0,
        );
        let marker = tabs.hits.right.expect("hidden right tabs get a marker");

        assert_eq!(
            tabs.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), marker.x, marker.y),
                &State::default(),
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Focused(Screen::C)),
            "the marker steps focus one tab past the last visible one"
        );
    }

    #[test]
    fn marker_clicks_route_through_the_retained_runtime_surface() {
        // The same marker click, but delivered through `Ratcn::handle_event`
        // against the geometry retained by a real render — automatic
        // activation, so the step selects the hidden neighbor directly.
        let mut ratcn = Ratcn::new();
        let mut terminal =
            Terminal::new(TestBackend::new(18, TabsSize::Small.height())).expect("terminal");
        let theme = Theme::default_dark();
        let state = State::default(); // selected A
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("tabs"),
                        Tabs::new([
                            Tab::new(Screen::A, "A"),
                            Tab::new(Screen::B, "B"),
                            Tab::new(Screen::C, "C"),
                            Tab::new(Screen::D, "D"),
                            Tab::new(Screen::E, "E"),
                            Tab::new(Screen::F, "F"),
                        ])
                        .selection(|state: &State| Some(state.selected), Msg::Selected)
                        .activation(TabsActivation::Automatic),
                        area,
                    );
                });
            })
            .expect("draw");
        let marker = tab_layout(
            Rect::new(0, 0, 18, TabsSize::Small.height()),
            &["A", "B", "C", "D", "E", "F"],
            TabsSize::Small,
            0,
        )
        .right
        .expect("hidden right tabs get a marker");

        assert_eq!(
            ratcn.handle_event(
                mouse(MouseKind::Click(MouseButton::Left), marker.x, marker.y),
                &state,
            ),
            EventResult::Emit(Msg::Selected(Screen::C))
        );
    }

    #[test]
    fn click_on_a_disabled_tab_is_ignored() {
        let mut tabs = automatic();
        tabs.hits = tab_layout(
            Rect::new(0, 0, 30, TabsSize::Small.height()),
            &["A", "B", "C"],
            TabsSize::Small,
            0,
        );
        let state = State::default();
        // Column 8 is inside the disabled middle tab (x = 6..11).
        assert_eq!(
            tabs.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), 8, 0),
                &state,
                &mut EventCtx::default(),
            ),
            EventResult::Ignored
        );
    }

    #[test]
    fn disabled_tabs_consume_pointer_down_without_focusing_the_row() {
        #[derive(Default)]
        struct RoutedState {
            focus: crate::runtime::FocusState,
            selected: Screen,
        }

        #[derive(Debug, PartialEq)]
        enum RoutedMsg {
            Focus(crate::runtime::FocusState),
            Selected(Screen),
        }

        let theme = Theme::default_dark();
        let mut state = RoutedState {
            focus: crate::runtime::FocusState::intent([ChildId::Static("other")]),
            ..RoutedState::default()
        };
        let mut ratcn = Ratcn::new().focus(|state: &RoutedState| &state.focus, RoutedMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(20, 2)).expect("terminal");
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("other"),
                        crate::Button::<RoutedMsg>::new("Other")
                            .on_press(|| RoutedMsg::Selected(Screen::A)),
                        Rect::new(0, 0, 20, 1),
                    );
                    ctx.render_component(
                        ChildId::Static("tabs"),
                        Tabs::new([
                            Tab::new(Screen::A, "A"),
                            Tab::new(Screen::B, "B").disabled(true),
                        ])
                        .selection(
                            |state: &RoutedState| Some(state.selected),
                            RoutedMsg::Selected,
                        )
                        .activation(TabsActivation::Automatic),
                        Rect::new(0, 1, 20, 1),
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 8, 1), &state),
            EventResult::Consumed
        );
        assert_eq!(
            state.focus,
            crate::runtime::FocusState::intent([ChildId::Static("other")])
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 8, 1), &state),
            EventResult::Ignored
        );

        let EventResult::Emit(RoutedMsg::Focus(focus)) =
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 2, 1), &state)
        else {
            panic!("an enabled tab should receive runtime focus");
        };
        state.focus = focus;
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 2, 1), &state),
            EventResult::Emit(RoutedMsg::Selected(Screen::A))
        );
    }

    #[test]
    fn excess_allocated_rows_do_not_focus_or_activate_tabs() {
        #[derive(Default)]
        struct RoutedState {
            focus: crate::runtime::FocusState,
            selected: Screen,
        }

        #[derive(Debug, PartialEq)]
        enum RoutedMsg {
            Focus(crate::runtime::FocusState),
            Selected(Screen),
        }

        let theme = Theme::default_dark();
        let mut state = RoutedState {
            focus: crate::runtime::FocusState::intent([ChildId::Static("other")]),
            ..RoutedState::default()
        };
        let mut ratcn = Ratcn::new().focus(|state: &RoutedState| &state.focus, RoutedMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(20, 4)).expect("terminal");
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("tabs"),
                        Tabs::new([Tab::new(Screen::A, "A"), Tab::new(Screen::B, "B")])
                            .selection(
                                |state: &RoutedState| Some(state.selected),
                                RoutedMsg::Selected,
                            )
                            .activation(TabsActivation::Automatic),
                        Rect::new(0, 0, 20, 4),
                    );
                    ctx.render_component(
                        ChildId::Static("other"),
                        crate::Button::<RoutedMsg>::new("Other")
                            .on_press(|| RoutedMsg::Selected(Screen::A)),
                        Rect::new(0, 3, 8, 1),
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 2, 2), &state),
            EventResult::Ignored
        );
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 2, 2), &state),
            EventResult::Ignored
        );
        assert_eq!(
            state.focus,
            crate::runtime::FocusState::intent([ChildId::Static("other")])
        );

        let EventResult::Emit(RoutedMsg::Focus(focus)) =
            ratcn.handle_event(mouse(MouseKind::Down(MouseButton::Left), 2, 0), &state)
        else {
            panic!("the painted tabs row should focus");
        };
        state.focus = focus;
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Up(MouseButton::Left), 2, 0), &state),
            EventResult::Emit(RoutedMsg::Selected(Screen::A))
        );
    }

    #[test]
    fn automatic_home_and_end_select_the_first_and_last_enabled_tab() {
        let mut tabs = automatic();
        let from_c = State {
            selected: Screen::C,
            ..State::default()
        };
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Home), &from_c, &mut EventCtx::default(),),
            EventResult::Emit(Msg::Selected(Screen::A))
        );
        assert_eq!(
            tabs.handle_event(
                &key(KeyCode::End),
                &State::default(),
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Selected(Screen::C))
        );
    }

    #[test]
    fn selection_is_stable_when_tabs_are_reordered() {
        // Value-keyed: the same stored value selects the same tab regardless of
        // position, so a click resolves by value, not index.
        let mut tabs = Tabs::new([
            Tab::new(Screen::C, "C"),
            Tab::new(Screen::A, "A"),
            Tab::new(Screen::B, "B"),
        ])
        .selection(|s: &State| Some(s.selected), Msg::Selected);
        let state = State::default(); // selected A, now at index 1
        assert_eq!(tabs.selected_index(&state), Some(1));
        tabs.hits = tab_layout(
            Rect::new(0, 0, 30, TabsSize::Small.height()),
            &["C", "A", "B"],
            TabsSize::Small,
            0,
        );
        // The first tab (C) occupies columns 0..5.
        assert_eq!(
            tabs.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), 1, 0),
                &state,
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Selected(Screen::C))
        );
    }

    #[test]
    fn a_row_of_all_disabled_tabs_is_not_focusable() {
        let mut tabs: Tabs<Screen, State, Msg> = Tabs::new([
            Tab::new(Screen::A, "A").disabled(true),
            Tab::new(Screen::B, "B").disabled(true),
        ]);
        let state = State::default();

        assert!(!tabs.is_focusable(&state));
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Right), &state, &mut EventCtx::default(),),
            EventResult::Ignored
        );
    }

    #[test]
    fn interaction_area_requires_full_height_and_crops_excess_rows() {
        let tabs: Tabs<Screen, State, Msg> =
            Tabs::new([Tab::new(Screen::A, "A")]).size(TabsSize::Large);

        assert_eq!(
            tabs.interaction_area(Rect::new(0, 0, 1, 2)),
            Rect::default()
        );
        assert_eq!(
            tabs.interaction_area(Rect::new(2, 3, 1, 5)),
            Rect::new(2, 3, 1, 3)
        );
        assert_eq!(
            tabs.interaction_area(Rect::new(0, 0, 0, 3)),
            Rect::default()
        );
    }

    #[test]
    fn disabledness_controls_navigation_clicks_and_focusability() {
        let mut tabs: Tabs<Screen, State, Msg> = Tabs::new([
            Tab::new(Screen::A, "A").disabled(true),
            Tab::new(Screen::B, "B").disabled(true),
            Tab::new(Screen::C, "C"),
        ])
        .tab_focus(
            |state: &State| Some(state.focused.unwrap_or(state.selected)),
            Msg::Focused,
        )
        .selection(|state: &State| Some(state.selected), Msg::Selected);
        tabs.hits = tab_layout(Rect::new(0, 0, 30, 1), &["A", "B", "C"], TabsSize::Small, 0);

        assert!(tabs.is_focusable(&State::default()));
        assert_eq!(
            tabs.handle_event(
                &key(KeyCode::Right),
                &State::default(),
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Focused(Screen::C))
        );
        assert_eq!(
            tabs.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), 2, 0),
                &State::default(),
                &mut EventCtx::default(),
            ),
            EventResult::Ignored
        );
        assert_eq!(
            tabs.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), 8, 0),
                &State::default(),
                &mut EventCtx::default(),
            ),
            EventResult::Ignored
        );
        assert_eq!(
            tabs.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), 14, 0),
                &State::default(),
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Selected(Screen::C))
        );

        let all_disabled: Tabs<Screen, State, Msg> = Tabs::new([
            Tab::new(Screen::A, "A").disabled(true),
            Tab::new(Screen::B, "B").disabled(true),
            Tab::new(Screen::C, "C").disabled(true),
        ]);
        assert!(!all_disabled.is_focusable(&State::default()));
    }

    #[test]
    fn disabled_bits_remain_from_the_last_successful_runtime_render() {
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).expect("terminal");
        let mut state = State::default();
        render_runtime(&mut ratcn, &mut terminal, &state, false);
        state.disable_b = true;

        assert_eq!(
            ratcn.handle_event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Focused(Screen::B))
        );

        render_runtime(&mut ratcn, &mut terminal, &state, false);
        state.disable_b = false;
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Focused(Screen::C))
        );
    }

    #[test]
    fn failed_runtime_pass_preserves_previous_disabled_bits() {
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).expect("terminal");
        let mut state = State::default();
        render_runtime(&mut ratcn, &mut terminal, &state, false);
        state.disable_b = true;

        let failed = catch_unwind(AssertUnwindSafe(|| {
            render_runtime(&mut ratcn, &mut terminal, &state, true);
        }));

        assert!(failed.is_err());
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Focused(Screen::B))
        );
    }

    #[test]
    fn focused_and_selected_bindings_are_current_between_renders() {
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).expect("terminal");
        let mut state = State::default();
        render_runtime(&mut ratcn, &mut terminal, &state, false);
        state.focused = Some(Screen::B);
        state.selected = Screen::C;

        assert_eq!(
            ratcn.handle_event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Focused(Screen::C))
        );
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::Selected(Screen::B))
        );
    }

    #[test]
    fn consecutive_automatic_navigation_uses_current_selection_without_redraw() {
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).expect("terminal");
        let theme = Theme::default_dark();
        let mut state = State::default();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("tabs"),
                        Tabs::new([
                            Tab::new(Screen::A, "A"),
                            Tab::new(Screen::B, "B"),
                            Tab::new(Screen::C, "C"),
                        ])
                        .selection(|state: &State| Some(state.selected), Msg::Selected)
                        .activation(TabsActivation::Automatic),
                        area,
                    );
                });
            })
            .expect("draw");

        assert_eq!(
            ratcn.handle_event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Selected(Screen::B))
        );
        state.selected = Screen::B;
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Selected(Screen::C))
        );
    }

    #[test]
    fn hit_window_survives_semantic_changes_and_a_failed_pass() {
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(18, 1)).expect("terminal");
        let theme = Theme::default_dark();
        let mut state = State {
            selected: Screen::E,
            ..State::default()
        };
        let render = |ratcn: &mut Ratcn<State, Msg>,
                      terminal: &mut Terminal<TestBackend>,
                      state: &State,
                      fail: bool| {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    ratcn.render(frame, state, &theme, |ctx| {
                        ctx.render_component(
                            ChildId::Static("tabs"),
                            Tabs::new([
                                Tab::new(Screen::A, "A"),
                                Tab::new(Screen::B, "B"),
                                Tab::new(Screen::C, "C"),
                                Tab::new(Screen::D, "D"),
                                Tab::new(Screen::E, "E"),
                                Tab::new(Screen::F, "F"),
                            ])
                            .selection(|state: &State| Some(state.selected), Msg::Selected),
                            area,
                        );
                        assert!(!fail, "failed pass");
                    });
                })
                .expect("draw");
        };
        render(&mut ratcn, &mut terminal, &state, false);
        let (_, tab_f) = tab_layout(
            Rect::new(0, 0, 18, 1),
            &["A", "B", "C", "D", "E", "F"],
            TabsSize::Small,
            4,
        )
        .tabs
        .into_iter()
        .find(|(index, _)| *index == 5)
        .expect("tab F is visible around selected E");

        state.selected = Screen::A;
        state.focused = Some(Screen::A);
        let failed = catch_unwind(AssertUnwindSafe(|| {
            render(&mut ratcn, &mut terminal, &state, true);
        }));

        assert!(failed.is_err());
        assert_eq!(
            ratcn.handle_event(
                mouse(
                    MouseKind::Click(MouseButton::Left),
                    tab_f.x + tab_f.width / 2,
                    tab_f.y,
                ),
                &state,
            ),
            EventResult::Emit(Msg::Selected(Screen::F))
        );
    }

    #[test]
    fn same_target_click_enter_and_space_all_select() {
        let state = State::default();
        let mut tabs = Tabs::new([Tab::new(Screen::A, "A")])
            .selection(|state: &State| Some(state.selected), Msg::Selected)
            .activation(TabsActivation::Automatic);
        tabs.hits = TabLayout {
            tabs: vec![(0, Rect::new(0, 0, 5, 1))],
            ..TabLayout::default()
        };

        assert_eq!(
            tabs.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), 2, 0),
                &state,
                &mut EventCtx::default(),
            ),
            EventResult::Emit(Msg::Selected(Screen::A))
        );
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Enter), &state, &mut EventCtx::default(),),
            EventResult::Emit(Msg::Selected(Screen::A))
        );
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Char(' ')), &state, &mut EventCtx::default(),),
            EventResult::Emit(Msg::Selected(Screen::A))
        );
    }

    #[test]
    fn automatic_render_uses_selection_instead_of_stale_manual_tab_focus() {
        let theme = Theme::default_dark();
        let style = TabsStyle::from_theme(&theme);
        let state = State {
            focused: Some(Screen::C),
            selected: Screen::A,
            ..State::default()
        };
        // The tab strip is the only focus candidate, so startup focus resolves
        // onto it and it paints focused.
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(18, 1)).expect("terminal");

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(ChildId::Static("tabs"), automatic_with_tab_focus(), area);
                });
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer.cell((2, 0)).expect("selected A label cell").bg,
            style.selected_focused_background
        );
        assert_eq!(
            buffer.cell((14, 0)).expect("unselected C label cell").bg,
            style.background
        );
    }

    #[test]
    fn selected_tab_fills_like_the_default_button_others_like_secondary() {
        let theme = Theme::default_dark();
        let style = TabsStyle::from_theme(&theme);
        let area = Rect::new(0, 0, 18, TabsSize::Small.height());
        let mut buffer = Buffer::empty(area);

        // Selected A, no focused tab. A 0..5, B 6..11 (1-cell gap).
        Widget::render(
            TabsWidget::new(&["A", "B"])
                .selected_tab(Some(0))
                .style(style),
            area,
            &mut buffer,
        );

        // The selected tab fills with the primary (default button).
        let selected = buffer.cell((2, 0)).expect("selected label cell");
        assert_eq!(selected.bg, style.selected_background);
        assert_eq!(selected.bg, theme.primary);
        // An unselected tab fills with the secondary.
        let unselected = buffer.cell((8, 0)).expect("unselected label cell");
        assert_eq!(unselected.bg, style.background);
        assert_eq!(unselected.bg, theme.secondary);
    }

    #[test]
    fn disabled_selected_tab_keeps_distinct_active_colors() {
        let theme = Theme::default_dark();
        let style = TabsStyle::from_theme(&theme);
        let area = Rect::new(0, 0, 18, TabsSize::Small.height());
        let mut buffer = Buffer::empty(area);

        Widget::render(
            TabsWidget::new(&["A", "B"])
                .selected_tab(Some(0))
                .disabled_tabs(&[true, true])
                .style(style),
            area,
            &mut buffer,
        );

        let selected = buffer.cell((2, 0)).expect("selected disabled tab");
        let unselected = buffer.cell((8, 0)).expect("unselected disabled tab");
        assert_eq!(selected.bg, style.selected_disabled_background);
        assert_eq!(unselected.bg, style.disabled_background);
        assert_ne!(selected.bg, unselected.bg);
    }

    #[test]
    fn focused_tab_takes_the_button_focus_shift() {
        let theme = Theme::default_dark();
        let style = TabsStyle::from_theme(&theme);
        let area = Rect::new(0, 0, 18, TabsSize::Small.height());
        let mut buffer = Buffer::empty(area);

        // Selected A, focused tab on the unselected B, row focused → B lightens like a
        // focused secondary button.
        Widget::render(
            TabsWidget::new(&["A", "B"])
                .selected_tab(Some(0))
                .focused_tab(Some(1))
                .focused(true)
                .style(style),
            area,
            &mut buffer,
        );

        let focused = buffer.cell((8, 0)).expect("focused label cell");
        assert_eq!(focused.bg, style.focused_background);
    }

    /// Motion inside one component still asks for a redraw. A tabs row paints
    /// its hovered tab from `PaintCtx::hover_position`, so a motion from one
    /// tab to the empty end of the row changes the frame without changing
    /// hover, without moving focus, and without any component handling it. A
    /// motion is never `Ignored` once a surface exists, and that is the signal
    /// a host redraws on — the guarantee this row depends on.
    #[test]
    fn motion_within_the_row_is_never_ignored_so_hover_position_repaints() {
        let theme = Theme::default_dark();
        let style = TabsStyle::from_theme(&theme);
        let state = State::default();
        let tabs_area = Rect::new(0, 0, 18, TabsSize::Small.height());
        let mut ratcn = Ratcn::new();
        let mut terminal = Terminal::new(TestBackend::new(18, tabs_area.height)).expect("terminal");
        let draw = |ratcn: &mut Ratcn<State, Msg>, terminal: &mut Terminal<TestBackend>| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.render_component(ChildId::Static("tabs"), manual(), tabs_area);
                    });
                })
                .expect("draw");
        };

        draw(&mut ratcn, &mut terminal);
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 14, 0), &state),
            EventResult::Consumed,
            "arriving on the row"
        );
        draw(&mut ratcn, &mut terminal);
        let hovered_tab = terminal
            .backend()
            .buffer()
            .cell((14, 0))
            .expect("hovered label cell")
            .bg;
        assert_eq!(hovered_tab, style.hovered_background);

        // The empty end of the row is inside the tabs component but on no tab,
        // so nothing handles the motion. Hover does not move either — the
        // pointer is on the same component — yet the frame is now wrong.
        assert_eq!(
            ratcn.handle_event(mouse(MouseKind::Moved, 17, 0), &state),
            EventResult::Consumed,
            "a motion nothing handled is still the redraw signal"
        );
        draw(&mut ratcn, &mut terminal);
        assert_ne!(
            terminal
                .backend()
                .buffer()
                .cell((14, 0))
                .expect("label cell")
                .bg,
            style.hovered_background,
            "the tab under the old pointer position stopped being highlighted"
        );
    }

    #[test]
    fn hovered_row_shows_the_hover_shift_not_the_focus_shift() {
        use crate::runtime::ScopeOptions;

        let theme = Theme::default_dark();
        let style = TabsStyle::from_theme(&theme);
        let state = State {
            focused: Some(Screen::C),
            selected: Screen::A,
            ..State::default()
        };
        let tabs_area = Rect::new(0, 0, 18, TabsSize::Small.height());
        let decoy_area = Rect::new(0, tabs_area.height, 18, 1);
        let mut ratcn = Ratcn::new();
        let mut terminal =
            Terminal::new(TestBackend::new(18, tabs_area.height + 1)).expect("terminal");

        // The decoy scope is declared first, so startup focus lands there and
        // the tab strip paints unfocused — which is what lets the hover shift
        // be told apart from the focus shift below.
        let draw = |ratcn: &mut Ratcn<State, Msg>, terminal: &mut Terminal<TestBackend>| {
            terminal
                .draw(|frame| {
                    ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.scope(
                            ChildId::Static("decoy"),
                            decoy_area,
                            ScopeOptions::default().focusable(),
                            |_| {},
                        );
                        ctx.render_component(ChildId::Static("tabs"), manual(), tabs_area);
                    });
                })
                .expect("draw");
        };

        // Hover position comes from a real pointer event, and events need a
        // surface to route against — so this is render, move, render.
        draw(&mut ratcn, &mut terminal);
        ratcn.handle_event(mouse(MouseKind::Moved, 14, 0), &state);
        draw(&mut ratcn, &mut terminal);

        // C is the cursor tab. Pointing at the row gives it the hover colors, not
        // the focus ones — hover and focus are distinct so a style can express
        // both, and so pointing at an already-focused tab still changes something.
        let cursor = terminal
            .backend()
            .buffer()
            .cell((14, 0))
            .expect("cursor label cell");
        assert_eq!(cursor.bg, style.hovered_background);
        assert_ne!(
            style.hovered_background, style.focused_background,
            "hover must be distinguishable from focus"
        );
    }

    // The point of giving every state a foreground: a row whose fills all match
    // the surface still has to signal focus, hover, and selection.
    #[test]
    fn a_row_with_no_fill_still_distinguishes_every_state() {
        let surface = Color::Rgb(10, 10, 10);
        let style = TabsStyle {
            foreground: Color::Rgb(120, 120, 120),
            focused_foreground: Color::Rgb(200, 200, 200),
            hovered_foreground: Color::Rgb(255, 255, 255),
            selected_foreground: Color::Rgb(139, 92, 246),
            background: surface,
            focused_background: surface,
            hovered_background: surface,
            selected_background: surface,
            selected_focused_background: surface,
            selected_hovered_background: surface,
            ..TabsStyle::fallback()
        };

        let resting = style.resolve(false, false, false, false);
        let focused = style.resolve(false, true, false, false);
        let hovered = style.resolve(false, false, true, false);
        let selected = style.resolve(true, false, false, false);

        for resolved in [resting, focused, hovered, selected] {
            assert_eq!(resolved.fill, surface, "no state may paint its own fill");
        }
        assert_ne!(resting.foreground, focused.foreground);
        assert_ne!(focused.foreground, hovered.foreground);
        assert_ne!(resting.foreground, selected.foreground);
    }

    // Hover wins over focus, matching buttons, so pointing at the tab the
    // keyboard already sits on still changes something.
    #[test]
    fn hover_takes_precedence_over_focus() {
        let style = TabsStyle::from_theme(&Theme::default_dark());
        let both = style.resolve(false, true, true, false);
        assert_eq!(both.fill, style.hovered_background);
    }

    #[test]
    fn one_row_area_renders_tabs() {
        let area = Rect::new(0, 0, 20, TabsSize::Small.height());
        let mut buffer = Buffer::empty(area);

        Widget::render(
            TabsWidget::new(&["A", "B"]).selected_tab(Some(0)),
            area,
            &mut buffer,
        );

        assert_eq!(
            buffer.cell((2, 0)).expect("selected label cell").symbol(),
            "A"
        );
    }

    #[test]
    fn large_tabs_render_top_and_bottom_caps() {
        let area = Rect::new(0, 0, 20, TabsSize::Large.height());
        let mut buffer = Buffer::empty(area);

        Widget::render(
            TabsWidget::new(&["A"])
                .selected_tab(Some(0))
                .size(TabsSize::Large),
            area,
            &mut buffer,
        );

        assert_eq!(TabsWidget::new(&["A"]).size(TabsSize::Large).height(), 3);
        assert_eq!(buffer.cell((0, 0)).expect("top cap").symbol(), "▄");
        assert_eq!(buffer.cell((2, 1)).expect("label").symbol(), "A");
        assert_eq!(buffer.cell((0, 2)).expect("bottom cap").symbol(), "▀");
    }

    #[test]
    fn tab_hit_rects_match_the_painted_row() {
        let rects = tab_rects(Rect::new(0, 0, 20, 3), &["A", "B"], TabsSize::Small);

        assert_eq!(rects[0], Rect::new(0, 0, 5, TabsSize::Small.height()));
        assert!(rects[0].contains(Position { x: 2, y: 0 }));
        assert!(!rects[0].contains(Position { x: 2, y: 1 }));
    }

    #[test]
    fn tab_rects_omit_tabs_that_do_not_fit_fully() {
        // A is 5 cells wide; after one spacing cell, B also needs 5 cells.
        // Width 9 leaves room for A plus the overflow marker, but not B.
        let rects = tab_rects(Rect::new(0, 0, 9, 1), &["A", "B"], TabsSize::Small);

        assert_eq!(rects, vec![Rect::new(0, 0, 5, 1)]);
    }

    #[test]
    fn hidden_tabs_on_the_right_render_a_right_marker() {
        // Width 9 fits tab A (5) plus a gap and the marker, but not B.
        let area = Rect::new(0, 0, 9, TabsSize::Small.height());
        let mut buffer = Buffer::empty(area);

        Widget::render(
            TabsWidget::new(&["A", "B"]).selected_tab(Some(0)),
            area,
            &mut buffer,
        );

        assert_eq!(buffer.cell((2, 0)).expect("tab A").symbol(), "A");
        assert_eq!(buffer.cell((6, 0)).expect("right marker").symbol(), "›");
    }

    #[test]
    fn window_keeps_the_active_tab_visible_and_marks_the_hidden_side() {
        // Six 5-wide tabs cannot all fit in 18 cells. Selecting a late tab must
        // scroll it into view, with a left marker for the tabs now hidden.
        let labels = ["A", "B", "C", "D", "E", "F"];
        let area = Rect::new(0, 0, 18, TabsSize::Small.height());
        let mut buffer = Buffer::empty(area);

        Widget::render(
            TabsWidget::new(&labels).selected_tab(Some(4)),
            area,
            &mut buffer,
        );

        let row: String = (0..18)
            .map(|x| buffer.cell((x, 0)).expect("cell").symbol().to_string())
            .collect();
        assert!(
            row.contains('E'),
            "selected tab E must be on screen: {row:?}"
        );
        assert!(row.contains('‹'), "hidden left tabs get a marker: {row:?}");
        assert!(
            !row.contains('A'),
            "an off-screen tab is not painted: {row:?}"
        );
    }

    #[test]
    fn middle_window_marks_hidden_tabs_on_both_sides() {
        let labels = ["A", "B", "C", "D", "E", "F"];
        let area = Rect::new(0, 0, 18, TabsSize::Small.height());
        let mut buffer = Buffer::empty(area);

        Widget::render(
            TabsWidget::new(&labels).selected_tab(Some(3)),
            area,
            &mut buffer,
        );

        let row: String = (0..18)
            .map(|x| buffer.cell((x, 0)).expect("cell").symbol().to_string())
            .collect();
        assert!(row.contains('‹'), "left marker missing: {row:?}");
        assert!(row.contains('›'), "right marker missing: {row:?}");
        assert!(row.contains('D'), "active tab missing: {row:?}");
    }

    #[test]
    fn oversized_active_tab_clips_but_still_renders() {
        let area = Rect::new(0, 0, 4, TabsSize::Small.height());
        let mut buffer = Buffer::empty(area);

        Widget::render(
            TabsWidget::new(&["VeryWide"]).selected_tab(Some(0)),
            area,
            &mut buffer,
        );

        let row: String = (0..4)
            .map(|x| buffer.cell((x, 0)).expect("cell").symbol().to_string())
            .collect();
        assert_eq!(row, "Very");
    }

    #[test]
    fn a_row_too_narrow_for_markers_still_shows_the_active_tab() {
        // Width 6 holds exactly one tab and no room for a marker beside it: the
        // active tab wins over the "more exist" hint.
        let labels = ["AA", "BB", "CC"];
        let area = Rect::new(0, 0, 6, TabsSize::Small.height());
        let mut buffer = Buffer::empty(area);

        Widget::render(
            TabsWidget::new(&labels).selected_tab(Some(1)),
            area,
            &mut buffer,
        );

        let row: String = (0..6)
            .map(|x| buffer.cell((x, 0)).expect("cell").symbol().to_string())
            .collect();
        assert!(
            row.contains("BB"),
            "the active tab is shown, not a bare marker: {row:?}"
        );
    }

    #[test]
    fn tab_width_matches_a_button_and_counts_cells_not_chars_or_bytes() {
        // label + 4, the same width as a Button of the same text.
        assert_eq!(tab_width("A"), 5);
        assert_eq!(tab_width("åβ"), 6);
        assert_eq!(tab_width("日本"), 8, "CJK chars are 2 cells");
        assert_eq!(tab_width("🚀"), 6, "emoji are 2 cells");
        assert_eq!(tab_width("e\u{301}"), 5, "combining mark adds no cell");
        assert_eq!(
            tab_width("👩\u{200d}👩\u{200d}👦"),
            6,
            "a ZWJ sequence is one 2-cell glyph, not its char count"
        );
    }

    #[test]
    fn filled_middle_truncates_by_cells_and_pads_the_fill() {
        // Centered by cells when the label fits.
        assert_eq!(filled_middle("日本", 8), "  日本  ");
        // Truncation never splits a wide char's cells or overflows the width;
        // the shortfall is padded so the fill spans the whole tab.
        assert_eq!(filled_middle("日本語", 5), "日本 ");
        assert_eq!(filled_middle("日本語", 4), "日本");
        assert_eq!(filled_middle("日本語", 3), "日 ");
        assert_eq!(filled_middle("日本語", 0), "");
        for width in 0..=8 {
            assert_eq!(
                display_width(&filled_middle("日本語", width)),
                width.min(8),
                "middle row must span exactly the tab width"
            );
        }
    }

    #[test]
    fn wide_label_tab_paints_centered_with_the_fill_spanning_its_cells() {
        let theme = Theme::default_dark();
        let style = TabsStyle::from_theme(&theme);
        // The tab is label + 4 cells wide: "日本" is 4 cells, so the label
        // starts 2 cells in and the fill runs through cell 7.
        let area = Rect::new(0, 0, 12, TabsSize::Small.height());
        let mut buffer = Buffer::empty(area);

        Widget::render(
            TabsWidget::new(&["日本"])
                .selected_tab(Some(0))
                .style(style),
            area,
            &mut buffer,
        );

        assert_eq!(buffer.cell((2, 0)).expect("label start").symbol(), "日");
        assert_eq!(buffer.cell((4, 0)).expect("label middle").symbol(), "本");
        for x in [0, 1, 6, 7] {
            let pad = buffer.cell((x, 0)).expect("padding cell");
            assert_eq!(pad.symbol(), " ", "pad at {x}");
            assert_eq!(pad.bg, style.selected_background, "fill spans tab at {x}");
        }
        // Past the tab: untouched.
        assert_eq!(buffer.cell((8, 0)).expect("outside cell").bg, Color::Reset);
    }

    #[test]
    fn undersized_area_never_paints_outside_it() {
        let theme = Theme::default_dark();
        let style = TabsStyle::from_theme(&theme);
        // Buffer wider than the tabs' area; a wide label that cannot fit must
        // clip inside the area, not spill past it.
        let surround = Rect::new(0, 0, 12, TabsSize::Small.height());
        let area = Rect::new(2, 0, 5, TabsSize::Small.height());
        let mut buffer = Buffer::empty(surround);

        Widget::render(
            TabsWidget::new(&["日本語"])
                .selected_tab(Some(0))
                .style(style),
            area,
            &mut buffer,
        );

        for x in [0, 1, 7, 8, 9, 10, 11] {
            let outside = buffer.cell((x, 0)).expect("cell outside the area");
            assert_eq!(outside.symbol(), " ", "outside cell {x} written");
            assert_eq!(outside.bg, Color::Reset, "outside cell {x} styled");
        }

        // Zero-sized areas are a no-op, not a panic.
        let before = buffer.clone();
        Widget::render(
            TabsWidget::new(&["日本語"]).selected_tab(Some(0)),
            Rect::new(2, 0, 0, 1),
            &mut buffer,
        );
        Widget::render(
            TabsWidget::new(&["日本語"]).selected_tab(Some(0)),
            Rect::new(2, 0, 5, 0),
            &mut buffer,
        );
        assert_eq!(buffer, before);
    }

    #[test]
    fn tabs_widget_width_includes_spacing_between_tabs() {
        assert_eq!(TabsWidget::new(&["A", "B"]).width(), 11);
    }

    #[test]
    fn window_width_sums_tab_widths_and_the_gaps_between_them() {
        // Three 5-wide tabs: 5 + 1 + 5 + 1 + 5.
        assert_eq!(window_width(&width_prefix_sums(&[5, 5, 5]), 0, 2), 17);
        // A single tab has no gap.
        assert_eq!(window_width(&width_prefix_sums(&[5, 9, 5]), 1, 1), 9);
        // A sub-range: two tabs and one gap.
        assert_eq!(window_width(&width_prefix_sums(&[5, 5, 5, 5]), 1, 2), 11);
    }

    #[test]
    fn window_width_saturates_rather_than_wrapping() {
        // A run past what `u16` can hold clamps: a wrapped width would claim to
        // fit a row it dwarfs.
        assert_eq!(
            window_width(&width_prefix_sums(&[u16::MAX, 2]), 0, 1),
            u16::MAX
        );
    }

    #[test]
    fn tab_window_shows_everything_when_it_all_fits() {
        let window = tab_window(&[5, 5, 5, 5, 5, 5], 0, 100);
        assert_eq!((window.lo, window.hi), (0, 5));
        assert_eq!(window.width, 35); // 6 * 5 + 5 gaps
    }

    #[test]
    fn tab_window_keeps_a_late_active_tab_visible() {
        // Six 5-wide tabs, 18 cells. The visible group must include the active
        // tab, drop the earlier tabs (reserving a left marker), and stay within
        // budget.
        let window = tab_window(&[5, 5, 5, 5, 5, 5], 4, 18);
        assert!(
            window.lo <= 4 && 4 <= window.hi,
            "must include the active tab"
        );
        assert!(window.lo > 0, "earlier tabs are hidden");
        assert!(window.width <= 18);
    }

    #[test]
    fn tab_window_is_just_the_active_tab_when_nothing_else_fits() {
        // Room for one 6-wide tab only.
        let window = tab_window(&[6, 6, 6], 1, 6);
        assert_eq!((window.lo, window.hi), (1, 1));
        assert_eq!(window.width, 6);
    }

    #[test]
    fn string_sugar_keys_tabs_by_their_labels() {
        let tabs = Tabs::<String, State, Msg>::new(["One", "Two"]);
        assert_eq!(tabs.items[0].value(), "One");
        assert_eq!(tabs.items[0].label(), "One");
        assert!(!tabs.items[0].is_disabled());
    }

    // The check is a debug-build assertion, so this contract only exists where
    // debug assertions do.
    #[test]
    #[cfg(debug_assertions)]
    fn duplicate_tab_values_panic_with_the_shared_message() {
        let mut tabs: Tabs<Screen, State, Msg> =
            Tabs::new([Tab::new(Screen::A, "First"), Tab::new(Screen::A, "Second")]);

        let panic = catch_unwind(AssertUnwindSafe(|| {
            Component::prepare(&mut tabs, &State::default());
        }))
        .expect_err("duplicate tab values must panic");
        let message = panic
            .downcast_ref::<String>()
            .expect("panic carries a String");
        assert_eq!(
            message,
            "Tabs item values must be unique within a Tabs declaration"
        );
    }

    // Whole-control disabled mirrors List: not focusable, events ignored, and
    // the row painted in the style's disabled colors.
    #[test]
    fn disabled_row_is_not_focusable_and_ignores_events() {
        let mut tabs = manual().disabled(true);
        tabs.hits = tab_layout(
            Rect::new(0, 0, 30, TabsSize::Small.height()),
            &["A", "B", "C"],
            TabsSize::Small,
            0,
        );
        let state = State::default();

        assert!(!tabs.is_focusable(&state));
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Right), &state, &mut EventCtx::default()),
            EventResult::Ignored
        );
        assert_eq!(
            tabs.handle_event(&key(KeyCode::Enter), &state, &mut EventCtx::default()),
            EventResult::Ignored
        );
        assert_eq!(
            tabs.handle_event(
                &mouse(MouseKind::Click(MouseButton::Left), 2, 0),
                &state,
                &mut EventCtx::default()
            ),
            EventResult::Ignored
        );
    }

    #[test]
    fn disabled_row_paints_every_tab_in_disabled_colors() {
        let theme = Theme::default_dark();
        let style = TabsStyle::from_theme(&theme);
        let area = Rect::new(0, 0, 18, TabsSize::Small.height());
        let mut buffer = Buffer::empty(area);

        // Selected A, whole row disabled: the selected tab keeps its
        // selected-disabled identity, the rest grey out.
        Widget::render(
            TabsWidget::new(&["A", "B"])
                .selected_tab(Some(0))
                .disabled(true)
                .style(style),
            area,
            &mut buffer,
        );

        let selected = buffer.cell((2, 0)).expect("selected disabled tab");
        let unselected = buffer.cell((8, 0)).expect("unselected disabled tab");
        assert_eq!(selected.bg, style.selected_disabled_background);
        assert_eq!(unselected.bg, style.disabled_background);
    }

    #[test]
    fn a_disabled_row_is_skipped_by_tab_traversal() {
        #[derive(Default)]
        struct RoutedState {
            focus: crate::runtime::FocusState,
            selected: Screen,
        }

        #[derive(Debug, PartialEq)]
        enum RoutedMsg {
            Focus(crate::runtime::FocusState),
            Selected(Screen),
            Pressed,
        }

        let theme = Theme::default_dark();
        let state = RoutedState::default();
        let mut ratcn = Ratcn::new().focus(|state: &RoutedState| &state.focus, RoutedMsg::Focus);
        let mut terminal = Terminal::new(TestBackend::new(20, 2)).expect("terminal");
        terminal
            .draw(|frame| {
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.render_component(
                        ChildId::Static("tabs"),
                        Tabs::new([Tab::new(Screen::A, "A"), Tab::new(Screen::B, "B")])
                            .selection(
                                |state: &RoutedState| Some(state.selected),
                                RoutedMsg::Selected,
                            )
                            .activation(TabsActivation::Automatic)
                            .disabled(true),
                        Rect::new(0, 0, 20, 1),
                    );
                    ctx.render_component(
                        ChildId::Static("button"),
                        crate::Button::new("Next").on_press(|| RoutedMsg::Pressed),
                        Rect::new(0, 1, 20, 1),
                    );
                });
            })
            .expect("draw");

        // Startup focus resolves past the disabled row onto the button.
        assert_eq!(
            ratcn.handle_event(key(KeyCode::Enter), &state),
            EventResult::Emit(RoutedMsg::Pressed)
        );
    }
}
