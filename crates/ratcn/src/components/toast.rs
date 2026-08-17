use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Flex, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph, Widget},
};

use crate::text_width::{display_width, wrap_to_width};
use crate::toast::{Toast, ToastEntry, ToastKind, ToasterState};
use crate::{BorderStyle, Theme};

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
/// [`Ratcn`](crate::runtime::Ratcn) or the component layer — it is an ordinary
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

        // Wrap newest-first at the stack width. Skip an individually
        // unmeasurable toast, and stop once the remaining height cannot hold
        // the next measurable toast whole.
        let mut fitting: Vec<PaintedToast<'_, '_>> = Vec::new();
        let mut height = 0u16;
        for toast in self.visible() {
            let Some(painted) = PaintedToast::prepare(toast, &self.style, width) else {
                continue;
            };
            let gap = if fitting.is_empty() { 0 } else { self.gap };
            let stacked = height.saturating_add(gap).saturating_add(painted.height);
            if stacked > area.height {
                break;
            }
            height = stacked;
            fitting.push(painted);
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

        for painted in fitting {
            let toast_height = painted.height;
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
            painted.render(&self.style, toast_area, buf);
        }
    }
}

/// One toast wrapped for the stack width: the lines to paint and the rows they
/// occupy. Each toast wraps once per frame, so the height that decides the
/// stack and the content that fills it are the same measurement rather than two
/// that agree.
struct PaintedToast<'a, 'toast> {
    toast: &'a Toast<'toast>,
    lines: Vec<Line<'a>>,
    height: u16,
}

impl<'a, 'toast> PaintedToast<'a, 'toast> {
    /// Wrap `toast` at stack width `width`, or `None` when that width cannot
    /// render it at all. The height is the [`toast_lines`] count plus, when
    /// bordered, the two border rows. The horizontal chrome (one border and one
    /// padding column per side) mirrors the block [`Self::render`] builds, so
    /// the content width wrapped at is the width painted at.
    fn prepare(toast: &'a Toast<'toast>, style: &ToasterStyle, width: u16) -> Option<Self> {
        if width < minimum_toast_width(toast) {
            return None;
        }
        let (chrome_x, chrome_y) = if toast.is_bordered() { (4, 2) } else { (0, 0) };
        let lines = toast_lines(toast, style, width.saturating_sub(chrome_x));
        let height = u16::try_from(lines.len())
            .unwrap_or(u16::MAX)
            .saturating_add(chrome_y);
        Some(Self {
            toast,
            lines,
            height,
        })
    }

    /// Paint the wrapped toast into `area`, whose height is [`Self::height`].
    fn render(self, style: &ToasterStyle, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        let (accent, _) = style.resolve(self.toast.toast_kind());
        let content_area = if self.toast.is_bordered() {
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

        Paragraph::new(self.lines)
            .alignment(Alignment::Left)
            .render(content_area, buf);
    }
}

/// The wrapped content lines for one toast at `content_width`. The title wraps
/// with a hanging indent under its icon; the description wraps at the full
/// content width.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_position_defaults_to_bottom_right() {
        assert_eq!(ToastPosition::default(), ToastPosition::BottomRight);
    }

    const DEFAULT_DURATION: Duration = Duration::from_secs(4);
    const LONG_DESCRIPTION: &str = "a rather long description that wraps onto several rows";

    /// The rows one toast occupies at stack width `width`, zero when the width
    /// cannot render it — what the stack asks of [`PaintedToast::prepare`].
    fn toast_height(toast: &Toast<'_>, style: &ToasterStyle, width: u16) -> u16 {
        PaintedToast::prepare(toast, style, width).map_or(0, |painted| painted.height)
    }

    /// Paint one toast into `area`, wrapped at that area's width, so a test
    /// paints through the path the stack paints through.
    fn render_toast(toast: &Toast<'_>, style: &ToasterStyle, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        if let Some(painted) = PaintedToast::prepare(toast, style, area.width) {
            painted.render(style, area, buf);
        }
    }

    /// Every buffer row joined into a string, wide-glyph placeholders and all.
    fn painted_rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    fn row_of(rows: &[String], needle: &str) -> usize {
        rows.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} was not painted"))
    }

    fn symbol_count(buf: &Buffer, symbol: &str) -> usize {
        buf.content()
            .iter()
            .filter(|cell| cell.symbol() == symbol)
            .count()
    }

    #[test]
    fn expired_toasts_are_not_visible() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("old"), Duration::ZERO);
        toasts.push(
            Toast::new("new"),
            DEFAULT_DURATION.saturating_sub(Duration::from_millis(10)),
        );
        let toaster = ToasterWidget::new(&toasts, DEFAULT_DURATION).themed(&Theme::terminal());
        let visible = toaster.visible().collect::<Vec<_>>();

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].title(), "new");
    }

    #[test]
    fn max_visible_toasts_prefers_latest_items() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("one"), Duration::ZERO);
        toasts.push(Toast::new("two"), Duration::ZERO);
        toasts.push(Toast::new("three"), Duration::ZERO);
        let toaster = ToasterWidget::new(&toasts, Duration::ZERO)
            .themed(&Theme::terminal())
            .max_visible_toasts(2);
        let visible = toaster.visible().collect::<Vec<_>>();

        assert_eq!(visible[0].title(), "three");
        assert_eq!(visible[1].title(), "two");
    }

    #[test]
    fn max_visible_toasts_zero_paints_nothing() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("hidden"), Duration::ZERO);
        let area = Rect::new(0, 0, 20, 6);
        let mut buf = Buffer::empty(area);

        ToasterWidget::new(&toasts, Duration::ZERO)
            .max_visible_toasts(0)
            .inset(0, 0)
            .render(area, &mut buf);

        assert!(painted_rows(&buf).iter().all(|row| row.trim().is_empty()));
    }

    #[test]
    fn every_kind_resolves_its_accent_and_icon() {
        let style = ToasterStyle {
            border: Color::Red,
            success: Color::Green,
            info: Color::Blue,
            error: Color::Magenta,
            warning: Color::Yellow,
            loading: Color::Cyan,
            ..ToasterStyle::fallback()
        };
        let cases = [
            (ToastKind::Default, Color::Red, "--"),
            (ToastKind::Success, Color::Green, "OK"),
            (ToastKind::Info, Color::Blue, "i "),
            (ToastKind::Error, Color::Magenta, "!!"),
            (ToastKind::Warning, Color::Yellow, "!!"),
            (ToastKind::Loading, Color::Cyan, ".."),
        ];

        for (kind, expected_accent, expected_icon) in cases {
            let (accent, title_style) = style.resolve(kind);
            assert_eq!(accent, expected_accent, "wrong accent for {kind:?}");
            assert_eq!(title_style.fg, Some(style.foreground));
            assert_eq!(toast_icon(kind), expected_icon, "wrong icon for {kind:?}");
        }
    }

    #[test]
    fn toast_height_follows_wrapped_content() {
        let style = ToasterStyle::fallback();
        let toast = Toast::new("saved").description("a description long enough to wrap");

        // At the default width the description fits on one row: one title
        // row, one description row, two border rows.
        assert_eq!(toast_height(&toast, &style, DEFAULT_WIDTH), 4);
        // At 20 columns (16 of content) the description wraps to three rows.
        assert_eq!(toast_height(&toast, &style, 20), 6);
        // Borderless drops the two border rows and widens the content.
        assert_eq!(toast_height(&toast.clone().border(false), &style, 20), 3);
    }

    #[test]
    fn measurement_matches_paint() {
        let style = ToasterStyle::fallback();
        let toast = Toast::new("saved").description("a description long enough to wrap");
        let width = 20;
        let height = toast_height(&toast, &style, width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        render_toast(&toast, &style, area, &mut buf);

        // The bottom border lands on the measured last row, with the last
        // wrapped content row directly above it: measurement and paint agree
        // on exactly how tall the toast is and nothing is clipped to get
        // there.
        let bottom_left = buf[(0, height - 1)].symbol();
        assert_eq!(bottom_left, style.border_style.to_border_set().bottom_left);
        let last_content: String = (0..width).map(|x| buf[(x, height - 2)].symbol()).collect();
        assert!(
            last_content.contains("wrap"),
            "the final wrapped description row paints above the border, got {last_content:?}"
        );
    }

    #[test]
    fn narrow_areas_drop_toasts_until_a_complete_title_row_fits() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("saved"), Duration::ZERO);

        let mut too_narrow = Buffer::empty(Rect::new(0, 0, 8, 5));
        ToasterWidget::new(&toasts, Duration::ZERO)
            .toast_width(8)
            .inset(0, 0)
            .position(ToastPosition::TopLeft)
            .render(too_narrow.area, &mut too_narrow);
        let border = ToasterStyle::fallback().border_style.to_border_set();
        assert_eq!(symbol_count(&too_narrow, border.top_left), 0);
        assert!(
            painted_rows(&too_narrow)
                .iter()
                .all(|row| !row.contains("saved")),
            "a too-narrow toast must not leave border-only or clipped title content"
        );

        let mut minimum = Buffer::empty(Rect::new(0, 0, 9, 5));
        ToasterWidget::new(&toasts, Duration::ZERO)
            .toast_width(9)
            .inset(0, 0)
            .position(ToastPosition::TopLeft)
            .render(minimum.area, &mut minimum);
        let rows = painted_rows(&minimum);
        assert_eq!(minimum[(0, 0)].symbol(), border.top_left);
        assert!(
            rows.iter().any(|row| row.contains("sa")),
            "the minimum-width toast paints usable title content"
        );
        assert_eq!(minimum[(0, 4)].symbol(), border.bottom_left);
    }

    #[test]
    fn unmeasurable_bordered_toast_does_not_discard_fitting_borderless_toast() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("old bordered"), Duration::ZERO);
        toasts.push(Toast::new("new").border(false), Duration::ZERO);
        let area = Rect::new(0, 0, 7, 4);
        let mut buf = Buffer::empty(area);

        ToasterWidget::new(&toasts, Duration::ZERO)
            .toast_width(7)
            .inset(0, 0)
            .position(ToastPosition::TopLeft)
            .render(area, &mut buf);

        let rows = painted_rows(&buf);
        assert!(rows.iter().any(|row| row.contains("new")));
        assert!(!rows.iter().any(|row| row.contains("old")));
        let corner = ToasterStyle::fallback()
            .border_style
            .to_border_set()
            .top_left;
        assert_eq!(symbol_count(&buf, corner), 0);
    }

    #[test]
    fn undersized_viewport_keeps_the_newest_toasts_that_fit_whole() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("one"), Duration::ZERO);
        toasts.push(Toast::new("two"), Duration::ZERO);
        toasts.push(Toast::new("three"), Duration::ZERO);
        // Room inside the offset inset for two bordered toasts (3 rows each)
        // plus the gap, not three.
        let area = Rect::new(0, 0, 30, 9);
        let mut buf = Buffer::empty(area);
        ToasterWidget::new(&toasts, Duration::ZERO).render(area, &mut buf);

        let rows = painted_rows(&buf);
        assert!(rows.iter().any(|row| row.contains("three")));
        assert!(rows.iter().any(|row| row.contains("two")));
        assert!(
            !rows.iter().any(|row| row.contains("one")),
            "a toast that does not fit is dropped whole, not clipped"
        );
        let corner = ToasterStyle::fallback()
            .border_style
            .to_border_set()
            .top_left;
        assert_eq!(
            symbol_count(&buf, corner),
            2,
            "exactly the fitting toasts render"
        );
    }

    #[test]
    fn zero_gap_stacks_toasts_on_adjacent_rows() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("a"), Duration::ZERO);
        toasts.push(Toast::new("b"), Duration::ZERO);
        let area = Rect::new(0, 0, 12, 6);
        let mut buf = Buffer::empty(area);

        ToasterWidget::new(&toasts, Duration::ZERO)
            .toast_width(9)
            .gap(0)
            .inset(0, 0)
            .position(ToastPosition::TopLeft)
            .render(area, &mut buf);

        let corner = ToasterStyle::fallback()
            .border_style
            .to_border_set()
            .top_left;
        assert_eq!(buf[(0, 0)].symbol(), corner);
        assert_eq!(buf[(0, 3)].symbol(), corner);
    }

    #[test]
    fn wrapped_toast_stacks_whole_below_without_overlap() {
        let mut toasts = ToasterState::new();
        toasts.push(
            Toast::new("first").description(LONG_DESCRIPTION),
            Duration::ZERO,
        );
        toasts.push(Toast::new("second"), Duration::ZERO);
        let area = Rect::new(0, 0, 40, 20);
        let mut buf = Buffer::empty(area);
        ToasterWidget::new(&toasts, Duration::ZERO)
            .toast_width(20)
            .render(area, &mut buf);

        // Bottom-right: the newest toast hugs the bottom (rows 16..=18); the
        // older toast gets its full wrapped height (1 title + 4 description
        // rows at 16 content columns, rows 8..=14) above the gap row.
        let rows = painted_rows(&buf);
        assert_eq!(row_of(&rows, "second"), 17);
        assert_eq!(row_of(&rows, "first"), 9);
        assert_eq!(row_of(&rows, "description that"), 11);
        assert_eq!(row_of(&rows, "wraps onto"), 12);
        assert_eq!(row_of(&rows, "several rows"), 13);
        assert!(rows[15].trim().is_empty(), "the gap row stays blank");
        let set = ToasterStyle::fallback().border_style.to_border_set();
        assert_eq!(symbol_count(&buf, set.top_left), 2);
        assert_eq!(symbol_count(&buf, set.bottom_left), 2);
    }

    #[test]
    fn top_position_stacks_newest_first_without_overlap() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("earlier"), Duration::ZERO);
        toasts.push(
            Toast::new("latest").description(LONG_DESCRIPTION),
            Duration::ZERO,
        );
        let area = Rect::new(0, 0, 40, 20);
        let mut buf = Buffer::empty(area);
        ToasterWidget::new(&toasts, Duration::ZERO)
            .toast_width(20)
            .position(ToastPosition::TopLeft)
            .render(area, &mut buf);

        // Top-left: the newest toast starts at the inset row and occupies its
        // full wrapped height (rows 1..=7); the older toast follows after the
        // gap row (rows 9..=11).
        let rows = painted_rows(&buf);
        assert_eq!(row_of(&rows, "latest"), 2);
        assert_eq!(row_of(&rows, "several rows"), 6);
        assert!(rows[8].trim().is_empty(), "the gap row stays blank");
        assert_eq!(row_of(&rows, "earlier"), 10);
    }

    #[test]
    fn center_and_bottom_left_positions_anchor_the_stack() {
        let cases = [
            (ToastPosition::TopCenter, (6, 0)),
            (ToastPosition::BottomCenter, (6, 6)),
            (ToastPosition::BottomLeft, (0, 6)),
        ];
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("x"), Duration::ZERO);
        let area = Rect::new(0, 0, 21, 9);
        let corner = ToasterStyle::fallback()
            .border_style
            .to_border_set()
            .top_left;

        for (position, (x, y)) in cases {
            let mut buf = Buffer::empty(area);
            ToasterWidget::new(&toasts, Duration::ZERO)
                .toast_width(9)
                .inset(0, 0)
                .position(position)
                .render(area, &mut buf);

            assert_eq!(
                buf[(x, y)].symbol(),
                corner,
                "wrong anchor for {position:?}"
            );
        }
    }

    #[test]
    fn borderless_toast_wraps_at_the_full_stack_width() {
        let mut toasts = ToasterState::new();
        toasts.push(
            Toast::new("saved")
                .description(LONG_DESCRIPTION)
                .border(false),
            Duration::ZERO,
        );
        let area = Rect::new(0, 0, 30, 12);
        let mut buf = Buffer::empty(area);
        ToasterWidget::new(&toasts, Duration::ZERO)
            .toast_width(20)
            .position(ToastPosition::TopLeft)
            .render(area, &mut buf);

        // No border rows or columns: the title paints on the inset row and
        // the description wraps at all 20 content columns (4 rows), ending
        // exactly where the measured height says.
        let rows = painted_rows(&buf);
        assert_eq!(row_of(&rows, "saved"), 1);
        assert_eq!(row_of(&rows, "rows"), 5);
        assert!(
            rows[6].trim().is_empty(),
            "nothing paints past the last wrapped row"
        );
        let corner = ToasterStyle::fallback()
            .border_style
            .to_border_set()
            .top_left;
        assert_eq!(symbol_count(&buf, corner), 0);
    }

    #[test]
    fn cjk_content_wraps_by_display_cells() {
        let mut toasts = ToasterState::new();
        toasts.push(
            Toast::new("状態").description("日本語のテキスト"),
            Duration::ZERO,
        );
        let area = Rect::new(0, 0, 14, 8);
        let mut buf = Buffer::empty(area);
        ToasterWidget::new(&toasts, Duration::ZERO)
            .toast_width(12)
            .position(ToastPosition::TopLeft)
            .render(area, &mut buf);

        // 12 stack columns leave 8 content cells, so the 16-cell description
        // wraps into two full rows of wide glyphs. Cell positions assert the
        // display-cell rules: each glyph lands two cells after the previous.
        assert_eq!(buf[(6, 2)].symbol(), "状");
        assert_eq!(buf[(3, 3)].symbol(), "日");
        assert_eq!(buf[(9, 3)].symbol(), "の");
        assert_eq!(buf[(3, 4)].symbol(), "テ");
        assert_eq!(buf[(9, 4)].symbol(), "ト");
        // The bottom border lands below both wrapped rows, not through them.
        let set = ToasterStyle::fallback().border_style.to_border_set();
        assert_eq!(buf[(1, 5)].symbol(), set.bottom_left);
    }

    #[test]
    fn emoji_title_wraps_with_a_hanging_indent() {
        let mut toasts = ToasterState::new();
        toasts.push(Toast::new("🚀 saved"), Duration::ZERO);
        let area = Rect::new(0, 0, 20, 8);
        let mut buf = Buffer::empty(area);
        ToasterWidget::new(&toasts, Duration::ZERO)
            .toast_width(14)
            .position(ToastPosition::TopLeft)
            .render(area, &mut buf);

        // 14 stack columns leave 10 content cells and 7 title cells after the
        // icon; "🚀 saved" is 8 cells, so the title wraps and the second row
        // hang-indents under the icon.
        assert_eq!(buf[(6, 2)].symbol(), "🚀");
        assert_eq!(buf[(6, 3)].symbol(), "s");
        assert_eq!(buf[(10, 3)].symbol(), "d");
        let set = ToasterStyle::fallback().border_style.to_border_set();
        assert_eq!(buf[(1, 4)].symbol(), set.bottom_left);
    }

    #[test]
    fn wide_area_paints_at_the_stack_width_not_the_area_width() {
        let mut toasts = ToasterState::new();
        toasts.push(
            Toast::new("saved").description(LONG_DESCRIPTION),
            Duration::ZERO,
        );
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);
        ToasterWidget::new(&toasts, Duration::ZERO)
            .position(ToastPosition::TopRight)
            .render(area, &mut buf);

        // The default 42-column stack hugs the right inset edge; the
        // description wraps at 38 content cells into two rows, both painted.
        let set = ToasterStyle::fallback().border_style.to_border_set();
        assert_eq!(buf[(37, 1)].symbol(), set.top_left);
        assert_eq!(buf[(78, 1)].symbol(), set.top_right);
        assert_eq!(buf[(37, 5)].symbol(), set.bottom_left);
        let rows = painted_rows(&buf);
        assert_eq!(row_of(&rows, "onto several rows"), 4);
    }
}
