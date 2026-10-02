---
description: "A single-line text field for Ratatui apps: app-owned text, a titled border, placeholder, masking for secrets, an invalid look, and click-to-place and drag-to-select with the mouse."
---

# Input

A single-line text field: the value in a well, a block cursor while focused,
and an optional titled border around it. Typing, selection, and scrolling a
value longer than the field are handled; what the text means is yours.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 460px">
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
use ratcn::{Input, InputState};

#[derive(Default)]
struct AppState {
    name: InputState,
}

enum Msg {
    Name(InputState),
    Save,
}

// In draw(), declare the field:
ctx.component(
    "name",
    Input::new()
        .value(|state: &AppState| &state.name, Msg::Name)
        .title("Name")
        .placeholder("Ada Lovelace")
        .on_submit(|| Msg::Save),
    area,
);

// In update(), store what it emits:
Msg::Name(name) => state.name = name,
```

An untitled Input is one row; `.title(...)` draws a border with the title on
it, and the field is then three. Either way it sits at the top of the area it
is declared in and ignores rows below that.

## State

The text lives in your state as an `InputState`, not in the component. It
holds the text, the cursor, and any selection, which is why it is a type of its
own rather than a `String`: a cursor that lived in the component would be lost
whenever the field was not declared for a frame.

`.value(read, on_change)` binds it. `read` returns the state the field shows,
and `on_change` receives a new one after every edit *and* every cursor
movement. Store it as it is. Without the binding the field paints empty, is not
focusable, and answers no events.

```rust
let empty = InputState::default();
let filled = InputState::new("Ada Lovelace"); // cursor at the end

filled.value();  // "Ada Lovelace"
filled.cursor(); // 12, a character index
```

An `InputState` is one line: `InputState::new` turns each line break in the
value into a space. To clear or replace the text — a form reset, loading a record —
assign a new state. There is no `PartialEq`; compare `.value()`.

## Submitting

Unmodified <kbd>Enter</kbd> emits `.on_submit(...)` and never inserts a line
break. Without `on_submit`, Enter bubbles — to a [Dialog](./dialog)'s default
action, for instance.

## Placeholder and title

`.placeholder(...)` is the muted text shown while the field is empty, focused
or not; a focused field shows its cursor in front of it. `.title(...)` is the
label on the border.

## Masking

`.mask_char('•')` paints every character as the mask, for a secret. Only the
paint changes: the state keeps the text as typed, and `value()` returns it.
Each character is one mask cell whatever its real width, so the mask does not
give away the shape of what it hides.

```rust
Input::new()
    .value(|state: &AppState| &state.password, Msg::Password)
    .title("Password")
    .mask_char('•')
```

## Invalid and disabled

`.invalid(true)` paints the text, the border, and the title in the theme's
destructive color. It is a look and nothing more: the field stays editable so
the user can fix it, and what counts as invalid is your decision, made from the
value you already own.

```rust
let email = state.email.value();
let invalid = !email.is_empty() && !plausible_email(email);

Input::new()
    .value(|state: &AppState| &state.email, Msg::Email)
    .title("Email")
    .invalid(invalid)
```

`.disabled(true)` mutes the field, hides the cursor, takes it out of Tab
traversal, and makes it ignore every event.

## Styling

Colors derive from the theme: the field well at rest, shifted for focus and
further for hover, the same ladder a [List](./list#styling)'s rows use.
Override one field with `.style(...)`. The closure receives the active theme
each render, so a style built from it follows theme switches:

```rust
use ratcn::InputStyle;

Input::new()
    .value(|state: &AppState| &state.name, Msg::Name)
    .style(|theme| {
        let mut style = InputStyle::from_theme(theme);
        style.cursor_background = theme.accent;
        style
    })
```

`InputStyle::fallback()` is the no-theme starting point: plain ANSI colors that
render on any terminal.

## Keyboard

Editing comes from [`ratatui-textarea`](https://crates.io/crates/ratatui-textarea),
so its readline-style keys apply:

| Keys | Does |
|---|---|
| `←` `→` &nbsp;`Ctrl+B` `Ctrl+F` | Move one character |
| `Ctrl+←` `Ctrl+→` &nbsp;`Alt+B` `Alt+F` | Move one word |
| `Home` `End` &nbsp;`Ctrl+A` `Ctrl+E` | Move to the start / end |
| `Shift` + a movement | Select |
| `Backspace` `Delete` &nbsp;`Ctrl+H` `Ctrl+D` | Delete one character |
| `Ctrl+W` `Alt+Backspace` &nbsp;/&nbsp; `Alt+D` `Alt+Delete` | Delete the word before / after |
| `Ctrl+K` | Delete to the end |
| `Ctrl+C` `Ctrl+X` `Ctrl+Y` | Copy, cut, and paste within the field |
| `Enter` | Submit |

`Ctrl+C`, `Ctrl+X`, and `Ctrl+Y` use the editor's own buffer, not the system
clipboard — and a host that quits on `Ctrl+C`, as the demos do, takes that key
before the field sees it.

Keys route by binding, not by effect. A key the editor binds is the field's
even where it changes nothing — <kbd>←</kbd> at the start of the text,
<kbd>Ctrl+K</kbd> at its end — and every unmodified letter, `j` and `k`
included, is typing. A chord the editor does not bind bubbles, which is how
`Ctrl+S` reaches your save handler through a focused field; that includes
<kbd>Ctrl+U</kbd> and <kbd>Ctrl+R</kbd>, undo and redo upstream, since a state
keeps no history.

The field also leaves these alone, whatever the editor binds them to:

- <kbd>Tab</kbd> and <kbd>Shift+Tab</kbd>, <kbd>Esc</kbd>, and the function
  keys.
- <kbd>↑</kbd>, <kbd>↓</kbd>, <kbd>Page Up</kbd>, <kbd>Page Down</kbd>, and the
  editor's chords for the same moves — <kbd>Ctrl+N</kbd>, <kbd>Ctrl+P</kbd>,
  <kbd>Ctrl+V</kbd>, <kbd>Alt+V</kbd>, and the <kbd>Alt</kbd> chords for
  paragraphs and the top and bottom: one line has no vertical movement to make.
- <kbd>Ctrl+J</kbd>, which is how a terminal reports a line feed, and which the
  editor would take as "delete to the start".

See [Keyboard](../concepts/keyboard) for the rules the other components follow.

## Mouse

A click focuses the field and places the cursor on the character clicked. The
cursor moves on the release, not on the press. A drag selects from the
character pressed to the one under the pointer, and keeps extending — scrolling
the text — while the pointer moves on past either end of the field. The wheel
is left to whatever encloses the field.

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## Paste

A paste is inserted at the cursor as one line: each line break and tab becomes
a space, and other control characters are dropped. Pastes only arrive as such
when the host asks for them — bracketed paste in a terminal, a paste listener
in the browser. See [Host integration](../concepts/host-integration). Without
it, a terminal delivers a paste as keystrokes, and a pasted line break is an
Enter.

## Paint-only widget

`InputWidget` draws a field without focus or events. It is an ordinary Ratatui
widget, so it works in a plain Ratatui app with no `Ratcn` runtime: it paints
the `InputState` it is given, scrolled to keep the cursor in view, and you
supply the interaction states.

```rust
use ratcn::InputWidget;

frame.render_widget(
    InputWidget::new(&state.name)
        .title("Name")
        .placeholder("Ada Lovelace")
        .focused(is_focused)
        .hovered(is_hovered)
        .themed(&theme),
    area,
);
```

`.mask_char(...)`, `.invalid(...)`, and `.disabled(...)` match the component's.
Replace `.themed(...)` with `.style(...)` to supply exact colors.

Driving the editing is then yours, and `ratcn::text_edit` holds the key
conversion. Only the editor that was painted knows the view — how far it is
scrolled — so paint with `.paint(...)`, which hands that editor back, edit it,
and store the result:

```rust
use ratcn::{InputState, text_edit::{Editor, editor_input}};

// In draw(), keep the editor the paint hands back, in an
// `Option<Editor<'static>>` of your own:
self.painted = Some(
    InputWidget::new(&self.name)
        .focused(true)
        .paint(area, frame.buffer_mut()),
);

// On a key the field takes, edit that editor and store the result:
if let (Some(mut editor), Some(input)) = (self.painted.take(), editor_input(&key)) {
    editor.input(input);
    self.name = InputState::from_editor(editor);
}
```

## Limits

- **No undo or redo.** A state is replaced on every keystroke, which is the
  wrong place to keep a history. An app that wants undo keeps its own stack of
  states.
- **No maximum length.** Check the value in `update` and keep the previous
  state if the new one is too long.
- **No double-click word selection**, and no system clipboard beyond the host's
  paste.
- Text fields currently build against a fork of `ratatui-textarea`, until
  upstream releases two fixes they depend on.

## Full API

Every method, with binding requirements and edge-case detail:
[`Input`](https://docs.rs/ratcn/latest/ratcn/struct.Input.html),
[`InputWidget`](https://docs.rs/ratcn/latest/ratcn/struct.InputWidget.html),
[`InputStyle`](https://docs.rs/ratcn/latest/ratcn/struct.InputStyle.html),
[`InputState`](https://docs.rs/ratcn/latest/ratcn/struct.InputState.html).

## See also

Use [TextArea](./textarea) when the text has more than one line.
