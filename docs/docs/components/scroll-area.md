---
description: "A vertical viewport for arbitrary interactive ratcn descendants, with clipped paint and pointer routing, focus reveal, and a themed Ratatui scrollbar."
---

# ScrollArea

`ScrollArea` makes an arbitrary ratcn subtree vertically scrollable without
changing its layout. You give it the full logical content height; descendants
receive their real logical allocations however many of their rows are visible.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 360px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p scroll-area</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/scroll-area-demo/index.html" title="ratcn scroll area demo"></iframe>
  </div>
</div>

Ten buttons stand in a viewport three of them tall. Click one to focus it, or
step through them with Up and Down, which the demo passes to the runtime as Tab
and BackTab. Focus landing on a button the viewport is clipping scrolls that
button into view.

```rust
use ratatui::layout::Rect;
use ratcn::{Button, ScrollArea};

let scroll = ScrollArea::new(state.content_height).content(|ctx| {
    let content = ctx.area();
    ctx.component(
        "save",
        Button::new("Save").on_press(|| Msg::Save),
        Rect::new(content.x, content.y + 20, content.width, 3),
    );
});

ctx.component("content", scroll, Rect::new(0, 0, 40, 12));
```

The area owns its offset. Bind it with `.scroll(...)` when the app needs the
value — to persist it, or to scroll from elsewhere. The message carries the
new first visible content row:

```rust
let scroll = ScrollArea::new(state.content_height)
    .scroll(|state: &AppState| state.scroll_offset, Msg::ScrollAreaChanged);
```

```rust
match msg {
    Msg::ScrollAreaChanged(offset) => state.scroll_offset = offset,
}
```

## Focus

Focus moving to a descendant the viewport is clipping scrolls that descendant
into view on the same frame. Focus itself travels through
`Ratcn::focus(read, on_change)` as it does everywhere else: Tab, BackTab, focus
keys, and pointer focus all produce that message, and the area adds the reveal
on top of it.

## Layout and clipping

One column on the right is reserved for the scrollbar gutter. The content
callback receives the remaining width and exactly the configured logical
height, so ordinary Ratatui layout and real fixed-height allocations work
inside it.

Everything paints against the full logical area; the result is translated and
clipped to the viewport. Offscreen descendants stay declared and focusable, and
paint, hover, and take uncaptured pointer events on the rows that are visible. A
captured pointer gesture keeps routing to its owner after leaving the viewport,
so drag components work as they do elsewhere. Paint outside the logical content
is clipped away.

## Input

The mouse wheel scrolls three rows. Page Up and Page Down scroll by the visible
height; Home and End jump to the bounds. Descendants receive each event first,
so a focused list can take Page Down and a nested control can take the wheel.
An event that leaves the offset where it is — every one of these keys at an
edge, and a horizontal wheel — bubbles on to the app, which keeps app hotkeys on
those keys alive.

A bound offset reader runs for every event, so applying each emitted message
before routing the next one makes repeated wheel or page events compose even
without a redraw between them.

An area holding no focusable descendant is a focus stop itself, so keyboard
scrolling stays available for paint-only content.

Pointer motion leaves focus alone. For a pane or tile grid whose direct
children should take focus as the pointer crosses them, opt in with
`.hover_focus()`.

Mouse events and `DragPhase` positions arrive in content coordinates, matching
`EventCtx::area`. A drag anchor is screen-absolute inside the runtime, so
scrolling under a held pointer leaves the travel it measures alone.

The scrollbar is an indicator of where the view sits.

## Layers

Hints, popups, modals, and `defer_paint` keep their normal layer behavior. They
are translated from logical coordinates once and escape the viewport clip. A
popup or hint whose declaring anchor is offscreen is dropped, so an invisible
trigger leaves no overlay behind.

## Styling

The scrollbar uses Ratatui's `Scrollbar`. Its thumb comes from the theme's
primary color and its track from the theme border color. Override both with
`.style(...)`:

```rust
use ratcn::{ScrollArea, ScrollAreaStyle};

let scroll = ScrollArea::new(100).style(|theme| ScrollAreaStyle {
    thumb: theme.accent,
    track: theme.muted_foreground,
});
```

## Limits

A `ScrollArea` inside another `ScrollArea` panics. Each viewport is backed by a
content-sized Ratatui buffer, and content above 262,144 cells panics; for
larger data sets, window the rows yourself and give the area the height of the
window.

## See also

- [List](./list) — a scrollable list of items, with its own cursor and offset.

## Full API

See [`ScrollArea`](https://docs.rs/ratcn/latest/ratcn/struct.ScrollArea.html)
and [`ScrollAreaStyle`](https://docs.rs/ratcn/latest/ratcn/struct.ScrollAreaStyle.html).
