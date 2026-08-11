---
description: "Writing your own ratcn component: the Component trait, the four kinds of data a component carries and when each is read, and how composites declare children."
---

# Custom components

Ratcn's built-ins are not special. `RenderCtx::render_component` accepts any
implementation of the `Component` trait, and the runtime gives a custom
component the same identity, focus, hover, hit-testing, and event routing it
gives `Button`. The kanban demo's `KanbanCard` — a draggable card with
board-aware drop handling — is a complete custom component in about a hundred
lines.

Write a component when the demoed behavior needs real event handling or its own
interaction identity. Purely decorative content should stay direct paint:
build a Ratatui widget and render it with `ctx.render_widget`. Paint-only content
declared as a component costs identity, traversal, and hit-testing for nothing.

## The trait

```rust
impl Component<AppState, Msg> for MyComponent {
    fn render(&mut self, ctx: &mut RenderCtx<'_, '_, AppState, Msg>) { ... }

    fn handle_event(
        &mut self,
        event: &Event,
        state: &AppState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<Msg> { ... }

    fn is_focusable(&self, state: &AppState) -> bool { ... }

    fn interaction_area(&self, area: Rect) -> Rect { ... }
}
```

Every method except `render` has a default. `is_focusable` defaults to `false`;
override it for anything that should take part in Tab traversal.
`interaction_area` defaults to returning the supplied paint area unchanged.
Override it when interactive pixels occupy only part of the allocation. The
runtime still paints with the supplied area, but retains the returned area for
focus, hit-testing, pointer capture, and event routing. A non-empty result must
be fully contained in the supplied paint area; otherwise rendering panics and
the previous retained surface remains active. Returning an area with zero
width or height keeps the component's identity and paint but excludes it and its
descendants from interaction for that surface. `scope_options` and `prepare`
matter only for composites (below).
`MeasuredComponent` adds a `measure` method so containers such as the Dialog
action row can size a component before rendering it.

A reusable component is worth splitting into the library's two halves: a
stateless paint widget that only draws, and the `Component` that owns behavior
and paints by constructing the widget. Keep shared vocabulary — dimensions,
variants, `width()` — on the paint widget so layout constraints and actual paint
cannot disagree. A one-off app component can skip the split and paint directly.

## What a component may hold

A component is built fresh each frame and then kept, inert, as the surface
events route through. It is never re-rendered when state changes, so by the time
an event arrives it may describe a slightly older frame than the state does.
That is fine, as long as each piece of what it holds is read at the right
moment. There are four kinds.

**Declaration props** — label, disabled, variant, colors. Plain values taken
from state while declaring (`.disabled(state.saving)`). These are deliberately
one frame old: they describe what the user actually saw, and what the user saw
is what their click meant. A button that looked enabled on screen should press.

**Controlled bindings** — the focused row, the scroll offset, the selection.
Stored as closures (`Fn(&S) -> …`) and called inside `handle_event`, so they
read state as it is *now*. This matters because two events can arrive before the
next frame is drawn, and each one has to build on the last:

```text
render N     state.name = ""      component retained
key 'a'      reads "" from current state → emits "a";  update persists
key 'b'      reads "a" from current state → emits "ab"   ← no render between
render N+1   paints "ab"
```

Had the value been copied in at declaration time, key `b` would also have
started from `""` and the first keystroke would be lost. An edit acts on the
state left by the previous *edit*, not the previous *render*.

**Render-derived caches** — anything a later pointer event needs in order to be
interpreted, such as a scroll offset used for hit-testing. Set them in `render`,
read them in `handle_event`. They are safe because they live in the same
retained instance the event routes to. They must never become a second copy of
app state. The declared area needs no cache: `EventCtx::area` hands
`handle_event` the same rect the event was hit-tested against.

**Transient interaction state** — gesture mechanics that must outlive the
instance itself, such as a drag anchor. A field would reset every frame, so
`ctx.transient::<T>()` stores one value per identity path instead, kept for as
long as that path keeps being declared. See [Dragging](./dragging) for the
standard use.

Paint can read the same value back with `RenderCtx::transient::<T>()` — that is
how a wheel scroll survives a redraw. Prefer writing from the event side,
where a single event carries the change.

## Handling events

Return `EventResult::Ignored` when routing should continue to the parent,
`Consumed` when the event is handled with no state change, and `Emit(msg)` to
send exactly one message to the app's `update`. Only `Ignored` bubbles.
Components never mutate app state; the message is the only output.

For a primary-button `Down`, `Ignored` also permits the runtime's focus fallback
after bubbling. Pointer capture is independent: a component can call
`ctx.capture_pointer(MouseButton::Left)` and still return `Ignored` to capture
the gesture and receive the normal focus change. `Consumed` vetoes fallback;
`Emit(msg)` takes precedence and returns the component message.

Match the built-ins' conventions: name event-wiring builders `on_<event>`, keep
a continuously tracked value (`<thing>` / `on_<thing>_change`) distinct from a
committed choice (`selected` / `on_select`), and ignore events while disabled
rather than becoming unfocusable-but-reactive.

## Composites

A component may declare descendants from its own `render` through the same
`RenderCtx` methods the root uses. Children nest under the component's
identity, and their focusability is discovered by the frame's structure pass —
there is nothing to announce.

Paint container pixels before declaring descendants: retained hit order follows
declaration order and cannot see direct frame paint performed afterward.

The bookkeeping these contracts force on a composite is packaged in
`ratcn::runtime`: `BodySlot` holds a user-supplied `FnOnce` body through its
configured/painted lifecycle, and `ChildSlots` holds measured standard
children across the gap between early preparation (so the focus claim can be
answered) and rendering. `Dialog` is built on both; a custom composite can be
too.

## Checklist

- Semantic state lives in the app; the component reads it and emits messages.
- Props that describe the declaration: plain values, set while declaring.
- State that events compose against: reader closures, read in `handle_event`.
- Geometry needed to interpret events: the declared area comes from
  `EventCtx::area`; cache anything beyond it in `render`.
- Interactive geometry within the paint area: express it with `interaction_area`.
- Gesture mechanics that outlive the instance: `ctx.transient`.
- `is_focusable` reflects the same condition that makes events ignored.
- One `Emit` per event; `Ignored` only when a parent should get a chance.
