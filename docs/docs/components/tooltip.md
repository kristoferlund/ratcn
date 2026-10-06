---
description: "A tooltip for Ratatui apps: a short explanation floated beside the control it describes, painted in an inert hint layer that never takes a click or focus."
---

# Tooltip

A short explanation floated beside the content it describes. The bubble is
declared in a hint layer, painted above the content around it (but beneath a
modal it sits outside of). It is inert: a click over it reaches the control
underneath, and it never takes focus.

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
keeps its own looks, focus order, and clicks. The Tooltip adds an explanation
and nothing else.

## State

There is none to keep. A Tooltip shows while the pointer is inside it and hides
when the pointer leaves, and the runtime owns hover, so the example above
stores nothing and routes nothing.

`.open_when(read)` replaces that rule when it is not quite what you want. The
reader gets your state and the hover answer the default uses:

```rust
// The default, spelled out.
.open_when(|_: &AppState, hovered| hovered)

// Gated: a disabled control explains nothing.
.open_when(|s: &AppState, hovered| hovered && !s.controls_disabled)

// Widened: keyboard focus shows it too, which the component cannot see itself.
.open_when(move |s: &AppState, hovered| hovered || s.focus.contains_path([id]))
```

Pass the Tooltip's own id to the focus query: its trigger's children sit
beneath it in the path.

A click focuses what it hits, so a focus-based reader keeps the bubble showing
after a press until focus moves elsewhere. For the web's `:focus-visible`
behavior instead, note which device is driving. The app sees every event, so
recording `state.keyboard = !matches!(event, Event::Mouse(_))` before routing
is enough:

```rust
.open_when(move |s: &AppState, hovered| {
    hovered || (s.keyboard && s.focus.contains_path([id]))
})
```

Use `.open(read, on_open_change)` instead when the app keeps a flag of its own
that the Tooltip should change, such as a first-run hint or a validation
failure. That form bundles the same reader with a message: the component asks
for `true` when the pointer moves onto the trigger, and `false` on Esc while
showing. Neither `.open_when(...)` nor the default emits anything, since there
is nothing to write.

## Placement

`.side(...)` picks the preferred side: `TooltipSide::Top` (the default),
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

`TooltipStyle` has three colors (`foreground`, `background`, and `border`)
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

`TooltipWidget` paints the bubble on its own. It is an ordinary Ratatui widget,
so it works in a plain Ratatui app with no `Ratcn` runtime. Take the look and
keep your own hover handling:

```rust
use ratcn::TooltipWidget;

let bubble = TooltipWidget::new("Write the ledger to disk").themed(&theme);
let width = bubble.width().min(40);
frame.render_widget(bubble, Rect::new(x, y, width, bubble.height(width)));
```

`.width()` reports the width the text needs unwrapped, and `.height(width)` the
rows it needs once wrapped to that width. Both include the border, so a layout
reserves exactly what paints. Replace `.themed(...)` with `.style(...)`
to supply exact colors.

## Keyboard and mouse

| Input | Does |
|---|---|
| Pointer onto the trigger | Show the bubble |
| Pointer off the trigger | Hide the bubble |
| `Esc`, with focus inside the trigger | Ask to close it (the `.open(...)` form only) |

Showing and hiding on hover is the default rule; a custom reader decides for
itself.

A hover-driven tooltip ignores Esc: there is no stored flag to clear, and the
pointer still says the bubble belongs on screen. Nothing else is captured.
Keys bubble through to the app, and a press over the bubble goes to whatever it
covers. A Tooltip is never a Tab stop, and neither is its bubble, so focus
passes straight through to the trigger.

A hover change that does not come from the pointer, such as a modal opening
over a showing tooltip, can reach the bubble one frame late. See
[Focus, hover, and identity](../concepts/focus-hover-identity#where-paint-and-structure-disagree).

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## Full API

See
[`Tooltip`](https://docs.rs/ratcn/latest/ratcn/struct.Tooltip.html),
[`TooltipWidget`](https://docs.rs/ratcn/latest/ratcn/struct.TooltipWidget.html),
[`TooltipStyle`](https://docs.rs/ratcn/latest/ratcn/struct.TooltipStyle.html),
and [`TooltipSide`](https://docs.rs/ratcn/latest/ratcn/enum.TooltipSide.html)
on docs.rs for the full API.

## See also

Use [Toast](./toast) for a message that announces something happened rather than
explaining what is under the pointer, and [Dialog](./dialog) when the content
needs input of its own.
