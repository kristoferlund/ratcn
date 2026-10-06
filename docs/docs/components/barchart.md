---
description: "BarChartWidget is a themed bar chart for Ratatui apps, built on Ratatui's BarChart with theme colors, bar grouping, and a value-display switch. Paint-only, so no runtime is needed."
---

# BarChartWidget

A themed bar chart. The bars, labels, and painting come from Ratatui's
`BarChart`; ratcn adds theme colors, bar grouping, and a switch for the values
printed in the bars. It only paints, so it is an ordinary Ratatui widget that
needs no `Ratcn` runtime, just `frame.render_widget(...)`.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 320px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p barchart</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/barchart-demo/index.html" title="ratcn bar chart demo"></iframe>
  </div>
</div>

```rust
use ratatui::widgets::Bar;
use ratcn::BarChartWidget;

let bars = vec![
    Bar::default().label("Mon").value(12),
    Bar::default().label("Tue").value(18),
    Bar::default().label("Wed").value(9),
];

frame.render_widget(BarChartWidget::new(bars).themed(&theme), area);
```

Bars run upward by default, and the chart fills the area it is given.

## Scale

By default the tallest bar fills the chart, so the scale moves whenever the data
does. Pin it with `.max_value(...)` for a chart that updates live or that should
be comparable with another chart.

```rust
BarChartWidget::new(bars).themed(&theme).max_value(24)
```

## Horizontal

`BarChartWidget::horizontal(...)` runs the bars across instead of up. Each bar
gets a whole row to itself, so labels have room to be phrases rather than
abbreviations. That is usually the reason to choose this direction.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 320px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p barchart-horizontal</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/barchart-horizontal-demo/index.html" title="ratcn horizontal bar chart demo"></iframe>
  </div>
</div>

```rust
BarChartWidget::horizontal(bars)
    .themed(&theme)
    .bar_width(1) // a horizontal bar's "width" is its height, in rows
    .bar_gap(0)
```

## Grouped

`BarChartWidget::grouped(...)` clusters bars so several series can be compared
across categories. Groups are `BarChartGroup` values rather than Ratatui's
`BarGroup`, so widget-level options such as `.show_values(false)` apply to
grouped bars too. For horizontal groups, add
`.direction(Direction::Horizontal)` (Ratatui's `ratatui::layout::Direction`).

```rust
use ratcn::BarChartGroup;

BarChartWidget::grouped(vec![
    BarChartGroup::new(q1_bars).label("Q1"),
    BarChartGroup::new(q2_bars).label("Q2"),
])
.themed(&theme)
.group_gap(2)
```

## Bar shape

`.bar_width(...)` and `.bar_gap(...)` size the bars; `.group_gap(...)` adds space
between clusters in a grouped chart, on top of the bar gap that already separates
the two bars either side of the boundary. `.show_values(false)` hides the number
printed inside each bar, for bars too narrow to fit one. To size a layout to the
chart, `.span()` measures the axis the bars sit along: the width of a vertical
chart, the height of a horizontal one.

A vertical bar rarely ends exactly on a cell boundary, so its top cell is
painted with a partial block. `.bar_set(...)` chooses those glyphs. The
default gives the smoothest result, and coarser sets exist for terminals whose
fonts lack the finer blocks. Horizontal bars use whole cells, so only the set's
`full` and `empty` symbols apply.

```rust
use ratatui::symbols;

BarChartWidget::new(bars)
    .themed(&theme)
    .show_values(false)
    .bar_set(symbols::bar::THREE_LEVELS)
```

## Styling

`.themed(&theme)` derives every color from the active theme. Use
`.style(BarChartStyle)` for explicit colors, starting from
`BarChartStyle::from_theme(...)` or from `BarChartStyle::fallback()` when there
is no theme:

```rust
use ratcn::BarChartStyle;

let mut style = BarChartStyle::from_theme(&theme);
style.bar = theme.accent;

BarChartWidget::new(bars).style(style)
```

### Per-bar colors

Bars reach Ratatui untouched, so Ratatui's own `Bar::style` works and patches
over the chart-wide bar color, for one bar or one series in a grouped chart:

```rust
Bar::default().value(18).style(Style::default().fg(Color::Red))
```

The value printed inside a bar is not covered: it keeps the chart's
`value_foreground` on the chart's `bar` background. Set `Bar::value_style` on
that bar to match, or hide values with `.show_values(false)`.

## Limits

- `BarChartStyle::label_foreground` colors vertical bar labels and group
  labels, but Ratatui does not apply a chart-level label style to ordinary
  horizontal bar labels. Set those labels' `Line` or `Span` foreground directly.
- Horizontal group labels sit in the space `.group_gap(...)` reserves, so they
  are not painted when that gap is `0`.
- A group with no bars is dropped: it paints nothing and takes no space.

## Full API

Every method, with parameter and edge-case detail:
[`BarChartWidget`](https://docs.rs/ratcn/latest/ratcn/struct.BarChartWidget.html),
[`BarChartGroup`](https://docs.rs/ratcn/latest/ratcn/struct.BarChartGroup.html),
[`BarChartStyle`](https://docs.rs/ratcn/latest/ratcn/struct.BarChartStyle.html).

## See also

The color roles every widget derives its default palette from:
[Themes](../concepts/themes).
