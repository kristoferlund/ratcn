---
description: "BarChartWidget is a themed adapter over Ratatui's BarChart, adding theme colors, bar grouping, and a value-display switch. Paint-only, so no runtime is needed."
---

# BarChartWidget

A themed adapter over Ratatui's `BarChart`, adding grouping and a value display
switch. The bars, labels, and painting are Ratatui's; ratcn supplies the theme
colors on top. It is paint-only and an ordinary Ratatui widget — no `Ratcn`
runtime needed, just `frame.render_widget(...)`.

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

Bars run upward by default; `BarChartWidget::vertical(...)` is the same
constructor under a clearer name. `.width()` measures the vertical chart's bar
grouping axis; horizontal charts expose that measurement through `.height()`.
The other axis remains area-driven because it contains the scaled bar length.

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
abbreviations — usually the reason to choose this direction.

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
grouped bars too. Set `.direction(Direction::Horizontal)` for horizontal groups
— that is `ratatui::layout::Direction`, the layout axis. The runtime's own
`Forward`/`Backward` enum is `ratcn::runtime::Step`, a different type.
Horizontal group labels occupy the space reserved by `.group_gap(...)` and are
not painted when that gap is `0`. A group with no bars is dropped: it paints
nothing, and it takes neither space nor a group gap in `.width()`/`.height()`.

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
printed inside each bar, for bars too narrow to fit one.

A vertical bar rarely ends exactly on a cell boundary, so its top cell is
painted with a partial block. `.bar_set(...)` chooses those glyphs — the
default gives the smoothest result, and coarser sets exist for terminals whose
fonts lack them. Horizontal bars use whole cells and only use the set's `full` and `empty`
symbols.

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
is no theme. `BarChartStyle::label_foreground` colors vertical bar labels and group labels.
Ratatui does not apply its chart-level label style to ordinary horizontal bar
labels, so set those labels' `Line` or `Span` foreground directly:

```rust
use ratcn::BarChartStyle;

let mut style = BarChartStyle::from_theme(&theme);
style.bar = theme.accent;

BarChartWidget::new(bars).style(style)
```

### Per-bar colors

Bars reach Ratatui untouched, so Ratatui's own `Bar::style` works and patches
over the chart-wide bar color — one bar, or one series in a grouped chart:

```rust
Bar::default().value(18).style(Style::default().fg(Color::Red))
```

The value printed inside a bar is not covered: it keeps the chart's
`value_foreground` on the chart's `bar` background. Set `Bar::value_style` on
that bar to match, or hide values with `.show_values(false)`.

## Full API

Every method, with parameter and edge-case detail:
[`BarChartWidget`](https://docs.rs/ratcn/latest/ratcn/struct.BarChartWidget.html),
[`BarChartGroup`](https://docs.rs/ratcn/latest/ratcn/struct.BarChartGroup.html),
[`BarChartStyle`](https://docs.rs/ratcn/latest/ratcn/struct.BarChartStyle.html).

## See also

The color roles every widget derives its default palette from:
[Themes](../concepts/themes).
