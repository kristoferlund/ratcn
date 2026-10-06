---
description: "A focusable terminal button for Ratatui apps. Wire on_press to emit a message; Tab moves between buttons and Enter, Space, or a click presses one."
---

# Button

A button that emits a message when pressed. `.on_press(...)` supplies the
message, and makes the button focusable; a button without it paints but
ignores keys and clicks. The default size is a single row with no border.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 260px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p button-small</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/button-small-demo/index.html" title="ratcn small button demo"></iframe>
  </div>
</div>

```rust
use ratcn::Button;

ratcn.render(frame, save_area, state, &state.theme, |ctx| {
    let save = Button::new("Save")
        .disabled(state.saving)
        .on_press(|| Msg::Save);

    ctx.component("save", save, save_area);
});
```

A button holds no state. Its label and disabledness are values you pass at
declaration, so they come straight from app state.

## Variants

Five variants set visual weight. They only change colors, so all are pressed
the same way. Each has a shorthand builder, or pass one to `.variant(...)`.

| Variant | Shorthand | Use for |
| --- | --- | --- |
| `Default` | (the default) | The main action on a screen. Filled with the primary color. |
| `Secondary` | `.secondary()` | A supporting action. Filled, but muted. |
| `Outline` | `.outline()` | A quiet action that still needs an edge. Border, no fill. |
| `Ghost` | `.ghost()` | The quietest. No fill or border until focused or hovered. |
| `Destructive` | `.destructive()` | Deleting or discarding. Filled with the destructive color. |

`Outline` needs a `Large` button to have a border to paint. At `Small` it has no
focus or hover indication; use `Ghost` for a quiet small button instead.

## Large

`ButtonSize::Large` is three rows: room for a border, or a fill cap above and
below the label.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 260px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p button-large</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/button-large-demo/index.html" title="ratcn large button demo"></iframe>
  </div>
</div>

```rust
use ratcn::{Button, ButtonSize};

Button::new("Delete")
    .destructive()
    .size(ButtonSize::Large)
    .on_press(|| Msg::Delete)
```

Use `ButtonSize::Large.height()` for layout constraints rather than
hard-coding a number, and `.width()` to size a button to its own label.

## Disabled

A disabled button is grayed out, skipped by Tab, and ignores events. Pass the
flag from app state, which is in scope while declaring.

```rust
Button::new("Save")
    .disabled(!state.can_save)
    .on_press(|| Msg::Save)
```

## Styling

Colors derive from the theme and the variant, so most buttons need no styling
call. To recolor one button, override the resolved style. The closure receives
the active theme each render, so a style built from it follows theme switches:

```rust
use ratcn::{ButtonStyle, ButtonVariant};

Button::new("Archive")
    .on_press(|| Msg::Archive)
    .style(|theme| {
        let mut style = ButtonStyle::from_theme(theme, ButtonVariant::Default);
        style.background = theme.accent;
        style
    })
```

A state whose `border` is `Some` paints as an outline; `None` paints a fill.
`ButtonStyle::fallback()` is the no-theme starting point: plain ANSI colors
that render on any terminal.

## Paint-only widget

`ButtonWidget` paints a button without focus or events. It is an ordinary
Ratatui widget, so it works in a plain Ratatui app with no `Ratcn` runtime. It
has the same variant and size builders, and you supply the interaction states
with `.focused(...)`, `.hovered(...)`, and `.disabled(...)`:

```rust
use ratcn::{ButtonSize, ButtonWidget};

frame.render_widget(
    ButtonWidget::new("Save")
        .secondary()
        .size(ButtonSize::Large)
        .themed(&theme)
        .focused(is_focused)
        .hovered(is_hovered),
    area,
);
```

## Keyboard and mouse

| Input | Does |
|---|---|
| `Enter` `Space` | Press the focused button |
| Left click | Press the button |

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## Limits

- A `Large` button given fewer than three rows paints nothing and takes no
  focus or clicks. Too little width only clips the label.
- In an area taller than the button, only its own one or three rows paint and
  take clicks.
- A click on a disabled button can bubble to a containing component, but never
  reaches an overlapping sibling behind it.

## Full API

See
[`Button`](https://docs.rs/ratcn/latest/ratcn/struct.Button.html),
[`ButtonWidget`](https://docs.rs/ratcn/latest/ratcn/struct.ButtonWidget.html),
[`ButtonVariant`](https://docs.rs/ratcn/latest/ratcn/enum.ButtonVariant.html),
[`ButtonSize`](https://docs.rs/ratcn/latest/ratcn/enum.ButtonSize.html),
and [`ButtonStyle`](https://docs.rs/ratcn/latest/ratcn/struct.ButtonStyle.html)
on docs.rs for the full API.

## See also

Use [Dialog](./dialog) when a confirmation needs its own layer, and
[Toast](./toast) to acknowledge what a press did.
