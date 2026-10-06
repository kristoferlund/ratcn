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
step through them with Tab and Shift+Tab. Focus landing on a button the viewport
is clipping scrolls that button into view. Page Up and Page Down scroll the
viewport without moving focus.

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
value, to persist it or to scroll from elsewhere. The message carries the
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

The reader runs for every event, so repeated wheel or page events compose
without a redraw between them, as long as the app applies each message as it
arrives.

## Focus

When focus moves to a descendant the viewport is clipping, the area scrolls it
into view on the same frame. That holds however focus got there: Tab, a click,
or a focus path your update function stores. It also holds for a descendant
declared for the first time on that frame, such as a row appended and focused
together. Focus itself travels through `Ratcn::focus(read, on_change)` as it
does everywhere else; the area only adds the reveal.

A reveal moves the view without emitting the `.scroll(...)` message, so a
bound offset can differ from the one the area paints with, and there is no way
to read the effective offset back. The next offset the area emits starts from
where the reveal left it. Ordinary descendants need nothing extra, but if you
paint related content outside the area, keep its windowing in step yourself.

## Layout and clipping

One column on the right is reserved for the scrollbar. The content callback
receives the remaining width and exactly the configured logical height, so
ordinary Ratatui layout and fixed-height allocations work inside it.

Everything paints against the full logical area, and the result is translated
and clipped to the viewport. Offscreen descendants stay declared and
focusable. Visible rows paint, hover, and take pointer events as usual, and a
captured drag keeps routing to its owner after it leaves the viewport. Mouse
events and `DragPhase` positions arrive in content coordinates, matching
`EventCtx::area`.

## Layers

Hints, popups, and modals keep their normal layer behavior. Each opens at the
place on screen its area names and declares in screen coordinates from there,
so a scroll area inside a dialog, or a popup inside a scroll area, is ordinary
nesting. A popup or hint follows its anchor: once scrolling carries the anchor
off screen, the layer is skipped, and it comes back with the anchor.

## Hover focus

Pointer motion leaves focus alone. For a pane or tile grid whose direct
children should take focus as the pointer crosses them, opt in with
`.hover_focus()`.

## Styling

The scrollbar uses Ratatui's `Scrollbar`. Its thumb comes from the theme's
primary color and its track from the theme's border color. Override both with
`.style(...)`:

```rust
use ratcn::{ScrollArea, ScrollAreaStyle};

let scroll = ScrollArea::new(100).style(|theme| ScrollAreaStyle {
    thumb: theme.accent,
    track: theme.muted_foreground,
});
```

## Keyboard and mouse

| Input | Does |
|---|---|
| Wheel | Scroll three rows |
| `Page Up` `Page Down` | Scroll by the visible height |
| `Home` `End` | Jump to the top / bottom |
| Drag the scrollbar thumb | Scroll, keeping the grabbed point under the pointer |
| Press the track | Jump the view to that row |

Descendants receive each event first, so a focused list can take Page Down and
a nested control can take the wheel. An event that leaves the offset where it
is, such as any of these keys at an edge or a horizontal wheel, bubbles on to
the app, so app hotkeys on those keys keep working. An area with no focusable
descendant is a focus stop itself, so keyboard scrolling works for paint-only
content too.

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## Limits

A `ScrollArea` inside another `ScrollArea` panics. So does content above
262,144 cells, and so does a single paint inside one covering more than that.
For larger data sets, window the rows yourself and give the area the height of
the window.

## Full API

See [`ScrollArea`](https://docs.rs/ratcn/latest/ratcn/struct.ScrollArea.html)
and [`ScrollAreaStyle`](https://docs.rs/ratcn/latest/ratcn/struct.ScrollAreaStyle.html).

## See also

- [List](./list): a scrollable list of items, with its own cursor and offset.
