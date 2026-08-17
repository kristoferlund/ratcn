use std::fmt;

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect, Size},
    style::{Color, Style},
    text::Line,
    widgets::Widget,
};

use ratcn::Theme;
use ratcn::button_shape::{BOTTOM_CAP, TOP_CAP, cap_row, filled_middle, shape_width};
use ratcn::color::{
    DISABLED_DIM, FOCUS_DARKEN, FOCUS_LIGHTEN, HOVER_DARKEN, HOVER_LIGHTEN, darken, dim, lighten,
};
use ratcn::linear_nav;
use ratcn::list_core;
use ratcn::runtime::{
    Component, Event, EventCtx, EventResult, KeyCode, KeyEvent, MeasuredComponent, MouseButton,
    MouseKind, PaintCtx, RenderCtx, Step,
};

/// How tall the tab row is drawn. Matches [`ButtonSize`](ratcn::ButtonSize),
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
/// A tab is drawn as a button, so these mirror [`ButtonStyle`](ratcn::ButtonStyle)
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
/// then the resting colors — the same precedence [`ButtonStyle`](ratcn::ButtonStyle)
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
    /// precedence matches [`ButtonStyle`](ratcn::ButtonStyle) — disabled first,
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
/// [`Ratcn`](ratcn::runtime::Ratcn) or the component layer: render it directly
/// and keep tracking the selected tab however you already do. Use [`Tabs`] when
/// you want keyboard traversal and selection messages handled for you; it paints
/// through this widget internally.
///
/// Inputs are parallel to the labels by index — which tab is selected
/// ([`selected_tab`](Self::selected_tab)), which tab the cursor is on
/// ([`focused_tab`](Self::focused_tab)), and which tabs are disabled
/// ([`disabled_tabs`](Self::disabled_tabs)) — the same shape as
/// [`ListWidget`](ratcn::ListWidget). When the tabs are wider than the row, the
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
        }
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
    /// enabled. Matches [`ListWidget::disabled_rows`](ratcn::ListWidget::disabled_rows).
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
        // Keep the focused tab visible when it exists; otherwise keep the
        // selected tab visible. The component derives the same tab, so paint and
        // hit-testing agree.
        let must_show = self.focused_tab.or(self.selected_tab).unwrap_or(0);
        let TabLayout { tabs, left, right } = tab_layout(area, self.labels, self.size, must_show);
        for (index, rect) in tabs {
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
        for (marker, slot) in [(LEFT_MARKER, left), (RIGHT_MARKER, right)] {
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

/// One tab: an identifying `value` and the `label` shown on it.
///
/// Selection is recorded as the value, not the tab's position, so the tab row
/// can be built from data that changes without the selection sliding onto a
/// different tab. `value` is usually the same enum the app matches on to decide
/// what to draw below the row. Values must be unique within a [`Tabs`]
/// declaration, or selection and tab focus are ambiguous; a debug build panics
/// on duplicates during declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab<T> {
    value: T,
    label: String,
    disabled: bool,
}

impl<T> Tab<T> {
    /// A tab identified by `value` and displayed as `label`.
    #[must_use]
    pub fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            disabled: false,
        }
    }

    /// Dim this tab and make it unselectable. Arrow keys skip it and clicks on
    /// it do nothing.
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

    /// Whether [`disabled`](Self::disabled) marked this tab unselectable.
    #[must_use]
    pub const fn is_disabled(&self) -> bool {
        self.disabled
    }
}

impl From<&str> for Tab<String> {
    fn from(label: &str) -> Self {
        Self::new(label.to_owned(), label)
    }
}

impl From<String> for Tab<String> {
    fn from(label: String) -> Self {
        Self::new(label.clone(), label)
    }
}

/// Reads a tab value (the selection or manual-mode tab focus) out of app
/// state; `None` means no tab holds that role yet.
type ReadFn<S, T> = Box<dyn Fn(&S) -> Option<T>>;
/// Builds the app's message from a tab value.
type OnChangeFn<T, M> = Box<dyn Fn(T) -> M>;
/// Resolves the tabs' style from the active theme (the style override).
type StyleFn = Box<dyn Fn(&Theme) -> TabsStyle>;

/// A focusable row of tabs, declared with
/// [`render_component`](ratcn::runtime::RenderCtx::render_component).
///
/// The horizontal counterpart of [`List`](ratcn::List), and it works the same
/// way: a cursor that moves and a selection that commits, both keyed by your own
/// values rather than by position.
///
/// The row draws only the tabs. What appears *below* them is the app's — match
/// on the selected value and render whatever that screen is. `Tabs` never owns
/// content.
///
/// # Cursor and selection
///
/// - [`tab_focus`](Self::tab_focus) is the cursor: which tab Left/Right, Home,
///   End, and typeahead are pointing at.
/// - [`selection`](Self::selection) is the active tab, the one whose content is
///   showing.
///
/// Typing a printable character jumps to the next enabled tab whose label
/// starts with it, cycling past the end — the same single-character typeahead
/// as [`List`](ratcn::List) and [`Select`](ratcn::Select). A character
/// matching no tab is ignored, so it still reaches the app as a hotkey.
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
    /// first enabled tab. Left/Right, Home/End, and typeahead move it. Moving
    /// it does not switch tabs — [`selection`](Self::selection) does that, on
    /// Enter, Space, or a click.
    ///
    /// Named for tabs rather than items, unlike
    /// [`List::item_focus`](ratcn::List::item_focus) and
    /// [`Select::item_focus`](ratcn::Select::item_focus): those two share the
    /// [`ListItem`](ratcn::ListItem) vocabulary, while a tab is its own
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
        row_width(self.items.iter().map(|tab| tab.label.as_str()))
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
        self.items.get(index).is_some_and(|tab| tab.disabled)
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
/// [`linear_nav`](ratcn::linear_nav): Left/Right for everyone, `h`/`l` for
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
        self.items.iter().position(|tab| &tab.value == value)
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
            Some(on_select) => EventResult::Emit(on_select(self.items[index].value.clone())),
            None => EventResult::Ignored,
        }
    }

    /// Move focus to the tab at `index`, or let the event bubble when no focus
    /// handler is wired.
    fn move_focus(&self, index: usize) -> EventResult<M> {
        match &self.on_focus_change {
            Some(on_focus_change) => {
                EventResult::Emit(on_focus_change(self.items[index].value.clone()))
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
        let labels: Vec<&str> = self.items.iter().map(|tab| tab.label.as_str()).collect();
        // Capture geometry for click hit-testing. The widget chooses visible
        // tabs from the same inputs, so paint and routing agree.
        let must_show = cursor.or(selected).unwrap_or(0);
        self.hits = tab_layout(ctx.area(), &labels, self.size, must_show);
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, S>) {
        let area = ctx.area();
        let state = ctx.state();
        let selected = self.selected_index(state);
        let cursor = self.cursor_index(state);
        let labels: Vec<&str> = self.items.iter().map(|tab| tab.label.as_str()).collect();
        let disabled: Vec<bool> = (0..self.items.len()).map(|i| self.disabled_at(i)).collect();
        let hovered_tab = ctx.hover_position().and_then(|position| {
            self.hits
                .tabs
                .iter()
                .find(|(_, rect)| rect.contains(position))
                .map(|(index, _)| *index)
        });
        let style = match &self.style {
            Some(style) => style(ctx.theme),
            None => TabsStyle::from_theme(ctx.theme),
        };
        let widget = TabsWidget::new(&labels)
            .selected_tab(selected)
            .focused_tab(cursor)
            .disabled_tabs(&disabled)
            .focused(ctx.focused)
            .hovered_tab(hovered_tab)
            .disabled(self.disabled)
            .size(self.size)
            .style(style);
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
        if area.width == 0 || area.height < self.height() {
            Rect::default()
        } else {
            Rect {
                height: self.height(),
                ..area
            }
        }
    }

    fn is_focusable(&self, _state: &S) -> bool {
        !self.disabled
            && self.keyboard_enabled()
            && linear_nav::first_enabled(self.items.len(), |index| self.disabled_at(index))
                .is_some()
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

#[derive(Default)]
struct TabLayout {
    /// Each visible tab as `(label index, rect)`. The index is the tab's original
    /// position. Hidden tabs mean this index may differ from the tab's screen slot.
    tabs: Vec<(usize, Rect)>,
    left: Option<Rect>,
    right: Option<Rect>,
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
/// lay them out. The resulting tab rects are the single source of tab geometry,
/// shared by the widget's paint and the component's hit-test.
fn tab_layout(area: Rect, labels: &[&str], size: TabsSize, must_show: usize) -> TabLayout {
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
