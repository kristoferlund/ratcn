---
description: "Writing your own ratcn component: the Component trait, the four kinds of data a component carries and when each is read, and how composites declare children."
---

# Custom components

Ratcn's built-ins are not special. `DeclareCtx::component` accepts any
implementation of the `Component` trait, and the runtime gives a custom
component the same identity, focus, hover, hit-testing, and event routing it
gives `Button`. The kanban demo's `KanbanCard` — a draggable card with
board-aware drop handling — is a complete custom component in about a hundred
lines.

Write a component when the demoed behavior needs real event handling or its own
interaction identity. Purely decorative content should stay direct paint:
build a Ratatui widget and paint it from a `ctx.paint` closure. Paint-only
content declared as a component takes on identity, traversal, and hit-testing
it never uses.

## The trait

```rust
// A frame reaches the first five in this order. `handle_event` runs on the
// retained instance between frames, and `reveal_in_viewport` opens the frame
// that answers a focus move.
impl Component<AppState, Msg> for MyComponent {
    fn prepare(&mut self, state: &AppState) { ... }

    fn scope_options(&self) -> ScopeOptions { ... }

    fn interaction_area(&self, area: Rect) -> Rect { ... }

    fn declare(&mut self, ctx: &mut DeclareCtx<'_, AppState, Msg>) { ... }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, AppState>) { ... }

    fn handle_event(
        &mut self,
        event: &Event,
        state: &AppState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<Msg> { ... }

    fn reveal_in_viewport(
        &mut self,
        target: Rect,
        state: &AppState,
        ctx: &mut EventCtx<'_>,
    ) { ... }
}
```

Declaring and painting are two methods because they happen in two walks.
`declare` lays the component out and declares its descendants, and paints
nothing. `paint` writes cells, after the whole tree is declared and focus has
resolved — which is why the interaction flags (`ctx.focused()`,
`ctx.contains_focus()`, `ctx.hovered()`, `ctx.contains_hover()`) live on
`PaintCtx` and not on `DeclareCtx`: while `declare` runs, focus has nothing
complete to resolve against yet. Hover is the exception, because it predates
the frame rather than following from it: `DeclareCtx::pointer_within()` reports
whether the pointer is inside this declaration, for the rare component whose
*structure* depends on it. Both methods run once per frame, so anything
`handle_event` reads back must be recorded in `declare`, and must therefore not
depend on those flags.

Every method except `declare` has a default. `paint` defaults to painting
nothing, which is right for a composite that is only a container.
`scope_options` carries the focus claim:
`ScopeOptions::default().focusable(true)` is how anything that should take part
in Tab traversal says so, and it defaults to `false`. It answers from the props
the component was declared with, so resolve anything state-dependent in
`prepare` first. The same options shape a composite's scope (below).
`interaction_area` defaults to returning the supplied paint area unchanged.
Override it when interactive pixels occupy only part of the allocation. The
runtime still paints with the supplied area, but retains the returned area for
focus, hit-testing, pointer capture, and event routing. A non-empty result must
be fully contained in the supplied paint area; otherwise rendering panics and
the previous retained surface remains active. Returning an area with zero width
or height keeps the component's identity and paint but excludes it and its
descendants from interaction for that surface. `prepare` runs once per
declaration, before any of those answers are read, so all of them may be
computed from what it pins: `Select` resolves its open flag there, and `List`,
`Select`, and `Tabs` use it to fail loud when two items carry the same value.
`reveal_in_viewport` is called on the component that declared a viewport when
focus lands on a descendant the viewport clips, so it can scroll that descendant
into view; [Layers and modals](./layers-and-modals) covers when the call
arrives, including the focus changes it answers on the frame after.
`MeasuredComponent` adds a `measure` method so containers such as the Dialog
action row can size a component before declaring it.

A reusable component is worth splitting into the library's two halves: a
stateless paint widget that only paints, and the `Component` that owns behavior
and paints by constructing the widget. Keep shared vocabulary — dimensions,
variants, `width()` — on the paint widget so layout constraints and actual paint
cannot disagree. A one-off app component can skip the split and paint directly.

## What a component may hold

A component is built fresh each frame and then kept, inert, as the surface
events route through. It is never rebuilt when state changes, so by the time
an event arrives it may describe a slightly older frame than the state does.
That is fine, as long as each piece of what it holds is read at the right
moment. There are four kinds.

**Declaration props** — label, disabled, variant, colors. Plain values taken
from state while declaring (`.disabled(state.saving)`). These are one frame
old: they describe what the user actually saw, and what the user saw
is what their click meant. A button that looked enabled on screen should press.

**Controlled bindings** — the focused row, the scroll offset, the selection.
Stored as closures (`Fn(&S) -> …`) and called inside `handle_event`, so they
read state as it is *now*. This matters because two events can arrive before the
next frame is painted, and each one has to build on the last:

```text
render N     state.name = ""      component retained
key 'a'      reads "" from current state → emits "a";  update persists
key 'b'      reads "a" from current state → emits "ab"   ← no render between
render N+1   paints "ab"
```

Had the value been copied in at declaration time, key `b` would also have
started from `""` and the first keystroke would be lost. An edit acts on the
state left by the previous *edit*, not the previous *render*.

**Declaration-derived caches** — anything a later pointer event needs in order
to be interpreted, such as a scroll offset used for hit-testing. Set them in
`declare`, read them in `handle_event`. They are safe because they live in the same
retained instance the event routes to. They must never become a second copy of
app state. The declared area needs no cache: `EventCtx::area` hands
`handle_event` the same rect the event was hit-tested against.

**Transient interaction state** — gesture mechanics that must outlive the
instance itself, such as a drag anchor. A field would reset every frame, so
`ctx.transient::<T>()` stores one value per identity path instead, kept for as
long as that path keeps being declared. See [Dragging](./dragging) for the
standard use.

`declare` can read the same value back with `DeclareCtx::transient::<T>()` — that
is how a wheel scroll survives a redraw. Prefer writing from the event side,
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

A component may declare descendants from its own `declare` through the same
`DeclareCtx` methods the root uses — `ctx.component` for a child with
behavior, `ctx.scope` for a region that only needs its own Tab boundary and
path segment. Children nest under the component's identity, and their
focusability is discovered as they declare — there is nothing to announce.

Container pixels need no care about order. Every component's `paint` is queued
where `declare` declared it and replayed in that order, and a component is
queued at the point it opens — so a container's background and border land
beneath everything it declares inside itself, whatever order the two methods
are written in. The same holds for `ctx.paint` closures: each is queued where
it was reached.

The queue position is fixed, so decoration that has to cover a composite's
*descendants* — a dimming wash — cannot come from `Component::paint` at all,
which is queued before them. A `ctx.paint` closure reached *after* those
declarations is queued after them, on the same layer, and is the usual answer.
`ctx.defer_paint` goes one step further, flushing after the current layer has
finished declaring, which is what decoration that must also cover *later
siblings* — a drag ghost — needs; it has no identity or geometry of its own.

What still follows declaration order is hit-testing, and it knows nothing about
pixels: a later sibling painted underneath another still takes the clicks over
its own area.

A composite is an ordinary `Component`; there is no composite trait to implement
and no lifecycle to opt into. What a composite does need is somewhere to keep
what its builders were handed until `declare` uses it, and a way to keep
answering geometry questions once that is gone. In practice that is four
pieces:

**Deferred painting.** A `paint` closure runs after declaration has ended, so it
owns what it paints with: it is `'static` and receives a `PaintCtx` carrying the
theme, the state, the area, and the interaction flags. Layout computed while
declaring has to be moved in. Where a style-dependent widget also produces
layout — a bordered `Block` whose colors follow focus — compute the layout from
a plain block (`Block::bordered().padding(p).inner(area)`, which depends only on
borders and padding) and build the styled one inside the closure.

**Caller-supplied bodies.** A region the caller fills is a closure, and it
should be `FnOnce` so the caller can move owned values into it. Store it as
`Option<Box<dyn FnOnce(&mut DeclareCtx<'_, S, M>)>>` and `take()` it in
`declare`, then hand it the area you chose for it with
`ctx.in_area(area, body)`. The body's children land in the composite's own
sibling namespace, so ids must be unique across every body it declares.

**Measured children.** A child the composite places itself has to be sized
before it is declared. Accept it as `impl MeasuredComponent<S, M> + 'static`,
call `measure()` in the builder, and keep the `Size` beside a closure that
declares it — the closure is what gets boxed, not the child, because
`Box<dyn Component>` does not itself implement `Component`. `declare` computes
each area from the stored sizes and runs the closures in insertion order, which
is also Tab order.

**Geometry that outlives the closures.** `handle_event` runs on the retained
instance, after `declare` has taken every closure, and it still has to recompute
the box the pointer landed in. So keep the layout *facts* — heights, measured
sizes, the fact that a body was configured at all — in fields that taking a
closure does not empty, and derive the rects from them in one function that
`declare`, `paint`, `interaction_area`, and `handle_event` all call. Anything
derived twice from two places will eventually disagree, and hit-testing is where
that shows up.

`Dialog` is the library's own reference implementation of all four: a body that
is either a description or a caller's closure, a footer that is either a
caller's closure or a measured action row, a private `dims` every rect comes
from, and border dragging that re-derives the box between frames.

Copy `components/dialog.rs` into your own crate and edit it: the `copy-fixture`
crate does exactly that with every copyable built-in and compiles each one alone
against `ratcn` as an ordinary external dependency, so a built-in can only use
what you can use too.

## Checklist

- Semantic state lives in the app; the component reads it and emits messages.
- Props that describe the declaration: plain values, set while declaring.
- State that events compose against: reader closures, read in `handle_event`.
- Geometry needed to interpret events: `EventCtx::area` is the rect the event
  was hit-tested against — the interaction area, narrowed if
  `interaction_area` returned one. Retain the paint allocation in `declare` when
  the geometry has to come from that instead, and cache anything else there
  too, never in `paint`.
- Everything that paints: `paint`, styled from its interaction flags.
- Interactive geometry within the paint area: express it with `interaction_area`.
- Gesture mechanics that outlive the instance: `ctx.transient`.
- The focus claim in `scope_options` answers from the props alone, and
  reflects the same condition that makes events ignored. Settle anything
  state-dependent in `prepare`.
- One `Emit` per event; `Ignored` only when a parent should get a chance.
