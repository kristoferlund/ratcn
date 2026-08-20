---
description: "Enabling mouse reporting for a ratcn app, and how clicks, hover, scrolling, and drags are routed to the component under the pointer and turned into messages."
---

# Mouse input

Mouse support is opt-in and split cleanly between you and the library, the same
way keyboard is. **You** turn mouse reporting on and feed raw events in; the
**library** routes each event to the component under the pointer and turns it
into a message — buttons press, lists select and scroll, tabs switch, things
hover, things [drag](./dragging). Where keyboard routes by
the focus path, the mouse routes through the geometry from the last successful
`Ratcn::render`.

See [Rendering and event routing](./rendering-and-events) for the shared retained
surface and bubbling contract, and [Host integration](./host-integration) for
loop ownership.

## Turning it on

A terminal reports mouse events once the host asks it to. With Ratcn's
`crossterm` feature, `InputModes` is the RAII helper that asks:

```rust
let _input_modes = ratcn::crossterm::InputModes::new()
    .mouse_capture()
    .enable()?;
```

The host decides when to enable the mode and retains the guard for the event
loop's lifetime. Dropping the last Ratcn guard that requested a mode restores
it, including on `?` or panic unwinding. A host with its own terminal lifecycle
abstraction can drive the same modes through that.

::: warning Bind it to a name
`let _input_modes = …` keeps the guard alive for the scope, and mouse capture
stays on for as long as it lives. The same retention rule applies to bracketed
paste and host-owned raw mode or alternate-screen guards.
:::

`InputModes` is feature-gated backend glue: the host's `enable()` call is what
changes terminal modes.

## Synthesizing clicks and drags

Backends deliver `Down`, `Up`, and `Moved` (crossterm may also deliver `Drag`).
A **click** ("a press and release on the same component") and a **drag** ("a
press, then a move with the button held") are higher-level — and synthesized for
you.
The `Ratcn` runtime owns one `MouseTracker`, so you feed the **raw** mouse events
straight to `handle_event`, the same call you already use for keys — there is no
separate tracker to own:

```rust
use ratcn::runtime::EventResult;

// In your event loop — hand the backend event straight to Ratcn, no
// conversion step (an unsupported event maps to `Ignored`):
let event = ratatui::crossterm::event::read()?;
if let EventResult::Emit(msg) = ratcn.handle_event(event, &state) {
    update(&mut state, msg);
}
```

`handle_event` synthesizes `Click`/`Drag`/`DragEnd` from the raw
`Down`/`Up`/`Moved` before routing: a press-then-release on one component
reaches it as a `Click`, a held-button move as a `Drag`, and the release that
ends a drag as a `DragEnd`. One raw event still produces at most one message —
the same one-event-one-message flow as keyboard.

What a click requires is that the **same component** is under the pointer at
the press and at the release. The pointer may move in between: drifting a
column while pressing a button still clicks it, the way a real mouse behaves.
Leaving the component ends the click, and returning to it before releasing
revives it. If a redraw moved or replaced things in between, a newly exposed
component at the same cell does not inherit the click.

Movement only cancels the click when a component claimed the gesture with
[`capture_pointer`](./dragging) on the `Down` — claiming is what declares the
movement a drag, and that release arrives as `DragEnd` instead. A claimed press
that never moved is still a click, so one component can both drag and be
clicked.

Hit-testing uses the geometry from the last successful render. When targets
overlap, the one declared later wins; paint-only widgets are never hit targets.
The event lands on the component under the pointer first and bubbles to its
ancestors from there, the same way keyboard events bubble from the focused
component — see
[Rendering and event routing](./rendering-and-events#routing).

## What components do with it

The normalized `MouseKind` is `Down`, `Up`, `Click`, `Drag`, `DragEnd`,
`Moved`, `Exited`, and `Scroll`. Components opt into the ones they need:

- **Primary click** activates by default — a button presses and a list row is
  chosen.
  Right and middle clicks do not activate or focus the standard built-ins. A
  primary click also moves focus, so the component is keyboard-ready immediately
  after. For a List item cursor or manual Tabs cursor, the click's selection
  message should update that app-owned cursor along with the committed value;
  those cursors are separate from runtime component focus.
- **Primary down focus is a fallback.** After the target and its ancestors
  return `Ignored`, the runtime focuses the innermost eligible target. Capturing
  the pointer does not consume the event, so capture plus `Ignored` still gets
  this fallback. `Consumed` vetoes it, while `Emit` returns the component's
  message instead.
- **Hover** is the runtime's own path, separate from focus and from your
  state: there is nothing to bind. `PaintCtx::hovered` and `contains_hover`
  let a component highlight under the pointer **without stealing focus**, so
  keyboard use of the focused component keeps working while the mouse drifts
  over a button, and `DeclareCtx::pointer_within()` answers the same question
  while declaring, for structure that depends on it. Every pointer event —
  `Down` and `Up` and `Scroll`, not only `Moved` — records where the pointer
  is; each committed frame then resolves hover from that position against the
  surface it just declared. A `Moved` event also resolves it immediately, and
  still reaches the component under the pointer afterwards.
- **A motion is never `Ignored`** once a surface exists. It comes back as at
  least `Consumed` whether or not it moved hover and whether or not any
  component handled it, because it is always news to the next frame: paint may
  read the pointer position itself through `PaintCtx::hover_position` (a tabs
  row highlights the tab under the pointer that way), so motion *within* one
  component matters as much as motion between two. That result is the redraw
  signal for a host that redraws on anything but `Ignored`.
- **A gesture freezes hover.** While a button is held — captured or not —
  hover stays on whatever the gesture started on, so the geometry a drag moves
  does not chase the pointer dragging it. The release hands it back. The
  freeze holds the *path*: a frozen target redeclared elsewhere paints hovered
  where it now is, and a frozen target that a modal covers or a redraw drops
  loses hover on that frame, gesture or no gesture.
- **Focus-follows-mouse** is the opt-in exception. By default hover never moves
  focus. `Ratcn::hover_focus()` applies at the implicit root; a nested scope uses
  `ScopeOptions::hover_focus()`. The first move onto another direct focusable
  child focuses that child's first focusable leaf. It is off everywhere it is
  not explicitly set, so hover and focus remain independent there.
- **Scroll** moves a stored, app-owned offset (lists) — the wheel
  scrolls the view, independent of the cursor.
- **Drag** moves an app-owned position, and **DragEnd** commits it. Captured
  gestures return `DragEnd` to their source; its release position lets that
  source apply its own drop-target hit test. See [Dragging](./dragging) for the
  full pattern; `EventCtx::drag` owns capture, path-transient gesture state,
  button matching, and release cleanup.
- **Exit** (`Exited`) cancels an active browser pointer gesture and empties
  hover until another pointer event arrives. It is runtime cleanup, not a
  component event, so a release outside the terminal grid cannot leave a stale
  drag or hover active when the pointer returns.

Only focus needs wiring at the root:

```rust
let ratcn = Ratcn::new().focus(|state: &AppState| &state.focus, Msg::FocusChanged);
```

The path semantics, the split between app-owned focus and runtime-owned hover,
and the `hover_focus` boundary rules are covered in
[Focus, hover, and identity](./focus-hover-identity).

## In the browser

The browser backend (ratzilla) reports mouse positions directly in **terminal
cell** coordinates (`col`/`row`) — the same space as crossterm — so no pixel
conversion or app-side mapping is needed. With ratcn's `ratzilla` feature, pass
the callback event directly to the runtime:

```rust
terminal.on_mouse_event({
    let app = Rc::clone(&app);
    move |mouse_event| app.borrow_mut().handle_event(mouse_event)
}).map_err(|error| io::Error::other(error.to_string()))?;
```

ratcn's `TryFrom` conversion accepts ratzilla's raw
`ButtonDown`/`ButtonUp`/`Moved` stream. It intentionally ignores ratzilla's
`SingleClick`/`DoubleClick`; the runtime's one tracker synthesizes clicks and
drags uniformly for native and browser input.

Page focus, key capture, and browser default prevention stay with the host.
The demos' shared host script shows one working policy — forwarding captured
keys to ratzilla's canvas and normalizing macOS Option chords from
`KeyboardEvent.code` — and an application can supply its own.
