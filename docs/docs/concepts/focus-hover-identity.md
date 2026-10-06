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
segment and focus boundary, and it needs no component:

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

Focus is a path stored in your app state: a `FocusState` bound with
`Ratcn::focus(read, on_change)`. Focus changes come back as messages for your
`update` to store, like every other state change.

You never have to compute a starting focus. `FocusState::default()` or an empty
`FocusState::intent(path)` means "default startup focus", and the runtime
resolves it to the first focusable component it finds. The first time the user
moves focus, your app receives a concrete path to store.

**No focus.** Store `FocusState::none()` when no component should paint as
focused, such as in an inactive hosted pane. It is distinct from default focus,
even though both have empty paths, and it does not disable input: Tab enters the
first eligible control, Shift+Tab the last, root `focus_key` bindings still
work, and pointer input can focus a control. Your app decides which events reach
an inactive pane. Opening a modal saves `none()` and moves focus into the modal;
closing it restores `none()`. See
[`FocusState`](https://docs.rs/ratcn/latest/ratcn/runtime/struct.FocusState.html)
for the exact semantics.

**Tab order follows declaration order.** `TabWrap::Wrap` cycles within a scope;
`TabWrap::Escape` lets Tab leave it and continue in the parent. Shift+Tab walks
backwards.

**Focus keys** jump between panes: a `focus_key` binding on the root or a scope
maps a key chord to a path, and focus lands on that target's first focusable
leaf. Character chords ignore Shift and letter case, while Ctrl and Alt must
match exactly; your own hotkey checks can use the same matching through
`KeyChord::matches`. There is no per-pane focus memory, so jumping back into a
pane starts at its first focusable leaf again.

**Parked focus.** If the focused component disappears, is disabled, or
collapses to zero size, the runtime keeps the stored path as it is rather than
guessing a replacement. Focus is *parked*. A parked target can still paint as
focused when it comes back, disabled controls ignore input meanwhile, and Tab
simply moves on to an eligible target. The one exception is an open modal,
which owns input until it closes: a stored path that names something real
outside the modal is pulled into it. A path that matches nothing stays parked
even then.

**Programmatic focus.** `FocusState::intent(path)` names a path without
validating it. Use it when app policy points focus somewhere that may not exist
yet, such as into a modal that opens this frame. `Ratcn::focus_path(path)`
instead validates against the last rendered frame and returns `None` for
missing, disabled, or covered targets; if the path ends at a scope, it descends
to the scope's first focusable leaf.

## What can be focused

Usually you declare nothing: focusable components make themselves known, and
the runtime discovers them. Every frame is declared in full before focus
resolves against it, so whether focus can descend into a scope is observed,
never promised.

One option changes a scope's own role. `ScopeOptions::focusable(true)` makes
the scope itself the Tab stop, for a pane with nothing focusable inside, such
as a read-only chart. Focus still prefers a focusable descendant when one
exists.

The declare-then-paint mechanics behind this are in
[Rendering and event routing](./rendering-and-events).

## Hover

Hover belongs to the runtime, not to your app. It is a path like focus (what
the pointer is on, root-first), but nothing in your app stores it, no message
carries it, and no `update` arm applies it. The runtime records where the
pointer is on every pointer event, and each frame it resolves hover from that
position against the tree it just declared, because a redraw can move a
component out from under a pointer that never moved.

The two paths are independent: typing keeps going to the focused field while
the mouse drifts across other controls.

The split follows who decides. Focus is something your app can decide on its
own (open a dialog, focus its first field), so it lives in your state and moves
by message. Hover is a fact about where the mouse physically is, which no app
decides, so the runtime keeps it and answers for it.

Two places read it:

- `PaintCtx::hovered` and `PaintCtx::contains_hover`, for styling under the
  pointer.
- `DeclareCtx::pointer_within()`, while declaring, for the rarer case where
  *structure* depends on the pointer, such as a tooltip deciding whether to
  declare its bubble. It reports whether the pointer is on the current
  declaration or anything inside it.

A pointer motion always returns at least `Consumed`, whether or not it moved
hover and whether or not a component handled it. Treat that as your redraw
signal: the frame on screen may no longer show what the pointer is on, or where
it is.

### Where paint and structure disagree

The two readers answer from different moments. Paint flags describe *this*
frame, resolved against the tree the declaration just finished building.
`pointer_within()` is read while that tree is still being built, so it answers
with the hover the **previous** frame resolved.

Pointer motion hides the gap, because the redraw it triggers declares with the
new hover. The lag shows only when hover changes *without* the pointer moving,
for example when a modal opens over the hovered component or a redraw slides
geometry out from under it. That frame paints the new answer but declares from
the old one, so a tooltip whose trigger has just been covered keeps its bubble
for exactly one frame.

### Gestures freeze it

While a mouse button is held, hover stops following the pointer and stays on
whatever the gesture started on, so the geometry a drag moves does not chase
the pointer dragging it. Releasing the button hands hover back. The freeze ends
early if a modal covers the target or a redraw stops declaring it, even though
the gesture itself runs on.

### Focus following the mouse

If you want focus to follow the mouse, opt in with `Ratcn::hover_focus()` at
the root or `ScopeOptions::hover_focus()` on a scope. Everywhere else, hover
and focus stay independent.

**The setting belongs to the scope whose children you want the mouse to
choose between**, and that is usually the root. Motion focuses the *direct
child* of that scope which the pointer entered, descending to its first
focusable leaf; motion between components *inside* that child changes nothing.

A pane grid with `hover_focus()` at the root behaves as expected: the mouse
picks the pane, then the keyboard works inside it. Set it on the pane instead
and every drift between two buttons in that pane moves focus, which is rarely
what anyone wants:

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

`Ratcn::handle_event` returns at most one message per event. That limit applies
to focus, not hover, because hover needs no message. The motion that enters a
`hover_focus` scope emits the focus change *and* moves hover, so the frame that
first paints the new pane focused already paints the component under the
pointer hovered. Because the focus
change is that event's one message, the motion returns before the components
under the pointer are offered it.

Any other motion that changes hover goes on to the components under the
pointer, so a list whose cursor follows the mouse moves it on the entering
motion rather than the one after.

## Gesture state

Some interaction state is too short-lived for your app state but must survive
the frame-by-frame rebuild of component instances. A drag anchor is the typical
example. `EventCtx::transient` stores such values by identity path: they
persist while the path stays declared and are cleaned up when it disappears.
Durable values still belong in app state. See [Dragging](./dragging) for the
standard use.
