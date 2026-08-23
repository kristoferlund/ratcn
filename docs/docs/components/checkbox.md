---
description: "A labeled boolean control for Ratatui apps: marker left, label right, the whole row one hit target. The checked and unchecked markers are yours to choose."
---

# Checkbox

A labeled boolean control: the marker on the left, the label on the right, and
the whole row as one hit target — a click on the label checks the box exactly
as a click on the marker does.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 260px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p checkbox</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/checkbox-demo/index.html" title="ratcn checkbox demo"></iframe>
  </div>
</div>

```rust
use ratcn::{Checkbox, Theme};

// In draw(), declare one Checkbox per option:
ctx.component(
    "vim",
    Checkbox::new("Vim bindings").checked(|state| state.vim, Msg::SetVim),
    area,
);
```

Enter or Space toggles while focused. Nothing paints a background, hover
changes nothing, and focus recolors the label — a checkbox reads as text on the
surface it sits on, with the one visible state keyboard users need.

## The markers are yours

The default pair is `☑` / `☐`. Because both markers are strings you choose,
the same component covers every binary control:

```rust
// An ASCII checkbox:
Checkbox::new("Telemetry")
    .checked_marker("[x]")
    .unchecked_marker("[ ]")

// A switch — the words are the point:
Checkbox::new("Terminal bell")
    .checked_marker("[ON]")
    .unchecked_marker("[off]")
```

Two options are a Checkbox wearing its states as labels; three or more are a
[Cycle](./cycle).

## State

The checked value is app-owned and arrives through `.checked(read, on_change)`:
`read` answers from app state each frame, `on_change` receives the requested
state after every toggle. Without the binding the checkbox paints but is not
focusable and answers no events.

## Paint-only widget

`CheckboxWidget` draws one row without focus, events, or state:

```rust
use ratcn::CheckboxWidget;

frame.render_widget(
    CheckboxWidget::new("Vim bindings", state.vim).themed(&theme),
    area,
);
```

`.width()` measures the columns it wants — marker, space, label — for layouts
that reserve exactly that. Replace `.themed(...)` with `.style(...)` to supply
exact colors.

## Full API

Every method, with binding requirements and edge-case detail:
[`Checkbox`](https://docs.rs/ratcn/latest/ratcn/struct.Checkbox.html),
[`CheckboxWidget`](https://docs.rs/ratcn/latest/ratcn/struct.CheckboxWidget.html),
[`CheckboxStyle`](https://docs.rs/ratcn/latest/ratcn/struct.CheckboxStyle.html).

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## See also

Use [Cycle](./cycle) when a setting has more than two options, or [List](./list)
with multi-selection when the user ticks any number of rows in a longer list.
