---
description: "A one-line text field for Ratatui apps: the value lives in app state, the caret in a transient. Placeholder, well, and grapheme-aware editing."
---

# Input

A one-line text field. The value lives in your state; the caret is
presentation, kept by the runtime against the field's identity so a rebuild
does not lose it.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 300px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p input</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/input-demo/index.html" title="ratcn input demo"></iframe>
  </div>
</div>

```rust
use ratcn::Input;

ctx.component(
    "name",
    Input::new()
        .placeholder("Your name")
        .value(|state| state.name.as_str(), Msg::SetName),
    area,
);
```

The field is an inset well, the same surface a [List](./list) or a closed
[Select](./select) sits in: one cell of padding on each side of the value,
one row tall. Empty, it shows the placeholder. Focused, a blinking `|`
overlays the insertion point without taking a column, so moving it never
shifts the letters. Tab moves between fields; type, Backspace, Delete,
Ctrl+Backspace, Ctrl+Delete, Option+Backspace, the arrows, Home, and End
edit. Hosts disagree on the bytes — Windows and WSL send Ctrl+H, macOS
Option is Alt+Backspace — and the field treats those as the same chord.
Paste inserts
as one piece, with newlines flattened so the field stays one line.

Enter and Esc are left to the app: this is a field, not a form.

## State

The string is app-owned and arrives through `.value(read, on_change)`: `read`
answers from app state each frame, `on_change` receives the whole new string
after every edit. Without the binding the field paints but is not focusable
and answers no events.

The caret is a transient. A click places it at the grapheme under the
pointer; a first keystroke with no click yet types at the end.

## Paint-only widget

`InputWidget` draws one row without focus, events, or state:

```rust
use ratcn::InputWidget;

frame.render_widget(
    InputWidget::new(&state.name)
        .placeholder("Your name")
        .themed(&theme),
    area,
);
```

Pass `.caret(Some(index))` to overlay a `|` at that grapheme, and
`.caret_on(false)` to hide it for the off half of a blink. Replace
`.themed(...)` with `.style(...)` to supply exact colors.

## Full API

Every method, with binding requirements and edge-case detail:
[`Input`](https://docs.rs/ratcn/latest/ratcn/struct.Input.html),
[`InputWidget`](https://docs.rs/ratcn/latest/ratcn/struct.InputWidget.html),
[`InputStyle`](https://docs.rs/ratcn/latest/ratcn/struct.InputStyle.html).

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## See also

Use [Checkbox](./checkbox) or [Cycle](./cycle) for a setting whose value is
chosen, not typed. A multi-line field is not this component.
