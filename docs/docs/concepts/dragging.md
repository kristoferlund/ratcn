---
description: "How dragging works in ratcn: one shared mechanism any component can opt into, with the dragged position kept as ordinary app-owned state."
---

# Dragging

Dragging in ratcn is not a property of any one component. It is a small, shared
mechanism that any component — a built-in like `Dialog`, or one you write
yourself — can opt into. The position being dragged is ordinary **app-owned
state**, moved the same way focus is: the component emits a message, your
`update` persists it. See [State and messages](./state-and-messages) for the
ownership boundary.

Drag the panel below by clicking anywhere on it and moving the mouse.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 480px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p drag</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/drag-demo/index.html" title="ratcn drag demo"></iframe>
  </div>
</div>

The panel in that demo is not a library component — it is an ordinary
registered component written in the app, which is the point: the same pieces
that make `Dialog` draggable are available to your own components.

## The lifecycle helper

Making something draggable takes three pieces:

1. **An app-owned offset.** A `CellOffset { x, y }` lives in your state. The
   declaration passes its current value and persists changes through an
   `on_change` message.
2. **`EventCtx::drag`.** Pass each mouse event and `DragOptions`. The helper
   matches the left button by default, anchors the initial offset, captures on
   `Down`, and retains capture and movement state by declaration path across
   rebuilds.
3. **Phase handling.** `Down` consumes the press, `Moved { offset, position }`
   updates app state, and `Ended { position, moved }` handles a click-release
   or commits a drop. Unrelated events produce `Ignored`.

The `Drag` events arrive ready-made. The mouse layer turns a button-held move
into `MouseKind::Drag` (see [the event model](#where-drag-events-come-from)), so
the component does not infer held-button state from `Moved` events.

## Making a built-in draggable

`Dialog` exposes the offset pair directly. Wire it and the dialog becomes
draggable by its border:

```rust
use ratcn::{Dialog, runtime::CellOffset};

// In your app state:
struct AppState {
    dialog_offset: CellOffset,
    // ...
}

// In update:
Msg::DialogMoved(offset) => state.dialog_offset = offset,

// When building the dialog:
let dialog = Dialog::new()
    .offset(state.dialog_offset)
    .on_offset_change(Msg::DialogMoved)
    .title("Confirm")
    // ...;

let area = ctx.area();
ctx.modal("confirm", dialog, area);
```

The durable offset remains yours, in app state. Events use the dialog geometry
and resolved offset from the last successful render. `Dialog` calls the same
lifecycle helper with a start policy that requires both an offset handler and a
border hit. Pointer capture continues outside the box until release, and the
dialog clamps every emitted offset to the screen.

## Making your own component draggable

A component becomes draggable with the same parts. The essentials, from the
demo's `DragPanel`:

```rust
use ratcn::runtime::{CellOffset, Component, DragOptions, DragPhase, Event,
    EventCtx, EventResult, RenderCtx, clamp_offset};

struct DragPanel {
    read_offset: Box<dyn Fn(&AppState) -> CellOffset>,
    on_change: Box<dyn Fn(CellOffset) -> Msg>,
    frame_area: Rect,
}

impl Component<AppState, Msg> for DragPanel {
    fn render(
        &mut self,
        ctx: &mut RenderCtx<'_, AppState, Msg>,
    ) {
        let area = ctx.area();
        // `area` is the panel box already computed by the declaration.
        // ...draw the box at `area`...
    }

    fn handle_event(
        &mut self,
        event: &Event,
        state: &AppState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<Msg> {
        let Event::Mouse(mouse) = event else {
            return EventResult::Ignored;
        };
        let point = Position::new(mouse.column, mouse.row);
        let can_start = self.box_rect(state).contains(point);
        match ctx.drag(
            mouse,
            DragOptions::new((self.read_offset)(state)).start_if(can_start),
        ) {
            DragPhase::Down | DragPhase::Ended { .. } => EventResult::Consumed,
            DragPhase::Moved { offset, .. } => {
                let clamped = clamp_offset(
                    self.frame_area,
                    Self::centered_rect(self.frame_area),
                    offset,
                );
                EventResult::Emit((self.on_change)(clamped))
            }
            DragPhase::Ignored => EventResult::Ignored,
        }
    }
}
```

Two details worth noting:

- **Gesture state follows identity.** `EventCtx::drag` stores its transient by
  declaration path, so replacement and reordering do not interrupt a captured
  gesture while that path remains present. Durable position still belongs in
  app state.
- **You choose the handle and the bounds.** The `start_if` policy decides *what
  is draggable* (here, anywhere in the box; `Dialog` uses just its border). The
  `Moved` phase decides *how far it can go* -- `clamp_offset`
  keeps a box inside an area, but a resizable pane would clamp to min/max sizes
  instead. The helper imposes neither.

## Dropping onto a target

Free movement only needs the offset; *drag and drop* — releasing a dragged
thing onto a target — additionally uses `DragPhase::Ended`. Its `position` is
the current release coordinate, not the last movement coordinate, and `moved`
distinguishes a drag from a click-without-movement. A component can therefore
hit-test exactly where the drop landed.

Drag a card to another column below; releasing it commits the move.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 420px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p kanban</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/kanban-demo/index.html" title="ratcn kanban demo"></iframe>
  </div>
</div>

The board is, again, an ordinary app-written component. Its phase handling:

```rust
match ctx.drag(mouse, DragOptions::default().start_if(state.drag.is_none())) {
    DragPhase::Down => EventResult::Consumed,
    // The first move picks the card up; later moves update its position.
    DragPhase::Moved { offset, .. } => {
        /* Emit(DragStarted { card, offset }) or DragMoved(offset) */
    }
    DragPhase::Ended { position, moved: true } => {
        EventResult::Emit(Msg::CardDropped {
            column: self.board.column_index_at(position.x),
        })
    }
    DragPhase::Ended { moved: false, .. } => EventResult::Consumed,
    DragPhase::Ignored => EventResult::Ignored,
}
```

While a card is dragged, its slot renders as an empty bordered placeholder —
the stack never reflows mid-drag, and an aborted drag has nowhere to "jump
back" from. Which column each card sits in, and the active drag, are both
plain app state — the drop is just one more message through `update`.

The dragged card shifts its original slot by `Moved.offset`, preserving the cell
where the press began instead of centering the card under the pointer.

The demo creates its cards at launch. Each card's creation number becomes both
its displayed label and its dynamic id (`number.to_string().into()` builds the
`ChildId::Dynamic`). The app passes a reference to that stored id in each
declaration, so identity follows the card when it moves between columns.

The floating dragged card is passive paint scheduled with
`RenderCtx::defer_paint`. Deferred paint runs after ordinary declarations in the
current layer and has no identity, geometry, focus, hover, or hit target; the
card's declared slot remains the interaction source. The dragged card clears
its area before painting so border and separator glyphs underneath cannot show
through. See
[Layers and modals](./layers-and-modals) for paint ordering.

## Where drag events come from

`Drag` events are synthesized, not raw. `Ratcn` owns one tracker, so
you feed plain `Down`/`Up`/`Moved` to `handle_event` and a button-held move
arrives as `MouseKind::Drag` (a `Down`/`Up` on one component as
`MouseKind::Click`, and the release of a *claimed* drag as
`MouseKind::DragEnd`) before routing — no separate tracker to wire:

```rust
if let EventResult::Emit(msg) = ratcn.handle_event(event, &state) {
    update(&mut state, msg);
}
```

Because the offset is app state and moves are emitted live, dragging needs
nothing special from the render loop: the message updates state, the next frame
draws the new position — the same one-event-one-message flow as every other
interaction.

Hover freezes for the length of the gesture. From the press to the release,
the runtime keeps hover on whatever the gesture started on instead of following
the pointer: the thing being dragged moves under a pointer that is by
definition on it, and the panel it passes over is not something the user is
pointing at. So a dragged component can style itself with `PaintCtx::hovered`
throughout, and nothing beneath the drag lights up on the way past. The freeze
holds the path, so a dragged target redeclared at its new position keeps
painting hovered; if it is covered by a modal or stops being declared, it loses
hover on that frame even though the gesture continues.

## What stays your responsibility

The helper is deliberately small, so these remain component or app policy:

- **Clamping policy.** `clamp_offset` covers "keep this box on screen." Other
  rules (snapping, min/max sizes) are the component's own.
- **Start hit-testing.** Compute handle eligibility and pass it to `start_if`.
- **Durable identity and drop state.** Card identity, dragged-card paint, and
  target policy stay in app state and rendering code.

If the thing being dragged stops being declared mid-gesture, the runtime ends
the capture cleanly, so the release cannot land on whatever is now under the
pointer. It cannot know what the drag *meant*, though — clear your own drag
state in the same `update` that removes the thing.

For gestures that do not fit this shape, `EventCtx::transient` and
`EventCtx::capture_pointer` are public and documented on docs.rs.
