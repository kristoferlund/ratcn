use ratatui::{
    buffer::Buffer,
    layout::{Direction, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::Line,
    widgets::{Bar, BarChart, BarGroup, Widget},
};

use crate::Theme;

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
/// [`Ratcn`](crate::runtime::Ratcn) or the component layer — it is an ordinary
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
    /// is [`Step`](crate::runtime::Step), a different type under a different
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
    /// space on the other axis; this measures the bar axis alone.
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
    /// space on the other axis; this measures the bar axis alone.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The constructors take an iterator, as `List`, `Select`, and `Tabs` do,
    /// so mapped or filtered data goes straight in.
    #[test]
    fn constructors_accept_any_iterator_of_bars() {
        let readings = [3u64, 0, 7];
        let from_iterator = BarChartWidget::new(
            readings
                .iter()
                .filter(|value| **value > 0)
                .map(|value| Bar::default().value(*value)),
        );
        let from_vec = BarChartWidget::new(vec![Bar::default().value(3), Bar::default().value(7)]);
        assert_eq!(
            from_iterator.width(),
            from_vec.width(),
            "the filtered iterator yields the same two bars a vec would"
        );

        let grouped = BarChartWidget::grouped(
            ["a", "b"]
                .into_iter()
                .map(|label| BarChartGroup::new([Bar::default().value(1)]).label(label)),
        );
        assert_eq!(
            grouped.width(),
            BarChartWidget::grouped(vec![
                BarChartGroup::new([Bar::default().value(1)]).label("a"),
                BarChartGroup::new([Bar::default().value(1)]).label("b"),
            ])
            .width()
        );
    }

    #[test]
    fn themed_bars_paint_the_primary_color() {
        let theme = Theme::default_dark();
        let area = Rect::new(0, 0, 3, 3);
        let mut buffer = Buffer::empty(area);

        BarChartWidget::new(vec![Bar::default().value(9)])
            .themed(&theme)
            .show_values(false)
            .render(area, &mut buffer);

        // The themed style paints bars in `primary` on the inset `field` well.
        let cell = buffer.cell((0, 2)).expect("bar cell");
        assert_eq!(cell.fg, theme.primary);
        assert_eq!(cell.bg, theme.field);
    }

    #[test]
    fn explicit_style_overrides_paint_exact_colors() {
        let style = BarChartStyle {
            foreground: Color::White,
            background: Color::Black,
            bar: Color::Magenta,
            value_foreground: Color::Yellow,
            label_foreground: Color::Gray,
        };
        let area = Rect::new(0, 0, 1, 1);
        let mut buffer = Buffer::empty(area);

        BarChartWidget::new(vec![Bar::default().value(1)])
            .style(style)
            .max_value(1)
            .show_values(false)
            .bar_width(1)
            .bar_gap(0)
            .render(area, &mut buffer);

        let cell = buffer.cell((0, 0)).expect("bar cell");
        assert_eq!(cell.fg, style.bar);
        assert_eq!(cell.bg, style.background);
    }

    #[test]
    fn per_bar_style_patches_over_the_chart_bar_color() {
        // Per-bar color is documented as already reachable through ratatui's
        // own `Bar::style`, so the pass-through must stay intact: bars are
        // handed to ratatui unmodified, and its per-bar style wins.
        let style = BarChartStyle {
            bar: Color::Magenta,
            ..BarChartStyle::fallback()
        };
        let area = Rect::new(0, 0, 2, 1);
        let mut buffer = Buffer::empty(area);

        BarChartWidget::new(vec![
            Bar::default().value(1),
            Bar::default()
                .value(1)
                .style(Style::default().fg(Color::Red)),
        ])
        .style(style)
        .max_value(1)
        .show_values(false)
        .bar_width(1)
        .bar_gap(0)
        .render(area, &mut buffer);

        assert_eq!(
            buffer.cell((0, 0)).expect("chart-colored bar").fg,
            style.bar
        );
        assert_eq!(buffer.cell((1, 0)).expect("per-bar color").fg, Color::Red);
    }

    #[test]
    fn a_recolored_bar_keeps_the_chart_wide_value_background() {
        // The documented caveat to per-bar color: the value text is painted
        // with the chart's value style, so it does not follow `Bar::style`.
        let style = BarChartStyle {
            bar: Color::Magenta,
            value_foreground: Color::Yellow,
            ..BarChartStyle::fallback()
        };
        let area = Rect::new(0, 0, 1, 1);
        let mut buffer = Buffer::empty(area);

        BarChartWidget::new(vec![
            Bar::default()
                .value(1)
                .style(Style::default().fg(Color::Red)),
        ])
        .style(style)
        .max_value(1)
        .bar_width(1)
        .bar_gap(0)
        .render(area, &mut buffer);

        let cell = buffer.cell((0, 0)).expect("value cell");
        assert_eq!(cell.fg, style.value_foreground);
        assert_eq!(cell.bg, style.bar);
    }

    #[test]
    fn max_value_pins_the_scale_so_a_sub_max_bar_leaves_headroom() {
        // Without `max_value` the tallest bar always fills the chart; pinning
        // the maximum is what keeps the scale steady across frames.
        let area = Rect::new(0, 0, 1, 2);
        let mut buffer = Buffer::empty(area);

        BarChartWidget::new(vec![Bar::default().value(1)])
            .max_value(2)
            .show_values(false)
            .bar_width(1)
            .bar_gap(0)
            .render(area, &mut buffer);

        assert_eq!(buffer.cell((0, 0)).expect("headroom cell").symbol(), " ");
        assert_eq!(buffer.cell((0, 1)).expect("bar cell").symbol(), "█");
    }

    #[test]
    fn horizontal_bars_render_left_to_right() {
        let area = Rect::new(0, 0, 4, 1);
        let mut buffer = Buffer::empty(area);

        BarChartWidget::horizontal(vec![Bar::default().value(1)])
            .max_value(1)
            .show_values(false)
            .bar_width(1)
            .bar_gap(0)
            .render(area, &mut buffer);

        assert_eq!(buffer.cell((3, 0)).expect("bar end cell").symbol(), "█");
    }

    #[test]
    fn horizontal_ordinary_labels_keep_their_own_style() {
        let mut style = BarChartStyle::fallback();
        style.foreground = Color::White;
        style.label_foreground = Color::Yellow;
        style.value_foreground = Color::Green;
        let area = Rect::new(0, 0, 10, 1);
        let mut buffer = Buffer::empty(area);

        BarChartWidget::horizontal(vec![Bar::default().label("Plain").value(1)])
            .style(style)
            .max_value(1)
            .bar_width(1)
            .bar_gap(0)
            .render(area, &mut buffer);

        assert_eq!(
            buffer.cell((0, 0)).expect("label cell").fg,
            style.foreground,
            "ratatui does not apply chart label_style to ordinary horizontal labels"
        );
        assert_eq!(
            buffer.cell((6, 0)).expect("value cell").fg,
            style.value_foreground
        );
    }

    #[test]
    fn direction_builder_changes_the_chart_orientation() {
        let area = Rect::new(0, 0, 4, 1);
        let mut buffer = Buffer::empty(area);

        BarChartWidget::new(vec![Bar::default().value(1)])
            .direction(Direction::Horizontal)
            .max_value(1)
            .show_values(false)
            .bar_width(1)
            .bar_gap(0)
            .render(area, &mut buffer);

        assert_eq!(buffer.cell((3, 0)).expect("bar end cell").symbol(), "█");
    }

    #[test]
    fn custom_bar_set_controls_rendered_symbols() {
        let area = Rect::new(0, 0, 1, 1);
        let mut buffer = Buffer::empty(area);
        let bar_set = symbols::bar::Set {
            full: "x",
            seven_eighths: "x",
            three_quarters: "x",
            five_eighths: "x",
            half: "x",
            three_eighths: "x",
            one_quarter: "x",
            one_eighth: "x",
            empty: ".",
        };

        BarChartWidget::new(vec![Bar::default().value(1)])
            .max_value(1)
            .show_values(false)
            .bar_width(1)
            .bar_gap(0)
            .bar_set(bar_set)
            .render(area, &mut buffer);

        assert_eq!(buffer.cell((0, 0)).expect("bar cell").symbol(), "x");
    }

    #[test]
    fn group_gap_separates_two_groups() {
        let area = Rect::new(0, 0, 3, 1);
        let mut buffer = Buffer::empty(area);
        let groups = vec![
            BarChartGroup::new(vec![Bar::default().value(1)]),
            BarChartGroup::new(vec![Bar::default().value(1)]),
        ];

        BarChartWidget::grouped(groups)
            .max_value(1)
            .bar_width(1)
            .bar_gap(0)
            .group_gap(1)
            .show_values(false)
            .render(area, &mut buffer);

        assert_eq!(buffer.cell((0, 0)).expect("first bar").symbol(), "█");
        assert_eq!(buffer.cell((1, 0)).expect("group gap").symbol(), " ");
        assert_eq!(buffer.cell((2, 0)).expect("second bar").symbol(), "█");
    }

    #[test]
    fn grouped_horizontal_charts_render_bars_and_group_labels() {
        let area = Rect::new(0, 0, 4, 4);
        let mut buffer = Buffer::empty(area);
        let groups = vec![
            BarChartGroup::new(vec![Bar::default().value(1)]).label("G1"),
            BarChartGroup::new(vec![Bar::default().value(1)]).label("G2"),
        ];

        BarChartWidget::grouped(groups)
            .direction(Direction::Horizontal)
            .max_value(1)
            .bar_width(1)
            .bar_gap(0)
            .group_gap(1)
            .show_values(false)
            .render(area, &mut buffer);

        assert_eq!(buffer.cell((3, 0)).expect("first bar end").symbol(), "█");
        assert_eq!(
            buffer.cell((0, 1)).expect("first group label").symbol(),
            "G"
        );
        assert_eq!(buffer.cell((3, 2)).expect("second bar end").symbol(), "█");
        assert_eq!(
            buffer.cell((0, 3)).expect("second group label").symbol(),
            "G"
        );
    }

    #[test]
    fn grouped_horizontal_labels_distinguish_bar_and_group_style() {
        use ratatui::style::Stylize;

        let mut style = BarChartStyle::fallback();
        style.foreground = Color::White;
        style.label_foreground = Color::Yellow;
        let area = Rect::new(0, 0, 10, 3);
        let mut buffer = Buffer::empty(area);
        let groups = vec![
            BarChartGroup::new(vec![
                Bar::default().label("Plain").value(1),
                Bar::default().label(Line::from("Red").red()).value(1),
            ])
            .label("G"),
        ];

        BarChartWidget::grouped(groups)
            .style(style)
            .direction(Direction::Horizontal)
            .max_value(1)
            .bar_width(1)
            .bar_gap(0)
            .group_gap(1)
            .show_values(false)
            .render(area, &mut buffer);

        assert_eq!(
            buffer.cell((0, 0)).expect("plain bar label").fg,
            style.foreground
        );
        assert_eq!(
            buffer.cell((0, 1)).expect("explicit bar label").fg,
            Color::Red
        );
        assert_eq!(
            buffer.cell((6, 2)).expect("group label").fg,
            style.label_foreground
        );
    }

    #[test]
    fn custom_text_value_is_preserved() {
        let area = Rect::new(0, 0, 2, 1);
        let mut buffer = Buffer::empty(area);

        BarChartWidget::new(vec![Bar::default().value(1).text_value("ok")])
            .max_value(1)
            .bar_width(2)
            .bar_gap(0)
            .render(area, &mut buffer);

        assert_eq!(buffer.cell((0, 0)).expect("value start").symbol(), "o");
        assert_eq!(buffer.cell((1, 0)).expect("value end").symbol(), "k");
    }

    #[test]
    fn show_values_false_hides_standalone_bar_values() {
        // Values paint by default; `show_values(false)` strips them so the
        // cell shows the bar glyph instead of the value text.
        let area = Rect::new(0, 0, 1, 2);
        let mut with_values = Buffer::empty(area);
        let mut without_values = Buffer::empty(area);
        let bars = || vec![Bar::default().value(9)];

        BarChartWidget::new(bars())
            .max_value(9)
            .bar_width(1)
            .bar_gap(0)
            .render(area, &mut with_values);
        BarChartWidget::new(bars())
            .max_value(9)
            .bar_width(1)
            .bar_gap(0)
            .show_values(false)
            .render(area, &mut without_values);

        assert_eq!(with_values.cell((0, 1)).expect("value cell").symbol(), "9");
        assert_eq!(without_values.cell((0, 1)).expect("bar cell").symbol(), "█");
    }

    #[test]
    fn vertical_width_measures_exactly_what_paints() {
        // Three 2-wide bars with 1-cell gaps: width() must be 2+1+2+1+2 = 8,
        // and a buffer of exactly that width must hold the last bar whole.
        let chart = || {
            BarChartWidget::new(vec![
                Bar::default().value(1),
                Bar::default().value(1),
                Bar::default().value(1),
            ])
            .max_value(1)
            .show_values(false)
            .bar_width(2)
            .bar_gap(1)
        };
        let width = chart().width();
        assert_eq!(width, 8);

        let area = Rect::new(0, 0, width, 1);
        let mut buffer = Buffer::empty(area);
        chart().render(area, &mut buffer);
        // The rightmost cell of the final bar is painted: nothing is truncated
        // when the area matches the measurement.
        assert_eq!(
            buffer.cell((width - 1, 0)).expect("last bar end").symbol(),
            "█"
        );

        // One cell narrower and the final bar no longer fits whole, so the cell
        // where its right edge would land is not a bar cell — the measurement
        // is exact, not merely sufficient.
        let narrow = Rect::new(0, 0, width - 1, 1);
        let mut narrow_buffer = Buffer::empty(narrow);
        chart().render(narrow, &mut narrow_buffer);
        assert_ne!(
            narrow_buffer.cell((width - 2, 0)).expect("cell").symbol(),
            "█"
        );
    }

    #[test]
    fn grouped_horizontal_height_measures_exactly_what_paints() {
        // Two groups of two 1-row bars with a 1-row group gap: height() must be
        // 2+1+2 = 5, and a buffer of exactly that height holds the last bar.
        let chart = || {
            BarChartWidget::grouped(vec![
                BarChartGroup::new(vec![Bar::default().value(1), Bar::default().value(1)]),
                BarChartGroup::new(vec![Bar::default().value(1), Bar::default().value(1)]),
            ])
            .direction(Direction::Horizontal)
            .max_value(1)
            .show_values(false)
            .bar_width(1)
            .bar_gap(0)
            .group_gap(1)
        };
        let height = chart().height();
        assert_eq!(height, 5);

        let area = Rect::new(0, 0, 4, height);
        let mut buffer = Buffer::empty(area);
        chart().render(area, &mut buffer);
        // The bottom cell of the final bar is painted on the last measured row.
        assert_eq!(
            buffer.cell((3, height - 1)).expect("last bar end").symbol(),
            "█"
        );

        // One row shorter loses the final bar entirely.
        let bar_cells = |buffer: &Buffer| {
            buffer
                .content()
                .iter()
                .filter(|cell| cell.symbol() == "█")
                .count()
        };
        let short = Rect::new(0, 0, 4, height - 1);
        let mut short_buffer = Buffer::empty(short);
        chart().render(short, &mut short_buffer);
        assert!(bar_cells(&short_buffer) < bar_cells(&buffer));
    }

    #[test]
    fn empty_groups_change_neither_the_measured_nor_the_painted_chart() {
        // A group with no bars paints nothing — ratatui drops it before
        // painting — so it must not claim a group gap in the measurement
        // either, wherever in the list it sits. A chart measured wider than it
        // paints leaves a stripe of unused cells in every layout that trusts
        // `width()`/`height()`.
        let bar_cells = |buffer: &Buffer| {
            buffer
                .content()
                .iter()
                .filter(|cell| cell.symbol() == "█")
                .count()
        };
        for direction in [Direction::Vertical, Direction::Horizontal] {
            // Swept over the bar gap because a boundary costs a bar gap as well
            // as a group gap: at `bar_gap(0)` an over-measured empty group and
            // an under-measured boundary would have hidden each other.
            for bar_gap in [0u16, 1, 2] {
                let chart = |groups: Vec<BarChartGroup<'static>>| {
                    BarChartWidget::grouped(groups)
                        .direction(direction)
                        .max_value(1)
                        .show_values(false)
                        .bar_width(1)
                        .bar_gap(bar_gap)
                        .group_gap(1)
                };
                let bar = || BarChartGroup::new(vec![Bar::default().value(1)]);
                let empty = || BarChartGroup::new(Vec::new());
                let grouping_span = |chart: &BarChartWidget<'_>| {
                    if direction == Direction::Vertical {
                        chart.width()
                    } else {
                        chart.height()
                    }
                };
                let area = |span: u16| {
                    if direction == Direction::Vertical {
                        Rect::new(0, 0, span, 1)
                    } else {
                        Rect::new(0, 0, 1, span)
                    }
                };
                let label = format!("{direction:?} bar_gap {bar_gap}");

                let two_groups = chart(vec![bar(), bar()]);
                // Two 1-cell bars either side of one boundary, which costs the
                // bar gap plus the 1-cell group gap.
                let measured = grouping_span(&two_groups);
                assert_eq!(measured, 3 + bar_gap, "{label}");
                let mut painted = Buffer::empty(area(measured));
                two_groups.render(area(measured), &mut painted);
                assert_eq!(bar_cells(&painted), 2, "{label} paints both bars");

                for (position, groups) in [
                    ("leading", vec![empty(), bar(), bar()]),
                    ("middle", vec![bar(), empty(), bar()]),
                    ("trailing", vec![bar(), bar(), empty()]),
                ] {
                    let with_empty = chart(groups);
                    assert_eq!(
                        grouping_span(&with_empty),
                        measured,
                        "a {position} empty group must not be measured ({label})"
                    );
                    let mut buffer = Buffer::empty(area(measured));
                    with_empty.render(area(measured), &mut buffer);
                    assert_eq!(
                        buffer, painted,
                        "a {position} empty group must not shift what paints ({label})"
                    );
                }
            }
        }
    }

    #[test]
    fn a_grouped_chart_measures_exactly_what_it_paints_at_every_gap() {
        // A group boundary costs a bar gap *and* a group gap: ratatui advances
        // by `bar_gap + bar_width` after every bar and only then adds the group
        // gap. Measuring the group gap alone made every multi-group chart with a
        // bar gap — and 1 is the default — claim less space than it paints, so
        // the last bar was dropped by whatever laid the chart out.
        let bar_cells = |buffer: &Buffer| {
            buffer
                .content()
                .iter()
                .filter(|cell| cell.symbol() == "█")
                .count()
        };
        for direction in [Direction::Vertical, Direction::Horizontal] {
            for bar_width in [1u16, 2, 3] {
                for bar_gap in [0u16, 1, 2] {
                    for group_gap in [0u16, 1, 2] {
                        for shape in [
                            vec![1usize],
                            vec![1, 1],
                            vec![2, 2],
                            vec![1, 2, 1],
                            vec![0, 1, 1],
                            vec![1, 0, 1],
                            vec![1, 1, 0],
                        ] {
                            let groups = shape.iter().map(|count| {
                                BarChartGroup::new(
                                    std::iter::repeat_with(|| Bar::default().value(1)).take(*count),
                                )
                            });
                            let chart = BarChartWidget::grouped(groups)
                                .direction(direction)
                                .max_value(1)
                                .show_values(false)
                                .bar_width(bar_width)
                                .bar_gap(bar_gap)
                                .group_gap(group_gap);
                            let measured = if direction == Direction::Vertical {
                                chart.width()
                            } else {
                                chart.height()
                            };
                            let area = if direction == Direction::Vertical {
                                Rect::new(0, 0, measured, 1)
                            } else {
                                Rect::new(0, 0, 1, measured)
                            };
                            let mut buffer = Buffer::empty(area);
                            chart.render(area, &mut buffer);

                            let label = format!(
                                "{direction:?}, bar_width {bar_width}, bar_gap {bar_gap}, group_gap {group_gap}, groups {shape:?}"
                            );
                            let bars: usize = shape.iter().sum();
                            assert_eq!(
                                bar_cells(&buffer),
                                bars * usize::from(bar_width),
                                "every bar must paint whole inside the measured span: {label}"
                            );
                            let last_cell = if direction == Direction::Vertical {
                                (measured - 1, 0)
                            } else {
                                (0, measured - 1)
                            };
                            assert_eq!(
                                buffer.cell(last_cell).expect("last cell").symbol(),
                                "█",
                                "the measured span must end where the last bar does: {label}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_documented_grouped_example_measures_what_it_paints() {
        // The chart the grouped docs show: two labelled groups of two bars at
        // the default `bar_width` of 3 and `bar_gap` of 1, with `group_gap(2)`.
        let chart = || {
            let group = |label: &'static str| {
                BarChartGroup::new(vec![Bar::default().value(1), Bar::default().value(1)])
                    .label(label)
            };
            BarChartWidget::grouped(vec![group("Q1"), group("Q2")])
                .max_value(1)
                .show_values(false)
                .group_gap(2)
        };

        // Four 3-cell bars, a 1-cell bar gap between each adjacent pair, and the
        // 2-cell group gap on top of the boundary's bar gap: 12 + 3 + 2.
        assert_eq!(chart().width(), 17);

        let area = Rect::new(0, 0, 17, 2);
        let mut buffer = Buffer::empty(area);
        chart().render(area, &mut buffer);
        let bar_row: String = buffer
            .content()
            .iter()
            .take(17)
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert_eq!(bar_row, "███ ███   ███ ███");
    }

    #[test]
    fn show_values_false_hides_grouped_bar_values() {
        // `show_values` must mean the same thing in every mode: hiding values
        // on a grouped chart strips the value text from every grouped bar,
        // exactly as it does for standalone bars.
        let area = Rect::new(0, 0, 1, 2);
        let mut with_values = Buffer::empty(area);
        let mut without_values = Buffer::empty(area);
        let group = || BarChartGroup::new(vec![Bar::default().value(9)]);

        BarChartWidget::grouped(vec![group()])
            .max_value(9)
            .bar_width(1)
            .bar_gap(0)
            .render(area, &mut with_values);
        BarChartWidget::grouped(vec![group()])
            .max_value(9)
            .bar_width(1)
            .bar_gap(0)
            .show_values(false)
            .render(area, &mut without_values);

        assert_eq!(with_values.cell((0, 1)).expect("value cell").symbol(), "9");
        assert_eq!(without_values.cell((0, 1)).expect("bar cell").symbol(), "█");
    }
}
