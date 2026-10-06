---
description: "How dragging works in ratcn: one shared mechanism any component can opt into, with the dragged position kept as ordinary app-owned state."
---

# Dragging

Dragging in ratcn is not a property of any one component. It is a small, shared
mechanism that any component, a built-in like `Dialog` or one you write
yourself, can opt into. The position being dragged is ordinary **app-owned
state**, moved the same way focus is: the component emits a message, and your
`update` persists it. See [State and messages](./state-and-messages) for the
ownership boundary.

Drag the block below by clicking anywhere on it and moving the mouse.

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

The block in that demo is not a library component. It is an ordinary component
declared by the app, and that is the point: the same pieces that make `Dialog`
draggable are available to your own components.

## The lifecycle helper

Making something draggable takes three pieces:

1. **An app-owned offset.** A `CellOffset { x, y }` lives in your state. The
   declaration passes its current value and persists changes through an
   `on_change` message.
2. **`EventCtx::drag`.** Pass it each mouse event and a `DragOptions`. The
   helper matches the left button by default, anchors the initial offset,
   captures the pointer on `Down`, and keeps the capture and movement state by
   declaration path across rebuilds.
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

The durable offset stays yours, in app state. `Dialog` calls the same lifecycle
helper, starting a drag only when it has an offset handler and the press lands
on its border. The pointer stays captured outside the box until release, and
the dialog clamps every emitted offset to the screen.

A drag handle shows its [pointer shape](./mouse#pointer-shape) from paint:
`Grab` while hovered, `Grabbing` while `ctx.pointer_captured()`.

## Making your own component draggable

A component becomes draggable with the same parts. Here is the drag demo's
`DraggableBlock`, with its `paint` and its label field left out:

```rust
use ratatui::layout::{Constraint, Rect};
use ratcn::runtime::{
    Component, DeclareCtx, DragOptions, DragPhase, Event, EventCtx, EventResult,
    clamp_offset,
};

struct DraggableBlock {
    /// The frame area the block stays inside.
    area: Rect,
}

impl Component<AppState, Msg> for DraggableBlock {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, AppState, Msg>) {}

    fn handle_event(
        &mut self,
        event: &Event,
        state: &AppState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<Msg> {
        let Event::Mouse(mouse_event) = event else {
            return EventResult::Ignored;
        };
        match ctx.drag(mouse_event, DragOptions::new(state.block_offset)) {
            DragPhase::Down | DragPhase::Ended { .. } => EventResult::Consumed,
            DragPhase::Moved { offset, .. } => {
                let block = self.area.centered(
                    Constraint::Length(BLOCK_WIDTH),
                    Constraint::Length(BLOCK_HEIGHT),
                );
                EventResult::Emit(Msg::BlockMoved(clamp_offset(self.area, block, offset)))
            }
            DragPhase::Ignored => EventResult::Ignored,
        }
    }
}
```

The declaring side offsets the block's area with `offset_rect` before passing
it to `ctx.component`, and `paint` styles it from `ctx.hovered()`. Three details
are worth noting:

- **Gesture state follows identity.** `EventCtx::drag` stores its state by
  declaration path, so replacing or reordering components does not interrupt a
  captured gesture while that path remains present. The durable position still
  belongs in app state.
- **You choose the handle.** `DragOptions::start_if` decides *what is
  draggable*. The block above never calls it and takes the default, a press
  anywhere in its area; `Dialog` passes a border hit test, and the kanban cards
  below pass "no card is in flight yet".
- **You choose the bounds.** The `Moved` phase decides *how far it can go*.
  `clamp_offset` keeps a box inside an area, and a resizable pane clamps to
  min/max sizes of its own.

## Dropping onto a target

Free movement only needs the offset. *Drag and drop*, releasing a dragged thing
onto a target, also uses `DragPhase::Ended`. Its `position` is where the
release happened, so a component can hit-test exactly where the drop landed,
and `moved` tells a drag apart from a click on the handle.

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

Each card is, again, an ordinary app-written component. This is `KanbanCard`'s
phase handling:

```rust
match ctx.drag(
    mouse_event,
    DragOptions::default().start_if(state.active_drag.is_none()),
) {
    DragPhase::Down => EventResult::Consumed,
    // The first move picks the card up; later moves update its position.
    DragPhase::Moved { offset, .. } => match &state.active_drag {
        Some(active) if active.card_id == self.card_id => {
            EventResult::Emit(Msg::DragMoved(offset))
        }
        None => EventResult::Emit(Msg::DragStarted(ActiveDrag {
            card_id: self.card_id.clone(),
            offset,
        })),
        Some(_) => EventResult::Consumed,
    },
    DragPhase::Ended { position, moved } => {
        let is_mine = state
            .active_drag
            .as_ref()
            .is_some_and(|active| active.card_id == self.card_id);
        if moved && is_mine {
            EventResult::Emit(Msg::CardDropped {
                target_column_index: self.board_layout.column_index_at(position),
            })
        } else {
            EventResult::Consumed
        }
    }
    DragPhase::Ignored => EventResult::Ignored,
}
```

`column_index_at` answers `None` for a release outside every column, which the
update treats as a canceled drag.

Which column each card sits in, and the active drag, are both plain app state;
the drop is just one more message through `update`. While a card is dragged,
its slot paints as an empty bordered placeholder, so the stack never reflows
mid-drag and an aborted drag has nowhere to "jump back" from. The dragged card
itself is drawn at its original slot shifted by `Moved.offset`, so the cell
where the press began stays under the pointer.

The demo creates its cards at launch. Each card's creation number becomes both
its displayed label and its dynamic id (`number.to_string().into()` builds the
`ChildId::Dynamic`). The app passes a reference to that stored id in each
declaration, so identity follows the card when it moves between columns.

The floating dragged card is a `DeclareCtx::hint` layer declared by the card
being dragged. A hint paints above every card declared after it and takes no
input (no focus, no hover, no hit target), so the card's declared slot remains
the interaction source. Layers are transparent, so the dragged card clears its
area before painting, and the border and separator glyphs underneath cannot
show through. See [Layers and modals](./layers-and-modals) for paint ordering.

## Where drag events come from

`Drag` events are synthesized, not raw. `Ratcn` owns one gesture tracker, so
you feed plain `Down`/`Up`/`Moved` events to `handle_event`, and before routing
a button-held move arrives as `MouseKind::Drag`. A `Down`/`Up` on one component
arrives as `MouseKind::Click`, and the release of a *claimed* drag as
`MouseKind::DragEnd`. There is no separate tracker to wire:

```rust
if let EventResult::Emit(msg) = ratcn.handle_event(event, &state) {
    update(&mut state, msg);
}
```

Because the offset is app state and moves are emitted live, dragging needs
nothing special from the render loop. The message updates state, and the next
frame paints the new position: the same one-event-one-message flow as every
other interaction.

Hover freezes for the length of the gesture. From the press to the release,
the runtime keeps hover on whatever the gesture started on instead of following
the pointer. The thing being dragged moves under a pointer that is by
definition on it, and the panel it passes over is not something the user is
pointing at. So a dragged component can style itself with `PaintCtx::hovered`
throughout, and nothing beneath the drag lights up on the way past. The freeze
holds the declaration path, so a dragged target redeclared at its new position
keeps painting hovered. If a modal covers it or it stops being declared, it
loses hover on that frame even though the gesture continues.

## What stays your responsibility

The helper covers capture, anchoring, and button matching. These remain
component or app policy:

- **Clamping policy.** `clamp_offset` covers "keep this box on screen." Other
  rules (snapping, min/max sizes) are the component's own.
- **Start hit-testing.** Compute handle eligibility and pass it to `start_if`.
- **Durable identity and drop state.** Card identity, dragged-card paint, and
  target policy stay in app state and rendering code.

If the thing being dragged stops being declared mid-gesture, the runtime ends
the capture cleanly, so the release cannot land on whatever is now under the
pointer. It cannot know what the drag *meant*, though, so clear your own drag
state in the same `update` that removes the thing.

For gestures that do not fit this shape, `EventCtx::transient` and
`EventCtx::capture_pointer` are public and documented on
[docs.rs](https://docs.rs/ratcn/latest/ratcn/runtime/struct.EventCtx.html).
Check `EventCtx::pointer_captured` before continuing such a gesture: a
descendant's captured drag bubbles up too, and only the owner sees it as
captured.
