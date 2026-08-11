---
description: "A centered, bordered modal dialog for Ratatui apps: title, body, and action row, declared on its own layer, with dragging and dismiss keys built in."
---

# Dialog

`Dialog` is a centered, bordered box with a title, a body, and an action row. It
is an ordinary composite component — what makes it modal is declaring it with
`ctx.modal(...)`, which puts it on its own layer above everything else.

## Default

A title, a description, and two actions. `Dialog` measures and places the action
row itself, so there is no layout code for it.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 500px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p dialog</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/dialog-demo/index.html" title="ratcn dialog demo"></iframe>
  </div>
</div>

```rust
use ratcn::{Button, Dialog};

Dialog::new()
    .title("Delete item")
    .description("This cannot be undone.")
    .on_dismiss(|| Msg::Cancel)
    .action("cancel", Button::new("Cancel").secondary().on_press(|| Msg::Cancel))
    .action("delete", Button::new("Delete").destructive().on_press(|| Msg::Delete))
```

Tab moves between actions and wraps inside the dialog, Enter presses the focused
one, and Escape emits `.on_dismiss(...)`. A different key or chord can take
Escape's place via `.dismiss_key(...)` — it accepts a `char`, a `KeyCode`, or a
`KeyChord` such as `KeyChord::from('w').ctrl()`. Add `.offset(...)` and
`.on_offset_change(...)` to drag the border and move the box.
Wiring `.on_dismiss(...)` is also what lets the dialog itself take focus: focus
prefers a focusable child and falls back to the dialog only when there is none,
so the dismiss key still has somewhere to land. A dialog without `on_dismiss`
is never focused itself.
By default the box uses `theme.surface`; the modal backdrop further separates it
by dimming the rest of the app.

Wrapping is what a dialog wants, so it is the default. `.tab_wrap(TabWrap::Escape)`
lets traversal leave the dialog's scope when it is used as an ordinary
component. When the dialog is declared with `ctx.modal(...)`, the modal boundary
still contains Tab traversal; it never reaches the base layer.

As an ordinary non-modal component, only the painted box participates in pointer
routing, so controls outside it remain clickable.

## Opening and closing

The app decides when a dialog opens. Keep a `ModalState` beside focus and bind
it, then declare the dialog when that state says it is open:

```rust
use ratcn::runtime::{ModalState, Ratcn};

let ratcn = Ratcn::new()
    .focus(|s: &AppState| &s.focus, Msg::FocusChanged)
    .modals(|s: &AppState| &s.modals);

// In update():
state.modals.open("confirm", &mut state.focus)?;
state.modals.close(&mut state.focus);

// In render(), after the base layer:
ratcn.render(frame, &state, &theme, |ctx| {
    // ... base content first ...
    if state.modals.is_open("confirm") {
        ctx.modal("confirm", dialog, ctx.area());
    }
});
```

`open` saves the focus the user had; `close` puts it back exactly. Binding
`.modals(...)` is what stops a keypress landing on a dialog the app already
considers closed, and keeps focus correct on the dialog's first frame.

See [Layers and modals](../concepts/layers-and-modals) for declaration ordering
and the full layering contract.

## Custom content

Use `.description(...)` for confirmations. For anything else, `.content(...)`
gives you the body area and the normal declaration API:

```rust
Dialog::new().content(6, move |ctx| {
        ctx.render_component("options", List::new(options), ctx.area());
    })
```

Focusable children just work — the runtime discovers them during the frame's
structure pass. Children live in the dialog's sibling namespace, so their ids
must not collide with action ids.

`.footer(height, ...)` does the same for the action row when it needs custom
layout — a checkbox on the left, a status message beside the buttons. It cannot
be combined with `.action(...)`. `.action(...)` takes measured components,
including `Button` and `Tabs`.

## Sizing

The box sizes itself to its description. Set `.outer_width(...)` or
`.outer_height(...)` to fix either dimension; both are clamped to the area the
dialog is given. Custom `.content(height, ...)` and `.footer(height, ...)`
always declare their own measured strip height.

## Dragging

Pass the app-owned offset with `.offset(...)` and add `.on_offset_change(...)` to
make the border draggable. Without the handler the dialog does not move. The
emitted offset is clamped so the painted box stays inside the area supplied to
the dialog. See
[Dragging](../concepts/dragging).

## Styling

Use `.style(...)` to replace the theme-derived border, title, background, and
description colors. The closure receives the active theme on every render:

```rust
Dialog::new().style(|theme| {
    let mut style = DialogStyle::from_theme(theme);
    style.border = theme.accent;
    style
})
```

## Full API

Every method, with panics and edge-case detail:
[`Dialog`](https://docs.rs/ratcn/latest/ratcn/struct.Dialog.html),
[`DialogStyle`](https://docs.rs/ratcn/latest/ratcn/struct.DialogStyle.html),
[`ModalState`](https://docs.rs/ratcn/latest/ratcn/runtime/struct.ModalState.html).

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## See also

The layer mechanics a Dialog is built on, and how modal state is stacked:
[Layers and modals](../concepts/layers-and-modals).
