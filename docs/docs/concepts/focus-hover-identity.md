---
description: "How components get stable identities from ID paths, how Tab traversal and focus keys work, why focus lives in your app state, and why hover does not."
---

# Focus, hover, and identity

Every declared component or scope has an ID, and its full identity is the path
of IDs from the root down to it. IDs must be unique among siblings; the same ID
may appear under different parents. Fixed children pass a plain `&'static str`;
data-driven children build a `ChildId::Dynamic` once from runtime data (say,
`number.to_string().into()`) and store it, so the item keeps its identity when
it moves or the list reorders. The
[Kanban demo](./dragging#dropping-onto-a-target) uses this so each card keeps
its focus and drag state when dragged between columns.

Scopes create the nesting. A scope is a named grouping with its own path
segment and focus boundary — no component needed:

```rust
ctx.scope(
    "editor",
    pane_area,
    ScopeOptions::default().tab_wrap(TabWrap::Wrap),
    |ctx| {
        ctx.component(
            "save",
            Button::new("Save").on_press(|| Msg::Save),
            save_area,
        );
    },
);
```

The button's path is `editor/save`. Another scope may contain its own `save`,
but a second `save` directly under `editor` is a declaration error.

## Focus

Focus is a path stored in your app state — a `FocusState` bound with
`Ratcn::focus(read, on_change)`. Focus changes come back as messages for your
`update` to store, like every other state change.

You never have to compute a starting focus: an empty path means "default
startup focus", and the runtime resolves it to the first focusable component it
finds. The first time the user moves focus, your app receives a concrete path
to store.

**Tab order follows declaration order.** `TabWrap::Wrap` cycles within a scope;
`TabWrap::Escape` lets Tab leave it and continue in the parent. Shift+Tab walks
backwards.

**Focus keys** jump between panes: a `focus_key` binding on the root or a scope
maps a key chord to a path, and focus lands on that target's first focusable
leaf. Character chords ignore Shift and letter case, while Ctrl and Alt must
match exactly; the same matching is available to your own hotkey checks as
`KeyChord::matches`. There is no per-pane focus memory — jumping back into a
pane starts at its first focusable leaf again.

**Parked focus.** If the focused component disappears, is disabled, or
collapses to zero size, Ratcn keeps the stored path as-is rather than guessing
a replacement — focus is *parked*. A parked target can still render as focused
when it comes back, disabled controls ignore input meanwhile, and Tab simply
moves on to an eligible target. The one exception is an open modal: a stored
path that names something real outside it is pulled into the modal, because
the modal owns input until it closes — while a path that matches nothing stays
parked even then. Why the library never silently retargets focus is covered in
[Design decisions](./design-decisions).

**Programmatic focus.** `FocusState::intent(path)` names a path without
validating it — use it when app policy points focus somewhere that may not
exist yet, such as into a modal that opens this frame. `Ratcn::focus_path(path)`
instead validates against the last rendered frame and returns `None` for
missing, disabled, or covered targets; if the path ends at a scope, it descends
to the scope's first focusable leaf.

## Telling Ratcn what can be focused

Nothing, usually: focusable components make themselves known, and the runtime
discovers them — every frame is declared in full before focus resolves against
it, so whether focus can descend into a scope is observed, never promised. One
option changes a scope's own role:

- `focusable()` makes the scope itself the Tab stop — for a pane with nothing
  focusable inside, such as a read-only chart. Focus still prefers a focusable
  descendant when one exists.

The declare-then-draw mechanics behind this live in
[Design decisions](./design-decisions).

## Hover

Hover is the runtime's, not yours. It is a path like focus — what the pointer
is on, root-first — but nothing in your app stores it, no message carries it,
and no `update` arm applies it. Ratcn writes it itself: every pointer event
records where the pointer is, and every frame it commits resolves hover from
that position against the surface it just declared — because a redraw can move
a component out from under a pointer that never moved.

The two paths are deliberately independent: typing keeps going to the focused
field while the mouse drifts across other controls.

Why the split? Focus is a decision your app can make on its own — open a
dialog, focus its first field — so it lives in your state and moves by
message. Hover is a fact about where the mouse physically is, which no app
decides, so the runtime keeps it and answers for it.

Two places read it:

- `PaintCtx::hovered` and `PaintCtx::contains_hover`, for styling under the
  pointer.
- `DeclareCtx::pointer_within()`, while declaring, for the rarer case where
  *structure* depends on the pointer — a tooltip deciding whether to declare
  its bubble. It reports whether the pointer is on the current declaration or
  anything inside it.

A motion always comes back as at least `Consumed`, whether or not it moved
hover and whether or not a component handled it. That is your redraw signal:
the frame on screen may no longer show what the pointer is on, or where it is.

### Where paint and structure disagree

The two readers answer from different moments, and it is worth knowing which.
Paint flags are *this* frame's, resolved against the tree the declaration just
finished building. `pointer_within()` is read while that tree is still being
built, so it answers with the hover the **previous** frame resolved.

Motion does not expose the gap: a motion returns non-`Ignored`, the host
redraws, and that redraw declares with the new hover. What lags by one frame is
hover changing *without* the pointer moving — a modal opening over the hovered
node, a redraw sliding geometry out from under it. Such a frame paints the new
answer and declares from the old one, so a tooltip whose trigger has just been
covered keeps its bubble for exactly one frame.

### Gestures freeze it

While a mouse button is held, hover stops following the pointer and stays on
whatever the gesture started on — otherwise the geometry a drag moves would
chase the pointer dragging it. Releasing hands it back.

The freeze holds the *path*, not the geometry: a frozen target that is
redeclared somewhere else paints hovered where it now is. And it lasts only
while that target could still be under the pointer at all — a modal that covers
it, or a redraw that drops it, ends the freeze on the frame that does so, even
though the gesture itself runs on.

### Focus following the mouse

If you want focus to follow the mouse, opt in with `Ratcn::hover_focus()` at
the root or `ScopeOptions::hover_focus()` on a scope. Everywhere else, hover
and focus stay independent.

**The setting belongs to the scope whose children you want the mouse to
choose between**, and that is usually the root. Motion focuses the *direct
child* of that scope which the pointer entered, descending to its first
focusable leaf; motion between components *inside* that child changes nothing.

That distinction is the whole point. A pane grid with `hover_focus()` at the
root behaves as expected — the mouse picks the pane, then the keyboard works
inside it. Set it on the pane instead and every drift between two buttons in
that pane moves focus, which is rarely what anyone wants:

```rust
// Usually right: the mouse picks the pane, not the control inside it.
Ratcn::new().hover_focus()

// Rarely right: every move between controls inside the pane steals focus.
ctx.scope(id, area, ScopeOptions::default().hover_focus(), declare)
```

Focus follows the mouse *in*, but never out. Moving the pointer off a scope
onto empty space empties hover and leaves focus where it was, because there is
nowhere better to put it.

### One event, one message

`Ratcn::handle_event` returns at most one message per event. That limit binds
focus, not hover: the motion that enters a scope emits the focus change *and*
moves hover, because hover needs no message — the runtime writes its own. The
frame that first paints the new scope focused already paints the new target
hovered.

A motion that changes hover is not swallowed by having done so: it goes on to
the components under the pointer, so a list whose cursor follows the mouse
moves it on the entering motion rather than the one after. The `hover_focus`
case is the exception — the focus change *is* the event's one message, so that
motion returns before the components under it are offered anything.

## Gesture state

Some interaction state is too short-lived for your app state but must survive
the frame-by-frame rebuild of component instances — a drag anchor, for
example. `EventCtx::transient` stores such values by identity path: they
persist while the path stays declared and are cleaned up when it disappears.
Durable values still belong in app state. See [Dragging](./dragging) for the
standard use.
