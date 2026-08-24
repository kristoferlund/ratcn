---
description: "A one-of-many control for Ratatui apps that cycles in place: it shows the current option, and every act advances to the next. Built for settings rows."
---

# Cycle

A control that cycles through its options in place: the current value is all
it shows, and every click, <kbd>Enter</kbd>, <kbd>Space</kbd>,
<kbd>Right</kbd>/<kbd>l</kbd>, or <kbd>Ctrl+N</kbd> advances to the next —
wrapping at the end. <kbd>Left</kbd>/<kbd>h</kbd> and <kbd>Ctrl+P</kbd> walk
backward.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 300px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p cycle</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/cycle-demo/index.html" title="ratcn cycle demo"></iframe>
  </div>
</div>

```rust
use ratcn::Cycle;

ctx.component(
    "size",
    Cycle::new(["Small", "Medium", "Large"])
        .selection(|state| state.size, Msg::SetSize),
    area,
);
```

The row paints like a small ghost button: plain text at rest, a quiet fill
while hovered or focused. A column of cycles reads as values, not as a wall of
chrome — which is what makes the settings-row layout work: the setting's name
on the left, the Cycle right-aligned on the same row.

## Where a Checkbox ends

Two options are a [Checkbox](./checkbox) wearing its states as labels
(`[ON]`/`[off]`). Three or more options, or an ordered scale such as
Small/Medium/Large, are a Cycle.

## State

The selection is app-owned and arrives through `.selection(read, on_change)`:
`read` returns the index shown each frame (an out-of-range answer clamps to
the last option), and `on_change` receives the index the user moved to.
Without the binding the Cycle paints but is not focusable and answers no
events.

## Paint-only widget

`CycleWidget` draws the current option across `area`, with no focus, events,
or state:

```rust
use ratcn::CycleWidget;

const SIZES: [&str; 3] = ["Small", "Medium", "Large"];

frame.render_widget(CycleWidget::new(SIZES[state.size]).themed(&theme), area);
```

The interactive component paints and answers events in exactly the columns its
current value occupies — a Cycle is as wide as the text it shows, so rows of
different settings end at different columns, and a fill on hover or focus
hugs the value instead of stretching across the row. Replace `.themed(...)`
with `.style(...)` to supply exact colors.

## Full API

Every method, with binding requirements and edge-case detail:
[`Cycle`](https://docs.rs/ratcn/latest/ratcn/struct.Cycle.html),
[`CycleWidget`](https://docs.rs/ratcn/latest/ratcn/struct.CycleWidget.html),
[`CycleStyle`](https://docs.rs/ratcn/latest/ratcn/struct.CycleStyle.html).

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## See also

Use [Checkbox](./checkbox) for two-state settings, or [Select](./select) when
the whole option list should open for browsing instead of cycling in place.
