use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Flex, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph, Widget},
};

use ratcn::text_width::{display_width, wrap_to_width};
use ratcn::toast::{Toast, ToastEntry, ToastKind, ToasterState};
use ratcn::{BorderStyle, Theme};

const DEFAULT_WIDTH: u16 = 42;
const DEFAULT_GAP: u16 = 1;
const DEFAULT_VISIBLE_TOASTS: usize = 3;
const DEFAULT_INSET: u16 = 1;
const TITLE_PREFIX_WIDTH: u16 = 3;
const MIN_TITLE_WIDTH: u16 = 2;

/// Which corner or edge the toast stack sits against.
///
/// The choice also decides stacking direction: a stack anchored to the top grows
/// downward, one anchored to the bottom grows upward, so the newest toast always
/// appears nearest the edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ToastPosition {
    /// Top-left corner.
    TopLeft,
    /// Centered along the top edge.
    TopCenter,
    /// Top-right corner.
    TopRight,
    /// Bottom-left corner.
    BottomLeft,
    /// Centered along the bottom edge.
    BottomCenter,
    /// Bottom-right corner.
    #[default]
    BottomRight,
}

impl ToastPosition {
    /// Whether this position anchors to the top edge, and so stacks downward.
    #[must_use]
    pub const fn is_top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopCenter | Self::TopRight)
    }

    const fn horizontal_flex(self) -> Flex {
        match self {
            Self::TopLeft | Self::BottomLeft => Flex::Start,
            Self::TopCenter | Self::BottomCenter => Flex::Center,
            Self::TopRight | Self::BottomRight => Flex::End,
        }
    }
}

/// Colors for the toast stack: one shared look, plus an accent per
/// [`ToastKind`].
///
/// Toasts all share a surface and border; what distinguishes them is the accent
/// applied to the title marker and border, chosen by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToasterStyle {
    /// Title text on every toast.
    pub foreground: Color,
    /// Description text, dimmer than the title.
    pub muted_foreground: Color,
    /// Fill behind a toast. Toasts sit over app content, so this should be
    /// opaque rather than `Color::Reset`.
    pub background: Color,
    /// Border and accent for [`ToastKind::Default`], and the fallback for any
    /// kind added later.
    pub border: Color,
    /// Accent for [`ToastKind::Success`].
    pub success: Color,
    /// Accent for [`ToastKind::Info`].
    pub info: Color,
    /// Accent for [`ToastKind::Error`].
    pub error: Color,
    /// Accent for [`ToastKind::Warning`].
    pub warning: Color,
    /// Accent for [`ToastKind::Loading`], usually muted since it is transient.
    pub loading: Color,
    /// Which line-drawing characters the border uses. See [`BorderStyle`].
    pub border_style: BorderStyle,
}

impl ToasterStyle {
    /// A neutral style using plain ANSI colors, for painting without a
    /// [`Theme`]. Prefer [`from_theme`](Self::from_theme) when one is available.
    ///
    /// The background is `Color::Black` rather than `Color::Reset` because the
    /// toast surface must be opaque — it composites over base content.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            foreground: Color::Reset,
            muted_foreground: Color::DarkGray,
            background: Color::Black,
            border: Color::DarkGray,
            success: Color::LightGreen,
            info: Color::LightMagenta,
            error: Color::LightRed,
            warning: Color::Yellow,
            loading: Color::DarkGray,
            border_style: BorderStyle::Single,
        }
    }

    /// Derive toast colors from `theme`.
    #[must_use]
    pub const fn from_theme(theme: &Theme) -> Self {
        Self {
            foreground: theme.foreground,
            muted_foreground: theme.muted_foreground,
            background: theme.surface,
            border: theme.border,
            success: theme.primary,
            info: theme.accent,
            error: theme.destructive,
            warning: theme.warning,
            loading: theme.muted_foreground,
            border_style: BorderStyle::Single,
        }
    }

    /// The accent color and title style for a toast kind — the single place a
    /// kind is turned into style.
    #[allow(
        unreachable_patterns,
        reason = "the wildcard is unreachable inside this crate but required once this module \
                  is copied outside it, where `ToastKind`'s #[non_exhaustive] blocks an \
                  exhaustive match"
    )]
    fn resolve(&self, kind: ToastKind) -> (Color, Style) {
        let accent = match kind {
            ToastKind::Success => self.success,
            ToastKind::Info => self.info,
            ToastKind::Error => self.error,
            ToastKind::Warning => self.warning,
            ToastKind::Loading => self.loading,
            // `Default` and any future variant both fall back to the border
            // accent.
            ToastKind::Default | _ => self.border,
        };
        (
            accent,
            Style::default()
                .fg(self.foreground)
                .bg(self.background)
                .add_modifier(Modifier::BOLD),
        )
    }
}

/// Draws the newest toasts stacked in a corner. Paint-only by nature: toasts
/// take no focus and handle no events, so there is no interactive half.
///
/// **Usable in any ratatui app.** Nothing here depends on
/// [`Ratcn`](ratcn::runtime::Ratcn) or the component layer — it is an ordinary
/// [`Widget`] over an app-owned [`ToasterState`], so
/// `frame.render_widget(...)` is all it needs.
///
/// Each toast's height is measured from its content wrapped at the stack
/// width, through the same code path that paints it. When the area cannot
/// hold every visible candidate, the newest toasts that fit whole are drawn
/// and the rest are dropped for that frame — a toast is never clipped
/// mid-content and toasts never overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToasterWidget<'a, 'toast> {
    toasts: &'a [ToastEntry<'toast>],
    now: Duration,
    style: ToasterStyle,
    position: ToastPosition,
    width: u16,
    gap: u16,
    max_visible_toasts: usize,
    inset_x: u16,
    inset_y: u16,
}

impl<'a, 'toast: 'a> ToasterWidget<'a, 'toast> {
    /// Draw the toasts in `toasts`, judging expiry against `now`.
    ///
    /// `now` is a reading from your own clock, the same one used for
    /// [`ToasterState::push`]. Expired toasts are skipped here even before
    /// [`prune_expired`](ToasterState::prune_expired) removes them, so a late
    /// prune shows stale toasts for at most zero frames.
    #[must_use]
    pub fn new(toasts: &'a ToasterState<'toast>, now: Duration) -> Self {
        Self::from_entries(toasts.entries(), now)
    }

    /// As [`new`](Self::new), but from a bare slice of entries — for apps that
    /// keep toasts in their own structure rather than a [`ToasterState`].
    #[must_use]
    pub fn from_entries(toasts: &'a [ToastEntry<'toast>], now: Duration) -> Self {
        Self {
            toasts,
            now,
            style: ToasterStyle::fallback(),
            position: ToastPosition::default(),
            width: DEFAULT_WIDTH,
            gap: DEFAULT_GAP,
            max_visible_toasts: DEFAULT_VISIBLE_TOASTS,
            inset_x: DEFAULT_INSET,
            inset_y: DEFAULT_INSET,
        }
    }

    /// Take colors from `theme`.
    #[must_use]
    pub const fn themed(mut self, theme: &Theme) -> Self {
        self.style = ToasterStyle::from_theme(theme);
        self
    }

    /// Use these exact colors, ignoring any theme.
    #[must_use]
    pub const fn style(mut self, style: ToasterStyle) -> Self {
        self.style = style;
        self
    }

    /// Which corner or edge the stack sits against. Defaults to
    /// [`ToastPosition::BottomRight`].
    #[must_use]
    pub const fn position(mut self, position: ToastPosition) -> Self {
        self.position = position;
        self
    }

    /// How wide each toast is, in cells. Titles and descriptions wrap to this,
    /// so it also decides how tall a toast ends up.
    #[must_use]
    pub const fn toast_width(mut self, width: u16) -> Self {
        self.width = width;
        self
    }

    /// Blank rows between stacked toasts.
    #[must_use]
    pub const fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// How many toasts to consider for painting at once, newest first.
    ///
    /// A cap on the stack, independent of the space available: older toasts past
    /// this many are simply not drawn, though they stay in the state and still
    /// expire on schedule. Candidates may still be dropped when the available
    /// area cannot hold them whole.
    #[must_use]
    pub const fn max_visible_toasts(mut self, max_visible_toasts: usize) -> Self {
        self.max_visible_toasts = max_visible_toasts;
        self
    }

    /// Inset from the anchored edges, in cells, so the stack does not sit flush
    /// against the terminal border.
    #[must_use]
    pub const fn inset(mut self, x: u16, y: u16) -> Self {
        self.inset_x = x;
        self.inset_y = y;
        self
    }

    /// The toasts this widget would draw, newest first: unexpired as of `now`,
    /// capped at [`max_visible_toasts`](Self::max_visible_toasts).
    ///
    /// Does not account for available space — a toast listed here may still be
    /// dropped at paint time if the area cannot hold it whole.
    #[must_use = "reports which toasts would draw without drawing them"]
    pub fn visible(&self) -> impl Iterator<Item = &'a Toast<'toast>> + '_ {
        self.toasts
            .iter()
            .filter(move |toast| !toast.is_expired(self.now))
            .rev()
            .take(self.max_visible_toasts)
            .map(ToastEntry::toast)
    }
}

impl<'a, 'toast: 'a> Widget for ToasterWidget<'a, 'toast> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() || self.max_visible_toasts == 0 {
            return;
        }

        let area = area.inner(Margin::new(self.inset_x, self.inset_y));
        let width = self.width.min(area.width);
        if width == 0 {
            return;
        }

        // Measure newest-first at the stack width. Skip an individually
        // unmeasurable toast, and stop once the remaining height cannot hold
        // the next measurable toast whole.
        let mut fitting: Vec<(&Toast<'_>, u16)> = Vec::new();
        let mut height = 0u16;
        for toast in self.visible() {
            let toast_height = toast_height(toast, &self.style, width);
            if toast_height == 0 {
                continue;
            }
            let gap = if fitting.is_empty() { 0 } else { self.gap };
            let stacked = height.saturating_add(gap).saturating_add(toast_height);
            if stacked > area.height {
                break;
            }
            height = stacked;
            fitting.push((toast, toast_height));
        }
        if height == 0 {
            return;
        }

        let vertical_flex = if self.position.is_top() {
            Flex::Start
        } else {
            Flex::End
        };
        let [column] = Layout::vertical([Constraint::Length(height)])
            .flex(vertical_flex)
            .areas(area);
        let [column] = Layout::horizontal([Constraint::Length(width)])
            .flex(self.position.horizontal_flex())
            .areas(column);

        let mut y = if self.position.is_top() {
            column.y
        } else {
            column.y + column.height
        };

        for (toast, toast_height) in fitting {
            let toast_y = if self.position.is_top() {
                let current = y;
                y = y.saturating_add(toast_height).saturating_add(self.gap);
                current
            } else {
                y = y.saturating_sub(toast_height);
                let current = y;
                y = y.saturating_sub(self.gap);
                current
            };
            let toast_area = Rect::new(column.x, toast_y, column.width, toast_height);
            render_toast(toast, &self.style, toast_area, buf);
        }
    }
}

fn render_toast(toast: &Toast<'_>, style: &ToasterStyle, area: Rect, buf: &mut Buffer) {
    if area.is_empty() || area.width < minimum_toast_width(toast) {
        return;
    }
    Clear.render(area, buf);

    let (accent, _) = style.resolve(toast.toast_kind());
    let content_area = if toast.is_bordered() {
        // A plain ratatui border, themed inline. Components draw their own
        // border rather than depending on the `Border` component.
        let block = Block::bordered()
            .border_set(style.border_style.to_border_set())
            .border_style(Style::default().fg(accent))
            .style(Style::default().fg(style.foreground).bg(style.background))
            .padding(Padding::symmetric(1, 0));
        let inner = block.inner(area);
        block.render(area, buf);
        inner
    } else {
        buf.set_style(
            area,
            Style::default().fg(style.foreground).bg(style.background),
        );
        area
    };

    Paragraph::new(toast_lines(toast, style, content_area.width))
        .alignment(Alignment::Left)
        .render(content_area, buf);
}

/// The wrapped content lines for one toast at `content_width` — the single
/// code path both measurement ([`toast_height`]) and paint use, so the stack
/// allocation can never disagree with what renders. The title wraps with a
/// hanging indent under its icon; the description wraps at the full content
/// width.
fn toast_lines<'a>(
    toast: &'a Toast<'_>,
    style: &ToasterStyle,
    content_width: u16,
) -> Vec<Line<'a>> {
    let (accent, title_style) = style.resolve(toast.toast_kind());
    let icon = toast_icon(toast.toast_kind());
    let prefix_width = usize::from(TITLE_PREFIX_WIDTH);

    let mut lines = Vec::new();
    let title_width = usize::from(content_width).saturating_sub(prefix_width);
    for (row, segment) in wrap_to_width(toast.title(), title_width)
        .into_iter()
        .enumerate()
    {
        let prefix = if row == 0 {
            Span::styled(icon, Style::default().fg(accent))
        } else {
            Span::raw(" ".repeat(display_width(icon)))
        };
        lines.push(Line::from(vec![
            prefix,
            Span::raw(" "),
            Span::styled(segment, title_style),
        ]));
    }
    if let Some(description) = toast.description_text() {
        for segment in wrap_to_width(description, content_width.into()) {
            lines.push(Line::from(segment).style(Style::default().fg(style.muted_foreground)));
        }
    }
    lines
}

/// The rows one toast occupies at stack width `width`: its [`toast_lines`]
/// count plus, when bordered, the two border rows. The horizontal chrome
/// (one border and one padding column per side) mirrors the block
/// [`render_toast`] builds, so the content width measured here is the width
/// painted at.
fn toast_height(toast: &Toast<'_>, style: &ToasterStyle, width: u16) -> u16 {
    if width < minimum_toast_width(toast) {
        return 0;
    }
    let (chrome_x, chrome_y) = if toast.is_bordered() { (4, 2) } else { (0, 0) };
    let content_width = width.saturating_sub(chrome_x);
    let lines = u16::try_from(toast_lines(toast, style, content_width).len()).unwrap_or(u16::MAX);
    lines.saturating_add(chrome_y)
}

/// Minimum width that can render the icon, its separator, and a whole terminal
/// glyph of title content without clipping.
const fn minimum_toast_width(toast: &Toast<'_>) -> u16 {
    let chrome = if toast.is_bordered() { 4 } else { 0 };
    chrome + TITLE_PREFIX_WIDTH + MIN_TITLE_WIDTH
}

#[allow(
    unreachable_patterns,
    reason = "the wildcard is unreachable inside this crate but required once this module is \
              copied outside it, where `ToastKind`'s #[non_exhaustive] blocks an exhaustive match"
)]
const fn toast_icon(kind: ToastKind) -> &'static str {
    match kind {
        ToastKind::Success => "OK",
        ToastKind::Error | ToastKind::Warning => "!!",
        ToastKind::Info => "i ",
        ToastKind::Loading => "..",
        // `Default` and any future variant both fall back to the default
        // icon (see the matching fallback in `ToasterStyle::resolve`).
        ToastKind::Default | _ => "--",
    }
}
