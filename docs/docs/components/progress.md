---
description: "A slim progress bar for Ratatui apps: a themed take on ratatui's Gauge, with an optional label and percentage above the track, in the composition shadcn/ui made familiar."
---

# Progress

A slim bar showing how far a task has come: the fill's share of the track is
the work done. This is a themed, opinionated take on
[ratatui's `Gauge`](https://docs.rs/ratatui/latest/ratatui/widgets/struct.Gauge.html)
— the gauge keeps drawing, down to the fractional block that lets the fill
move in eighths of a cell — and adds what an application bar wants: theme
colors, and an optional label and percentage above the track.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 300px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p progress</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/progress-demo/index.html" title="ratcn progress demo"></iframe>
  </div>
</div>

```rust
use ratcn::ProgressWidget;

frame.render_widget(ProgressWidget::new(0.33).themed(&theme), area);
```

The ratio runs from `0.0` (empty) to `1.0` (full) and is clamped on the way
in, so a division that briefly misbehaves cannot smear the bar off its track.

## Label and value

`.label(...)` names the task, flush left above the track;
`.show_value(true)` prints the percentage, flush right on that same row —
the composition shadcn/ui reaches by stacking `ProgressLabel` and
`ProgressValue` over `ProgressTrack`:

```rust
use ratcn::ProgressWidget;

frame.render_widget(
    ProgressWidget::new(state.uploaded / state.total)
        .label("Uploading notes.md")
        .show_value(true)
        .themed(&theme),
    area,
);
```

The composition costs one extra row: `.height()` answers `2` when either
option shows and `1` when neither does. Given a single row anyway, the bar
keeps it and the header is dropped; given taller rows than it needs, the bar
stays one row of track and leaves the rest alone. The percentage and the fill
round independently — the gauge rounds its last cell in eighths — so at a
width's rounding edge the number can briefly sit one eighth of a cell away
from the bar it describes.

## Colors

Themed, the four roles derive from the theme: `fill` takes the primary,
`track` the inset well the other control surfaces use, `label` the muted
foreground, and `value` the ordinary foreground. `.style(...)` supplies exact
colors for the same four roles instead.

## Full API

Every method, with edge-case detail:
[`ProgressWidget`](https://docs.rs/ratcn/latest/ratcn/struct.ProgressWidget.html),
[`ProgressStyle`](https://docs.rs/ratcn/latest/ratcn/struct.ProgressStyle.html).

A progress bar paints only: it takes no focus, handles no events, and works
like any other ratatui widget — no runtime required.

## See also

[BarChartWidget](./barchart) compares several values at once; a Progress
follows one value over time. For a step counter that never animates — three
of five files migrated — plain text in a [List](./list) row may say it best.
