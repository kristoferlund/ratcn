---
description: "How a frame is declared, how ratcn keeps it as the retained surface, and how events reach the right component and bubble back to your app as messages."
---

# Rendering and event routing

Your app drives ratcn through two calls:

- `Ratcn::render(frame, area, state, theme, declare)` declares and paints one frame.
- `Ratcn::handle_event(event, state)` routes one event through the last
  successful declaration.

The declaration is ordinary Rust code that runs every frame. Build components
from current state, split areas with Ratatui, queue decorative widgets with
`DeclareCtx::paint_widget`, and declare interactive components with
`DeclareCtx::component`:

```rust
let area = frame.area();
ratcn.render(frame, area, &state, &state.theme, |ctx| {
    ctx.paint_widget(Paragraph::new("Account"), title_area);
    ctx.component(
        "save",
        Button::new("Save")
            .disabled(state.saving)
            .on_press(|| Msg::Save),
        save_area,
    );
});

if let EventResult::Emit(msg) = ratcn.handle_event(event, &state) {
    update(&mut state, msg);
}
```

Everything inside the closure works through `&mut DeclareCtx`, the same context
type used by nested scopes, dialog sections and a component's own `declare`.
`ctx.area()` is the area the current callback is responsible for, and
`ctx.state()` is the app state for this declaration pass. Components you declare
with `component` get an identity and can receive events; widgets you paint are
decoration and cannot.

Declaring is also how things appear and disappear: an `if` around a `component`
call adds or removes that component for the frame. There is no separate
mount/unmount step.

Pass a pane's rectangle as `area` to host a tree there, or `frame.area()` to use
the whole frame. Floating components place themselves within that area, and
layer paint and modal dimming are clipped to it; `ctx.frame_area()` reports it,
translated into logical coordinates inside a viewport. The area is not a paint
sandbox, though: a base widget can still paint outside its rect. Events arrive
in screen coordinates, and routing input between several hosted trees is up to
your app.

## Declaring comes before painting

Declaring does not paint. `ctx.paint` queues a `'static` closure at the point
the declaration reaches it. Once the tree is complete and focus has resolved,
the runtime replays the queue in that order, so paint order is still
declaration order. Each closure gets a `PaintCtx` carrying the theme, the
state, the area and the interaction flags, so it never borrows from the
declaration it came from.

`ctx.paint_widget(widget, area)` covers the common case of one independent
write: same queue position, no closure to write. Reach for the closure when
several writes share captured data, when one reads the interaction flags, or
when they belong together as a single op.

Because focus resolves only after the tree is complete, a declaration cannot
ask whether it is focused or contains focus; `PaintCtx` answers that. Hover is
the exception: `DeclareCtx::pointer_within()` answers it while declaring.

The closure is `FnMut` and usually runs once per frame. It runs a second time
when focus lands on content a viewport has scrolled out of view: the viewport
scrolls to reveal it, the first declaration is discarded whole, and the second,
built with the revealed offset, is painted. So build what the declaration
consumes inside the closure, and keep side effects out of it.

## Offscreen rendering

Use `Ratcn::render_into(buffer, area, state, theme, declare)` when the destination
is your own `ratatui::buffer::Buffer`, for example a page taller than its visible
window. Ordinary terminal apps should keep using `render`, which runs the same
lifecycle through the frame's buffer.

```rust
use ratatui::{buffer::Buffer, layout::Rect};

let area = Rect::new(0, 0, 80, 200);
let mut page = Buffer::empty(area);
ratcn.render_into(&mut page, area, &state, &state.theme, |ctx| {
    // Declare the full page, including content below the visible window.
});
```

Allocate and resize the buffer yourself. Rendering does not clear it, so clear a
reused buffer first when old content should disappear. Choose an `area` within
`buffer.area`: layout receives it unchanged, without validation or clamping.
Coordinates are absolute within the buffer, including its origin; they do not
restart at `(0, 0)` for a sub-area.

Copying a visible window to the terminal, and translating pointer positions back
into buffer coordinates before `handle_event`, are your app's job. See
[`Ratcn::render_into`](https://docs.rs/ratcn/latest/ratcn/runtime/struct.Ratcn.html#method.render_into)
for the full contract.

## What an event sees

A successful render does two things: it paints, and it retains what was
declared (the component instances, their identities, their areas, and the
props they were built with). That is the **retained surface**, and
`handle_event` routes events through it until the next successful render
replaces it.

This matters because events can arrive *between* an update and the next
redraw: under key repeat, during a paste, or in a browser backend where input
and animation frames are not one-to-one. When that happens, the event is handled by the
retained component from the last frame, and two rules apply:

- **What the user saw wins for intent.** Declared props such as a button's
  label and disabledness stay as they were painted: a button that was enabled
  on screen when clicked should press, even if state disabled it a moment ago.
- **Edits read fresh state.** Controlled values, such as the text being edited
  or the focused row, are read from current app state at event time, so consecutive
  edits compose. Type `a` then `b` before a redraw and the second keystroke
  starts from `"a"`, not from the empty value the last frame painted.

You mostly don't have to think about this, because the built-in components put
each value on the correct side. It becomes relevant when you
[write your own component](./custom-components), which walks through which data
belongs where.

### When a declaration is wrong

Declaration mistakes panic: duplicate sibling ids in one scope, a duplicate
modal root id, a modal root that a bound `ModalState` does not name, an
`interaction_area` reaching outside its paint area. Each is a bug in the
declaration, and the panic points at the call that made it.

Declaration, validation, and the modal check all finish before the first cell
is written, so a pass that fails one of them paints nothing. Replacing the
retained surface is the last step of a successful render, so any panic during a
render, in declaration or in paint, leaves the previous surface in place. The
last good frame stays on screen, events keep routing through it, and nothing
ever routes through a half-declared frame. Before the first successful render, all
events are `Ignored`. A host that wants to keep running through a declaration
bug can catch the unwind around its draw call and carry on calling
`handle_event`.

## Routing

Keyboard and paste events go to the focused component first. Mouse events go to
the component under the pointer, using the geometry from the last successful
render; when targets overlap, the one declared later wins. Either way, the
event lands on a leaf component first, and anything the leaf does not handle
bubbles up through its ancestors.

Every component answers with one of:

| Result | Routing effect |
| --- | --- |
| `EventResult::Emit(msg)` | Stop and return one app message. |
| `EventResult::Consumed` | Stop without a message. |
| `EventResult::Ignored` | Keep bubbling; if nothing handles it, return `Ignored` to the app. |

Only `Ignored` bubbles, and `handle_event` returns at most one message per
event. While a modal is open, input that nothing in the modal handled is
absorbed rather than reaching the UI underneath (see
[Layers and modals](./layers-and-modals)).

Mouse buttons arrive as raw `Down`/`Up`/`Moved` events and the runtime
synthesizes `Click` and `Drag` from them before routing; see
[Mouse input](./mouse).

## App shortcuts

Two patterns cover app-level keys:

- A shortcut that must always work, no matter what is focused (suspend, say):
  check the event *before* calling `handle_event`. Once the app has text
  fields, `Ctrl+C` is the exception: a field copies its selection on it, so
  route it first and quit only if nothing was copied (see
  [The clipboard](./host-integration#the-clipboard)).
- A shortcut that should only fire when no component wanted the event: call
  `handle_event` first and act only on `EventResult::Ignored`.

## Tab order follows declaration order

Sibling declaration order is forward Tab order, and nested scopes keep that
tree order. It is independent of screen position: moving a component visually
does not change traversal unless you also reorder the declarations.

A component participates in focus and hit-testing within the area it was
declared at. A component can shrink its interactive area below its paint area
(to crop blank allocation), and a component or scope declared with zero area
keeps its identity but stays out of traversal and hit-testing for that frame.

Stable identity and focus behavior are covered in
[Focus, hover, and identity](./focus-hover-identity). Paint ordering and modal
layers are covered in [Layers and modals](./layers-and-modals).
