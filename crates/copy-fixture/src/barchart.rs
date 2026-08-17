use ratatui::{
    buffer::Buffer,
    layout::{Direction, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::Line,
    widgets::{Bar, BarChart, BarGroup, Widget},
};

use ratcn::Theme;

/// Colors for a [`BarChartWidget`].
///
/// A chart has no interaction states, so unlike the other style structs there is
/// one color per role rather than one per role per state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarChartStyle {
    /// Default text color for anything not covered by the roles below.
    pub foreground: Color,
    /// Fill behind the whole chart.
    pub background: Color,
    /// The bars themselves.
    pub bar: Color,
    /// The value printed inside each bar, so it must contrast with `bar`.
    pub value_foreground: Color,
    /// Vertical bar labels and group labels. Ratatui's horizontal renderer does
    /// not apply its chart-level label style to ordinary bar labels; style those
    /// labels directly on their [`Line`](ratatui::text::Line) or spans.
    pub label_foreground: Color,
}

impl BarChartStyle {
    /// A neutral style using plain ANSI colors, for painting without a
    /// [`Theme`]. Prefer [`from_theme`](Self::from_theme) when one is available.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            foreground: Color::Reset,
            background: Color::Reset,
            bar: Color::Cyan,
            value_foreground: Color::Reset,
            label_foreground: Color::DarkGray,
        }
    }

    /// Derive chart colors from `theme`. Bars use the primary color, with the
    /// value drawn in the primary foreground so it stays legible on top.
    #[must_use]
    pub const fn from_theme(theme: &Theme) -> Self {
        Self {
            foreground: theme.foreground,
            background: theme.field,
            bar: theme.primary,
            value_foreground: theme.primary_foreground,
            label_foreground: theme.muted_foreground,
        }
    }
}

/// A themed adapter over ratatui's [`BarChart`], adding grouping and a value
/// display switch.
///
/// It is not a component in the sense the rest of this crate uses the word.
/// Every bar, label, and value is a ratatui type, the drawing is ratatui's, and
/// what this adds on top is three things:
///
/// - theme colors, from a [`Theme`] via [`themed`](Self::themed) or an explicit
///   [`BarChartStyle`]
/// - [`grouped`](Self::grouped) charts held as [`BarChartGroup`], so
///   widget-level options reach grouped bars too
/// - [`show_values`](Self::show_values), one switch for whether bars print
///   their value
///
/// Anything else you want from a bar chart, you configure on the ratatui
/// [`Bar`]s you pass in — including per-bar color, see
/// [Per-bar colors](#per-bar-colors) below.
///
/// Charts take no focus and handle no events, so there is no interactive half.
///
/// **Usable in any ratatui app.** Nothing here depends on
/// [`Ratcn`](ratcn::runtime::Ratcn) or the component layer — it is an ordinary
/// [`Widget`], so `frame.render_widget(...)` is all it needs.
///
/// # Per-bar colors
///
/// Bars are passed through to ratatui untouched, so ratatui's own
/// [`Bar::style`] works: it patches over the chart-wide bar color, which is how
/// you color one bar, or one series inside a [`grouped`](Self::grouped) chart.
///
/// ```
/// # use ratatui::{style::{Color, Style}, widgets::Bar};
/// # use ratcn::BarChartWidget;
/// let bars = vec![
///     Bar::default().value(12),
///     Bar::default().value(18).style(Style::default().fg(Color::Red)),
/// ];
/// # let _ = BarChartWidget::new(bars);
/// ```
///
/// The value text printed inside a bar is not covered by this: it keeps the
/// [`value_foreground`](BarChartStyle::value_foreground) on the chart-wide
/// [`bar`](BarChartStyle::bar) background, so a recolored bar shows its value
/// against the chart's bar color rather than its own. Set
/// [`Bar::value_style`] on that bar to match, or turn values off with
/// [`show_values`](Self::show_values).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarChartWidget<'a> {
    /// Every chart is a list of groups, so measurement and painting have one
    /// shape to handle. Ungrouped bars are one unlabeled group, and groups with
    /// no bars are dropped on the way in — ratatui drops them before painting,
    /// and a group that never paints must not be measured either.
    groups: Vec<BarChartGroup<'a>>,
    style: BarChartStyle,
    max_value: Option<u64>,
    direction: Direction,
    bar_width: u16,
    bar_gap: u16,
    group_gap: u16,
    bar_set: symbols::bar::Set<'a>,
    show_values: bool,
}

/// One group of bars for [`BarChartWidget::grouped`]. Mirrors ratatui's
/// [`BarGroup`] but keeps its bars readable, so widget-level options such as
/// [`BarChartWidget::show_values`] apply to grouped bars too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarChartGroup<'a> {
    label: Option<Line<'a>>,
    bars: Vec<Bar<'a>>,
}

impl<'a> BarChartGroup<'a> {
    /// A group of bars with no group label.
    #[must_use]
    pub fn new(bars: impl IntoIterator<Item = Bar<'a>>) -> Self {
        Self {
            label: None,
            bars: bars.into_iter().collect(),
        }
    }

    /// Label the group as a whole — the category name under a cluster, where
    /// each bar inside carries its own series label.
    #[must_use]
    pub fn label(mut self, label: impl Into<Line<'a>>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl<'a> BarChartWidget<'a> {
    /// A vertical chart of `bars`, using [`BarChartStyle::fallback`].
    #[must_use]
    pub fn new(bars: impl IntoIterator<Item = Bar<'a>>) -> Self {
        Self::with_groups([BarChartGroup::new(bars)])
    }

    /// The same as [`new`](Self::new), named for when the direction matters to
    /// the reader.
    #[must_use]
    pub fn vertical(bars: impl IntoIterator<Item = Bar<'a>>) -> Self {
        Self::new(bars)
    }

    /// A chart whose bars run left to right. Useful when labels are long, since
    /// a horizontal bar has a whole row for its label.
    #[must_use]
    pub fn horizontal(bars: impl IntoIterator<Item = Bar<'a>>) -> Self {
        Self::new(bars).direction(Direction::Horizontal)
    }

    /// A chart of clustered bars, for comparing several series across
    /// categories. See [`BarChartGroup`]. Use
    /// [`direction`](Self::direction) to make a grouped chart horizontal. A
    /// horizontal group label needs a non-zero [`group_gap`](Self::group_gap).
    ///
    /// A group with no bars is dropped: it paints nothing, so it occupies no
    /// space and adds no [`group_gap`](Self::group_gap) either. Filtered data
    /// can therefore be passed straight in without collapsing the gaps by hand.
    #[must_use]
    pub fn grouped(groups: impl IntoIterator<Item = BarChartGroup<'a>>) -> Self {
        Self::with_groups(groups)
    }

    fn with_groups(groups: impl IntoIterator<Item = BarChartGroup<'a>>) -> Self {
        Self {
            groups: groups
                .into_iter()
                .filter(|group| !group.bars.is_empty())
                .collect(),
            style: BarChartStyle::fallback(),
            max_value: None,
            direction: Direction::Vertical,
            bar_width: 3,
            bar_gap: 1,
            group_gap: 0,
            bar_set: symbols::bar::NINE_LEVELS,
            show_values: true,
        }
    }

    /// Take colors from `theme`.
    #[must_use]
    pub const fn themed(mut self, theme: &Theme) -> Self {
        self.style = BarChartStyle::from_theme(theme);
        self
    }

    /// Use these exact colors, ignoring any theme.
    #[must_use]
    pub const fn style(mut self, style: BarChartStyle) -> Self {
        self.style = style;
        self
    }

    /// Pin the value a full-height bar represents.
    ///
    /// By default the tallest bar fills the chart, which rescales the whole
    /// chart whenever the data changes — fine for a snapshot, misleading for
    /// something updating live. Set this to keep the scale steady across
    /// frames, or to compare two charts against each other.
    #[must_use]
    pub const fn max_value(mut self, max_value: u64) -> Self {
        self.max_value = Some(max_value);
        self
    }

    /// Whether bars run up (`Vertical`, the default) or across (`Horizontal`).
    ///
    /// This takes ratatui's [`ratatui::layout::Direction`] — the layout axis.
    /// The runtime's own `Forward`/`Backward` enum for stepping through items
    /// is [`Step`](ratcn::runtime::Step), a different type under a different
    /// name.
    #[must_use]
    pub const fn direction(mut self, direction: Direction) -> Self {
        self.direction = direction;
        self
    }

    /// Whether each bar prints its value inside it. On by default; turn it off
    /// for narrow bars where the number will not fit.
    ///
    /// Applies to grouped charts too, which is why groups are held as
    /// [`BarChartGroup`] rather than ratatui's `BarGroup`.
    #[must_use]
    pub const fn show_values(mut self, show_values: bool) -> Self {
        self.show_values = show_values;
        self
    }

    /// Minimum width for a vertical chart's bar grouping axis, in cells.
    ///
    /// Horizontal charts size their bar lengths from the supplied paint area, so
    /// this returns zero for them. Group labels and value text can require more
    /// space on the other axis and are intentionally not guessed here.
    #[must_use]
    pub fn width(&self) -> u16 {
        if self.direction == Direction::Vertical {
            self.grouping_span()
        } else {
            0
        }
    }

    /// Minimum height for a horizontal chart's bar grouping axis, in rows.
    ///
    /// Vertical charts size their bar lengths from the supplied paint area, so
    /// this returns zero for them. Group labels and value text can require more
    /// space on the other axis and are intentionally not guessed here.
    #[must_use]
    pub fn height(&self) -> u16 {
        if self.direction == Direction::Horizontal {
            self.grouping_span()
        } else {
            0
        }
    }

    /// Cells across each bar. Defaults to 3, which leaves room for a two-digit
    /// value.
    #[must_use]
    pub const fn bar_width(mut self, width: u16) -> Self {
        self.bar_width = width;
        self
    }

    /// Blank cells between adjacent bars. Defaults to 1.
    #[must_use]
    pub const fn bar_gap(mut self, gap: u16) -> Self {
        self.bar_gap = gap;
        self
    }

    /// Extra blank cells between groups, on top of [`bar_gap`](Self::bar_gap).
    /// Defaults to 0, and only matters for a [`grouped`](Self::grouped) chart.
    /// Horizontal group labels are drawn in this gap, so they require a value
    /// greater than 0.
    #[must_use]
    pub const fn group_gap(mut self, gap: u16) -> Self {
        self.group_gap = gap;
        self
    }

    /// The glyphs used to draw vertical bars.
    ///
    /// A vertical bar rarely ends exactly on a cell boundary, so its top cell
    /// is drawn with a partial block. The default (`NINE_LEVELS`) gives the
    /// smoothest result; coarser sets exist for terminals whose fonts lack
    /// those glyphs. Horizontal bars use only whole `full` and `empty` cells.
    #[must_use]
    pub fn bar_set(mut self, bar_set: symbols::bar::Set<'a>) -> Self {
        self.bar_set = bar_set;
        self
    }

    /// Cells the whole chart spans along its grouping axis.
    ///
    /// A [`bar_gap`](Self::bar_gap) sits between every adjacent pair of bars,
    /// including the pair either side of a group boundary — ratatui advances by
    /// `bar_gap + bar_width` after every bar and only then adds the
    /// [`group_gap`](Self::group_gap), which is why that gap is documented as
    /// extra space *on top of* the bar gap. So the span is every bar's width,
    /// one bar gap per gap between bars, and one group gap per boundary.
    fn grouping_span(&self) -> u16 {
        let bars: usize = self.groups.iter().map(|group| group.bars.len()).sum();
        let boundaries = u16::try_from(self.groups.len().saturating_sub(1)).unwrap_or(u16::MAX);
        bar_span(bars, self.bar_width, self.bar_gap)
            .saturating_add(boundaries.saturating_mul(self.group_gap))
    }
}

/// Cells `count` bars of `width` span with a `gap` between adjacent pairs.
fn bar_span(count: usize, width: u16, gap: u16) -> u16 {
    let count = u16::try_from(count).unwrap_or(u16::MAX);
    count
        .saturating_mul(width)
        .saturating_add(count.saturating_sub(1).saturating_mul(gap))
}

impl<'a> Widget for BarChartWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let show_values = self.show_values;
        let strip_values = move |bars: Vec<Bar<'a>>| -> Vec<Bar<'a>> {
            if show_values {
                bars
            } else {
                bars.into_iter().map(|bar| bar.text_value("")).collect()
            }
        };
        let groups: Vec<BarGroup<'a>> = self
            .groups
            .into_iter()
            .map(|group| {
                let bars = strip_values(group.bars);
                match group.label {
                    Some(label) => BarGroup::with_label(label, bars),
                    None => BarGroup::new(bars),
                }
            })
            .collect();
        let mut chart = BarChart::grouped(groups)
            .style(
                Style::default()
                    .fg(self.style.foreground)
                    .bg(self.style.background),
            )
            .bar_style(
                Style::default()
                    .fg(self.style.bar)
                    .bg(self.style.background),
            )
            .value_style(
                Style::default()
                    .fg(self.style.value_foreground)
                    .bg(self.style.bar)
                    .add_modifier(Modifier::BOLD),
            )
            .label_style(
                Style::default()
                    .fg(self.style.label_foreground)
                    .bg(self.style.background),
            )
            .bar_width(self.bar_width)
            .bar_gap(self.bar_gap)
            .group_gap(self.group_gap)
            .bar_set(self.bar_set)
            .direction(self.direction);
        if let Some(max_value) = self.max_value {
            chart = chart.max(max_value);
        }
        chart.render(area, buf);
    }
}
