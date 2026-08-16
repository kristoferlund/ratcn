---
description: "How a frame is declared, how ratcn retains it as the interaction surface, and how events reach the right component and bubble back to your app as messages."
---

# Rendering and event routing

Ratcn enters an app at two calls:

- `Ratcn::render(frame, state, theme, declare)` paints and declares one frame.
- `Ratcn::handle_event(event, state)` routes one event through the last
  successful declaration.

The declaration is immediate Rust code. Build components from current state,
split areas with Ratatui, queue decorative widgets with `RenderCtx::paint`, and
register interactive components with `RenderCtx::render_component`:

```rust
ratcn.render(frame, &state, &state.theme, |ctx| {
    ctx.paint(move |ctx| ctx.render_widget(Paragraph::new("Account"), title_area));
    ctx.render_component(
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

Everything inside the closure works through `&mut RenderCtx` — the same context
type used by nested scopes, dialog sections, and component render hooks.
`ctx.area()` is the area the current callback is responsible for, and
`ctx.state()` is the app state for this declaration pass. Components you declare
with `render_component` get an identity and can receive events; widgets you
paint are decoration and cannot.

Declaring does not draw. `ctx.paint` queues a `'static` closure at the point it
was reached, and the runtime replays the whole queue in that order once the
tree is complete and focus has resolved — so paint order is still declaration
order, and the closure gets a `PaintCtx` carrying the theme, the state, the
area, and the interaction flags rather than borrowing the declaration it came
from.

Declaring is also how things appear and disappear: an `if` around a
`render_component` call adds or removes that component for the frame. There is
no separate mount/unmount step.

## What an event sees

A successful render does two things: it paints, and it retains what was declared — the component instances, their identities, their areas, and
the props they were built with. That is the **retained surface**,
and `handle_event` routes events through it until the next successful render
replaces it.

This matters because events can arrive *between* an update and the next redraw
— under key repeat, paste, or a browser backend where input and animation
frames are not one-to-one. When that happens, the event is handled by the
retained component from the last frame, and two rules apply:

- **What the user saw wins for intent.** Declared props such as a button's
  label and disabledness stay as they were painted: a button that was enabled
  on screen when clicked should press, even if state disabled it a moment ago.
- **Edits read fresh state.** Controlled values — the text being edited, the
  focused row — are read from current app state at event time, so consecutive
  edits compose. Type `a` then `b` before a redraw and the second keystroke
  starts from `"a"`, not from the empty value the last frame painted.

You mostly don't have to think about this — the built-in components put each
value on the correct side. It becomes relevant when you
[write your own component](./custom-components), which walks through which data
belongs where.

If anything in the declaration panics, Ratcn keeps the previous interaction
surface. Pixels already painted stay on screen, but events never route through
a half-declared frame. Before the first successful render, all events are
`Ignored`. Why declaration mistakes panic instead of returning errors is
covered in [Design decisions](./design-decisions).

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
absorbed rather than reaching the UI underneath — see
[Layers and modals](./layers-and-modals).

Mouse buttons arrive as raw `Down`/`Up`/`Moved` events and the runtime
synthesizes `Click` and `Drag` from them before routing; see
[Mouse Input](./mouse).

## App shortcuts

Two patterns cover app-level keys:

- A shortcut that must always work, no matter what is focused (quit, suspend):
  check the event *before* calling `handle_event`.
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
