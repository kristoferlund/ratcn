---
description: "A tooltip for Ratatui apps: a short explanation floated beside the control it describes, painted in an inert hint layer that never takes a click or focus."
---

# Tooltip

A short explanation floated beside the content it describes. The bubble is
declared in a hint layer: painted above everything else, but inert — a click
over it reaches the control underneath, and it never takes focus.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 340px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p tooltip</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/tooltip-demo/index.html" title="ratcn tooltip demo"></iframe>
  </div>
</div>

```rust
use ratcn::{Button, Tooltip};

let tooltip = Tooltip::new("Write the ledger to disk").trigger(|ctx| {
    let area = ctx.area();
    ctx.component("save", Button::new("Save").on_press(|| Msg::Save), area);
});

ctx.component("save_tip", tooltip, area);
```

A Tooltip wraps rather than replaces: the area you declare it with is the
trigger's area, and `.trigger(...)` declares whatever goes there. That content
keeps its own looks, focus order, and clicks — the Tooltip adds an explanation
and nothing else.

## State

There is none to keep. A Tooltip shows while the pointer is inside it and hides
when the pointer leaves, and the runtime owns hover, so the example above
stores nothing and routes nothing.

`.open_when(read)` replaces that rule when it is not quite what you want. The
reader gets your state and the same hover answer the default uses:

```rust
// The default, spelled out.
.open_when(|_: &AppState, hovered| hovered)

// Gated: a disabled control explains nothing.
.open_when(|s: &AppState, hovered| hovered && !s.controls_disabled)

// Widened: keyboard focus shows it too, which the component cannot see itself.
.open_when(move |s: &AppState, hovered| hovered || s.focus.contains_path([id]))
```

The focus query is a root-anchored prefix, so the id you pass is the Tooltip's
own — its trigger's children sit beneath it in the path.

Mind what the focus half does on its own: a click focuses what it hits, so that
reader keeps the bubble showing after a press until focus moves elsewhere. Pair
focus with your own note of which device is driving if you want the web's
`:focus-visible` behavior instead — the app sees every event, so recording
`state.keyboard = !matches!(event, Event::Mouse(_))` before routing is enough:

```rust
.open_when(move |s: &AppState, hovered| {
    hovered || (s.keyboard && s.focus.contains_path([id]))
})
```

Use `.open(read, on_open_change)` instead when the app keeps a flag of its own
that the Tooltip should change — a first-run hint, a validation failure. That
form bundles the same reader with its message, and the component asks for
`true` when the pointer moves onto the trigger and `false` on Esc while
showing. Neither `.open_when(...)` nor the default emits anything, since there
is nothing to write.

## Interaction

Moving the pointer onto the trigger shows the bubble, and moving it off hides
it again. For a hover-driven tooltip — the two above, and the one in the demo —
Esc does nothing: there is no stored flag to clear, and the pointer still says
the bubble belongs on screen.

Esc belongs to the `.open(read, on_open_change)` form. While such a tooltip is
open and something inside its trigger has focus, an unmodified Esc bubbles out
to the Tooltip and asks the app to close it, so a keyboard user can dismiss an
explanation without reaching for the mouse. Nothing else is captured either
way: keys bubble through the bubble to the app, and a press over the bubble
goes to whatever it covers.

A Tooltip is never a Tab stop, and neither is its bubble — focus passes
straight through to the trigger.

Structure is decided while declaring, so the bubble follows a hover change that
did not come from the pointer one frame late: open a modal over a showing
tooltip and the bubble is declared once more, on the frame the modal appears,
before the next frame drops it. See
[Focus, hover, and identity](../concepts/focus-hover-identity#where-paint-and-structure-disagree).

## Placement

`.side(...)` picks the preferred side — `TooltipSide::Top` (the default),
`Bottom`, `Left`, or `Right`. The bubble is centered on the trigger's other
axis, flips to `TooltipSide::opposite()` when the preferred side has no room in
the frame, and is finally clamped inside the frame so it is always fully
visible.

```rust
Tooltip::new("Rebuilds the index").side(TooltipSide::Right)
```

Width is the text's natural width, capped by `.max_width(...)` (40 cells by
default, or `Tooltip::DEFAULT_MAX_WIDTH`) and by the terminal. Longer text wraps
and the bubble grows taller.

## Styling

`TooltipStyle` has three colors — `foreground`, `background`, and `border` —
and no interaction states, since a tooltip is never focused, hovered, or
disabled. `.style(...)` overrides them for one tooltip; the closure receives the
active theme each render, so a derived style follows theme switches:

```rust
use ratcn::TooltipStyle;

Tooltip::new("Destructive").style(|theme| {
    let mut style = TooltipStyle::from_theme(theme);
    style.border = theme.destructive;
    style
})
```

`TooltipStyle::fallback()` is the no-theme starting point: plain ANSI colors
that render on any terminal.

## Paint-only widget

`TooltipWidget` draws the bubble on its own. It is an ordinary Ratatui widget,
so it works in a plain Ratatui app with no `Ratcn` runtime — take the look and
keep your own hover handling:

```rust
use ratcn::TooltipWidget;

let bubble = TooltipWidget::new("Write the ledger to disk").themed(&theme);
let width = bubble.width().min(40);
frame.render_widget(bubble, Rect::new(x, y, width, bubble.height(width)));
```

`.width()` reports the width the text needs unwrapped, and `.height(width)` the
rows it needs once wrapped to that width — both including the border, so a
layout reserves exactly what paints. Replace `.themed(...)` with `.style(...)`
to supply exact colors.

## Full API

Every method, with binding requirements and edge-case detail:
[`Tooltip`](https://docs.rs/ratcn/latest/ratcn/struct.Tooltip.html),
[`TooltipWidget`](https://docs.rs/ratcn/latest/ratcn/struct.TooltipWidget.html),
[`TooltipStyle`](https://docs.rs/ratcn/latest/ratcn/struct.TooltipStyle.html),
[`TooltipSide`](https://docs.rs/ratcn/latest/ratcn/enum.TooltipSide.html).

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## See also

Use [Toast](./toast) for a message that announces something happened rather than
explaining what is under the pointer, and [Dialog](./dialog) when the content
needs input of its own.
