---
description: "A one-of-many control for Ratatui apps that cycles in place: it shows the current option, and every act advances to the next. Built for settings rows."
---

# Cycle

A one-of-many control that cycles through its options in place. It shows only
the current option, and each press advances to the next, wrapping at the end.
It suits settings rows, where a column of values should read as values rather
than as a wall of chrome.

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

The selection is app-owned and arrives through `.selection(read, on_change)`:
`read` returns the index shown each frame, and `on_change` receives the index
the user moved to. Without the binding the Cycle paints but is not focusable
and answers no events.

## Settings rows

The value paints like a small ghost button: plain text at rest, a quiet fill
while hovered or focused. A Cycle is exactly as wide as the text it shows, in
both paint and hit target, so the fill hugs the value instead of stretching
across the row.

For a settings row, paint the setting's name at the left edge and declare the
Cycle on the same row with `.align(Alignment::Right)`. The value hugs the right
edge, and nothing needs measuring:

```rust
ctx.paint_widget(Line::from("Text size").style(name), row);
ctx.component(
    "size",
    Cycle::new(["Small", "Medium", "Large"])
        .selection(|state| state.size, Msg::SetSize)
        .align(Alignment::Right),
    row,
);
```

For layouts that reserve space instead, `Cycle::width()` (and
`MeasuredComponent`) return the widest option plus its padding: the columns no
value ever outgrows.

## Where a Checkbox ends

Two options are a [Checkbox](./checkbox) wearing its states as labels
(`[ON]`/`[off]`). Three or more options, or an ordered scale such as
Small/Medium/Large, are a Cycle.

## Disabled

`.disabled(true)` mutes the value, takes it out of Tab order, and ignores
events. Pass the flag from app state, which is in scope while declaring.

## Styling

Colors derive from the theme. To recolor one Cycle, pass `.style(...)` a
closure that receives the active theme and returns a `CycleStyle`, usually
built from `CycleStyle::from_theme(theme)`.

## Paint-only widget

`CycleWidget` draws the current option across `area`, with no focus, events,
or state:

```rust
use ratcn::CycleWidget;

const SIZES: [&str; 3] = ["Small", "Medium", "Large"];

frame.render_widget(CycleWidget::new(SIZES[state.size]).themed(&theme), area);
```

You supply the interaction states with `.focused(...)`, `.hovered(...)`, and
`.disabled(...)`. Replace `.themed(...)` with `.style(...)` to supply exact
colors.

## Keyboard and mouse

| Input | Does |
|---|---|
| `Enter` `Space` &nbsp;`→` `l` &nbsp;`Ctrl+N` | Next option, wrapping to the first |
| `←` `h` &nbsp;`Ctrl+P` | Previous option, wrapping to the last |
| Left click | Next option |

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## Limits

- `Home` and `End` do nothing: the options form a ring with no ends.
- A `read` index past the last option shows the last option.

## Full API

See
[`Cycle`](https://docs.rs/ratcn/latest/ratcn/struct.Cycle.html),
[`CycleWidget`](https://docs.rs/ratcn/latest/ratcn/struct.CycleWidget.html),
and [`CycleStyle`](https://docs.rs/ratcn/latest/ratcn/struct.CycleStyle.html)
on docs.rs for the full API.

## See also

Use [Checkbox](./checkbox) for two-state settings, or [Select](./select) when
the whole option list should open for browsing instead of cycling in place.
