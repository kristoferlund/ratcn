---
description: "Paint ordering and the three layer kinds in ratcn: hint, popup, and modal, plus deferred paint, viewports, and the app-owned modal stack that drives them."
---

# Layers and modals

Ratcn paints in declaration order: what you declare later paints on top of what
you declared earlier. Nothing paints during the declaration itself — every paint
is queued where it was reached and replayed in that order once the tree is
complete — but the order you see on screen is the order you wrote. Three
mechanisms go beyond that order — deferred paint for passive overlays,
**layers** for content that must float above everything, and **viewports** for
content that scrolls inside a window. Pick the smallest one that does the job:

| Mechanism | Paint time and purpose | Interaction |
| --- | --- | --- |
| `paint` / `paint_widget` | Declaration order; ordinary Ratatui decoration or paint-only widgets | None |
| `component` | Declaration order, the component before its descendants | Identity, geometry, focus, hover, events |
| `defer_paint` | After the ordinary declarations in the current layer | Passive paint only |
| `hint` | A layer that explains: tooltips | Paints only; not a pointer or focus target |
| `popup` | A layer that offers a choice: dropdowns, menus | Own events; no dim, no capture, no focus stealing |
| `modal` | A layer that takes over: dialogs | Dims, captures, holds focus, traps keys |
| `viewport` | Declaration order, clipped to a visible rectangle | Descendants keep identity, focus, and events, in logical coordinates |

Use `defer_paint` for passive overlays that must land on top of the current
layer — a floating dragged card, say. The closure receives a `PaintCtx` over the
surface it belongs to — the enclosing layer's canvas, or the frame at base
level — carrying the theme and the app state. It has no identity and no hit
target, so its interaction flags are all false, it cannot receive events, and it
is not a way to defer an interactive component. A `paint` closure in the right
declaration position is simpler when ordering already works out.

## The three layer kinds

All three are one mechanism — a subtree that paints into its own canvas and
composites above everything declared outside it. They differ only in policy,
and the policy is a table in the runtime rather than a set of special cases.
Every one is callable from anywhere in the tree and anchors its subtree at the
declaring node, so `if open { ctx.popup(...) }` inside a component is the whole
ceremony.

A **hint** takes nothing. It is not a pointer target, so a press over it goes
to whatever it covers, and nothing inside it can hold focus even if the
component claims to be focusable. Because it takes no input it has no dismissal
of its own: whatever state opened it is what closes it. See
[Tooltip](../components/tooltip).

A **popup** occludes exactly its own footprint. A press inside it that nothing
handles is consumed at the popup root rather than reaching the control beneath;
a press outside routes to whatever is visibly there and additionally emits the
popup's `on_dismiss` message. Focus is never stolen — move it in with your own
message, in the same update that opens the popup. Keys bubble *through* the
popup root to the component that declared it. See [Select](../components/select).

A **modal** is the strongest: it becomes the **active layer**. While it is open
the area behind is dimmed, keyboard and mouse routing are confined to it, focus
resolves into it, and input that nothing inside handles is absorbed rather than
reaching the UI underneath. Declare stacked modals bottom to top.

## Modal state in your app

Whether a modal is open is app state, like everything else. `ModalState` stores
the stack of open modal IDs plus, for each, the focus to restore when it
closes. Open and close it in `update`, and declare the modal whenever your
state says it is open:

```rust
state.modals.open("confirm", &mut state.focus)?;

ratcn.render(frame, &state, &state.theme, |ctx| {
    // Base paint and declarations first.
    if state.modals.is_open("confirm") {
        let area = ctx.area();
        ctx.modal("confirm", dialog, area);
    }
});

state.modals.close(&mut state.focus);
```

`open` saves the current focus and moves focus intent to the new modal.
`close` pops the top modal and restores its exact saved focus. Ratcn provides
the stack and the focus bookkeeping; *when* and *which* modal opens stays your
decision.

The modal root does not have to be a component. `DeclareCtx::modal_scope` opens
the same layer around a plain scope closure — paint your own chrome and declare
children with `component`, exactly like a base-layer panel. Reach for it to
hand-roll a dialog-like layer that stays entirely app-owned; `Dialog` is the
packaged alternative with chrome, dragging, and dismiss keys built in.

## Binding the stack

Tell the runtime where your modal stack lives:

```rust
let ratcn = Ratcn::new()
    .modals(|state: &AppState| &state.modals);
```

With the binding in place, every render must declare exactly the modal IDs the
state says are open — a mismatch is a declaration bug and fails the render.
The binding also covers the brief gap between opening or closing a modal in
`update` and the redraw that reflects it: events arriving in that gap are
consumed instead of landing on a layer your state considers closed. Focus
needs no help from it — any open modal resolves focus into itself, bound or
not.

Only modals have semantic state to validate this way. Popups and hints are
opened by whatever app state your own component reads, and the runtime holds
nothing about them between frames.

See [Dialog](../components/dialog) for the packaged modal component.

## Viewports

`DeclareCtx::viewport` opens a clipped logical space. Descendants are declared
against content as wide as the visible rectangle and as tall as the content
height you give it, and the offset names the first content row on screen; an
offset past the end is clamped to the last one that fills the rectangle.
Everything a descendant sees is in that logical space — its area, its paint,
and the pointer coordinates its events carry — so a component inside a viewport
needs to know nothing about the scrolling around it. A viewport declared inside
another viewport panics, unless a modal opens between them.

Layers escape the clip. A `hint`, `popup`, or `defer_paint` closure declared
inside a viewport keeps the viewport's logical coordinates, is projected into
screen coordinates once, and lands above everything, so a dropdown near the
bottom edge stays whole. Each of those anchors to the declaration it was
reached from, and follows it out of sight: once the viewport has scrolled that
declaration off screen the layer is skipped for the frame, and it comes back
with its anchor.

A modal escapes the viewport entirely. Its area is in the coordinates of the
declaration that gave it, as every layer's is, and the modal opens at the place
on screen those coordinates name; a row the viewport has scrolled past the top
names the top edge, so a dialog opened from content that has scrolled away is
still on screen and still the layer holding focus. From there the modal is
screen-level: its frame area and everything it declares are in screen
coordinates, which is what makes a scroll area inside a dialog inside a scroll
area ordinary nesting. A popup or a hint keeps the viewport it was declared in,
so a viewport inside one of those is nested and says so.

When focus reaches a descendant the viewport is clipping, the runtime calls
`Component::reveal_in_viewport` on the component that opened the viewport, with
that descendant's logical area, before emitting the app's focus message. That
is how a scrolled-away control comes into view as Tab arrives at it.

[ScrollArea](../components/scroll-area) is this mechanism packaged with a
scrollbar and wheel and key handling.
