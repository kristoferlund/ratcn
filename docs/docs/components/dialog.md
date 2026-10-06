---
description: "A centered, bordered modal dialog for Ratatui apps: title, body, and action row, declared on its own layer, with dragging and dismiss keys built in."
---

# Dialog

`Dialog` is a centered, bordered box with a title, a body, and an action row.
It is an ordinary composite component. Declaring it with `ctx.modal(...)` is
what makes it modal: that puts it on its own layer, above everything else.

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

`Dialog` measures and places the action row itself, so there is no layout code
for it. The box uses `theme.surface`, and the modal backdrop dims the rest of
the app behind it.

Wiring `.on_dismiss(...)` makes Escape close the dialog. It also lets the dialog
itself take focus when it has no focusable child, so the dismiss key always has
somewhere to land. A dialog without `on_dismiss` is never focused itself.

## Opening and closing

The app decides when a dialog opens. Keep a `ModalState` beside focus and bind
it, then declare the dialog when that state says it is open:

```rust
use ratcn::runtime::{ModalState, Ratcn};

let mut ratcn = Ratcn::new()
    .focus(|s: &AppState| &s.focus, Msg::FocusChanged)
    .modals(|s: &AppState| &s.modals);

// In update():
state.modals.open("confirm", &mut state.focus)?;
state.modals.close(&mut state.focus);

// In render(), after the base layer:
let area = frame.area();
ratcn.render(frame, area, &state, &theme, |ctx| {
    // ... base content first ...
    if state.modals.is_open("confirm") {
        ctx.modal("confirm", confirm_dialog(), ctx.area());
    }
});
```

`open` saves the focus the user had, and `close` puts it back exactly. Binding
`.modals(...)` stops a keypress from landing on a dialog the app already
considers closed, and keeps focus correct on the dialog's first frame.

See [Layers and modals](../concepts/layers-and-modals) for declaration ordering
and the full layering contract.

## Custom content

Use `.description(...)` for confirmations. For anything else, `.content(...)`
gives you the body area and the normal declaration API:

```rust
Dialog::new().content(6, move |ctx| {
        ctx.component("options", List::new(options), ctx.area());
    })
```

Focusable children just work: the runtime discovers them as they declare.
Children share the dialog's sibling namespace with its actions, so their ids
must not collide with action ids.

`.action(...)` takes any measured component, including `Button` and `Tabs`.
When the action row needs custom layout (a checkbox on the left, a status
message beside the buttons), `.footer(height, ...)` gives you that strip the
same way `.content(...)` gives you the body. A dialog has one or the other,
not both.

## Sizing

The box sizes itself to its description. Set `.outer_width(...)` or
`.outer_height(...)` to fix either dimension; both are clamped to the area the
dialog is given. Custom `.content(height, ...)` and `.footer(height, ...)`
strips are exactly the height you pass.

## Dragging

Pass the app-owned offset with `.offset(...)` and add `.on_offset_change(...)`
to make the border draggable. Without the handler the dialog does not move.
The emitted offset is clamped so the box stays inside the area supplied to the
dialog. See [Dragging](../concepts/dragging).

## Without a modal layer

A `Dialog` declared with `ctx.component(...)` is an ordinary component. Only
the painted box takes pointer events, so controls outside it stay clickable.
Tab wraps inside the dialog by default; `.tab_wrap(TabWrap::Escape)` lets
traversal leave it. Inside `ctx.modal(...)` the modal boundary contains Tab
either way, so it never reaches the base layer.

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

## Keyboard and mouse

| Input | Does |
|---|---|
| `Tab` `Shift+Tab` | Move between actions and children, wrapping inside the dialog |
| `Enter` | Press the focused action |
| `Esc` | Emit `.on_dismiss(...)` |
| Drag the border | Move the box, with `.on_offset_change(...)` |

`.dismiss_key(...)` puts a different key or chord in Escape's place. It accepts
a `char`, a `KeyCode`, or a `KeyChord` such as `KeyChord::from('w').ctrl()`.

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## Full API

Every method, with panics and edge-case detail:
[`Dialog`](https://docs.rs/ratcn/latest/ratcn/struct.Dialog.html),
[`DialogStyle`](https://docs.rs/ratcn/latest/ratcn/struct.DialogStyle.html),
[`ModalState`](https://docs.rs/ratcn/latest/ratcn/runtime/struct.ModalState.html).

## See also

The layer mechanics a Dialog is built on, and how modal state is stacked:
[Layers and modals](../concepts/layers-and-modals).
