---
description: "A single-line text field for Ratatui apps: app-owned text, a titled border, placeholder, masking for secrets, an invalid look, and click-to-place and drag-to-select with the mouse."
---

# Input

A single-line text field: the value in a well, a block cursor while focused,
and an optional titled border around it. Typing, selection, and scrolling a
value longer than the field are handled; what the text means is yours.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 400px">
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
it, and the field is then three. `.height()` answers which, for a layout
constraint. Either way it sits at the top of the area it is declared in and
ignores rows below that. The text runs from edge to edge of the field, inside
the border when there is one.

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

An `InputState` is one line: `InputState::new` turns each line break and tab
in the value into a space and drops every other control character, as a paste
does. To clear or replace the text — a form reset, loading a record —
assign a new state. There is no `PartialEq`; compare `.value()`.

## Submitting

Unmodified <kbd>Enter</kbd> emits `.on_submit(...)` and never inserts a line
break. Without `on_submit`, Enter bubbles — to a [Dialog](./dialog)'s default
action, for instance.

## Placeholder and title

`.placeholder(...)` is the muted text shown while the field is empty, focused
or not, where the text would start; a focused field shows its cursor on its
first character. `.title(...)` is the label on the border.

## Prefix and suffix

`.prefix(...)` and `.suffix(...)` paint muted text inside the field, before
and after the editable text: `https://` before a URL, an icon before a search,
a unit after a number. Each takes a string or a styled `Line`, whose own
styles paint over the muted one. The text keeps a cell of space from each,
and a click on one lands at that end of the text. They are never masked.

```rust
Input::new()
    .value(|state: &AppState| &state.site, Msg::Site)
    .prefix("https://")
    .suffix(".com")
```

They are text only. A button belongs beside the field, not inside it.

## Masking

`.mask_char('•')` paints every character as the mask, for a secret. Only the
paint changes: the state keeps the text as typed, and `value()` returns it.
Each character is one mask cell whatever its real width, so the mask does not
give away the shape of what it hides. A masked field never copies or cuts, as
a browser's password field does not, so a `Ctrl+C` there quits like any other
that copied nothing. A paste still goes in.

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
| `Ctrl+W` `Alt+Backspace` `Alt+H` &nbsp;/&nbsp; `Alt+D` `Alt+Delete` | Delete the word before / after |
| `Ctrl+K` | Delete to the end |
| `Ctrl+C` `Ctrl+X` | Copy / cut the selection to the clipboard |
| `Ctrl+Y` | Paste the field's last copy or cut |
| `Enter` | Submit |

In a terminal, `Ctrl+C` and `Ctrl+X` act only on a selection; with nothing
selected they bubble, so an app that quits on `Ctrl+C` still does with a field
focused. In the browser, `Cmd+C`/`Cmd+X` on a Mac and `Ctrl+C`/`Ctrl+X`
elsewhere arrive as `Event::Copy` and `Event::Cut`, not as keys, and so does
`Shift+Delete` off a Mac; on a Mac, `Ctrl+C` and `Ctrl+X` are still keys. See [Copy and paste](#copy-and-paste).

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
- <kbd>Ctrl+M</kbd> and a raw line break, which would split the line, and
  <kbd>Ctrl+J</kbd>, which is how a terminal reports a line feed, and which the
  editor would take as "delete to the start".

See [Keyboard](../concepts/keyboard) for the rules the other components follow.

## Mouse

A click focuses the field and places the cursor on the character clicked. The
cursor moves on the release, not on the press. A drag selects from the
character pressed to the one under the pointer, and keeps extending — scrolling
the text — while the pointer moves on past either end of the field. The wheel
is left to whatever encloses the field.

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## Copy and paste

Copy and cut write the selection to the system clipboard, and a paste comes in
from it, on the keys each platform's users expect:

| Where | Copy | Cut | Paste |
|---|---|---|---|
| Browser, Mac | `Cmd+C` | `Cmd+X` | `Cmd+V` |
| Browser, Linux and Windows | `Ctrl+C` | `Ctrl+X`, `Shift+Delete` | `Ctrl+V` |
| Terminal, any OS | `Ctrl+C` on a selection | `Ctrl+X` on a selection | the terminal's own: `Cmd+V` (Mac), `Ctrl+Shift+V` (Linux), `Ctrl+V` (Windows Terminal) |

In a terminal, `Cmd+C` and `Ctrl+Shift+C` belong to the terminal, which copies
its own selection. In the browser the platform's chords arrive as
`Event::Copy`, `Event::Cut`, and `Event::Paste`, never as keys, so an app binds
those events rather than the keys. A copy keeps the selection; a cut removes
it. A copy or cut also fills the field's own buffer, so `Ctrl+Y`
pastes it back.

A paste is inserted at the cursor as one line: each line break and tab becomes
a space, and other control characters are dropped. Without bracketed paste, a
terminal delivers a paste as keystrokes, and a pasted line break is an Enter.

The host carries the clipboard both ways. Pastes only arrive as such when it
asks for them: bracketed paste in a terminal, `BrowserClipboard` in the
browser. Copies go out when it writes what `Ratcn::take_clipboard` returns
after each event. See [Host integration](../concepts/host-integration#the-clipboard).

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

`.prefix(...)`, `.suffix(...)`, `.mask_char(...)`, `.invalid(...)`, and
`.disabled(...)` match the component's.
Replace `.themed(...)` with `.style(...)` to supply exact colors.

Driving the editing is then yours, and `ratcn::text_edit` holds the key
conversion and the editor's binding table. Only the editor that was painted
knows the view — how far it is scrolled — so paint with `.paint(...)`, which
hands that editor back, edit it, and store the result. Which keys reach the
editor is your policy; this is the least of one:

```rust
use ratcn::{
    InputState,
    runtime::KeyCode,
    text_edit::{Editor, editor_input, is_editor_binding},
};

// Beside the state, the editor the last paint handed back, with the version
// of the state it was painted from:
painted: Option<(u64, Editor<'static>)>,

// In draw():
let editor = InputWidget::new(&self.name)
    .focused(true)
    .paint(area, frame.buffer_mut());
self.painted = Some((self.name.version(), editor));

// On a key:
match key.code {
    KeyCode::Enter => self.submit(),
    // Focus traversal, the enclosing view, and keys with nowhere to go on
    // one line.
    KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc | KeyCode::Up | KeyCode::Down => {}
    // Line breaks, shifted or not: Ctrl+J is a terminal's line feed.
    KeyCode::Char('j' | 'J' | 'm' | 'M') if key.modifiers.ctrl => {}
    KeyCode::Char('\n' | '\r') => {}
    _ => {
        if let Some(input) = editor_input(&key).filter(is_editor_binding) {
            // The painted editor, while the state is still the one it was
            // painted from.
            let mut editor = match self.painted.take() {
                Some((version, editor)) if version == self.name.version() => editor,
                _ => self.name.editor().clone(),
            };
            editor.input(input);
            self.name = InputState::from_editor(editor);
        }
    }
}
```

An editor that does end up with a second line — a key that split it — comes
back from `InputState::from_editor` as one, its lines joined with spaces.

## Limits

- **No undo or redo.** A state is replaced on every keystroke, which is the
  wrong place to keep a history. An app that wants undo keeps its own stack of
  states.
- **No maximum length.** Check the value in `update` and keep the previous
  state if the new one is too long.
- **No double-click word selection.**
- **No copy in macOS Terminal.app or the VTE terminals** (GNOME Terminal,
  xfce4-terminal, Tilix). A terminal app writes the clipboard with the OSC 52
  escape sequence, which they ignore. iTerm2 honors it only with its
  clipboard-access setting on, and tmux only with `set -g set-clipboard on`.
- **No in-app paste in a terminal.** Terminals do not let an app read the
  clipboard, so `Ctrl+V` cannot paste it; the terminal's own paste does.
- **No Linux primary selection** (select, then middle-click).
- **Cmd editing chords do nothing in the browser on a Mac** — `Cmd+←`, `Cmd+→`,
  `Cmd+Backspace` and the like. Use `Home`, `End`, and the `Ctrl` chords.
- **A page selection survives a click into the field in the browser.** It
  stays highlighted, but a copy in the field never copies it.
- **Safari is unverified.**
- **Clicks after a joined emoji** (such as 👩‍💻 or 👩🏽) on a line may place the
  cursor off from the character clicked. Keyboard editing is unaffected.
- Text fields currently build against a fork of `ratatui-textarea`, until
  upstream releases the changes they depend on.

## Full API

Every method, with binding requirements and edge-case detail:
[`Input`](https://docs.rs/ratcn/latest/ratcn/struct.Input.html),
[`InputWidget`](https://docs.rs/ratcn/latest/ratcn/struct.InputWidget.html),
[`InputStyle`](https://docs.rs/ratcn/latest/ratcn/struct.InputStyle.html),
[`InputState`](https://docs.rs/ratcn/latest/ratcn/struct.InputState.html).

## See also

Use [TextArea](./textarea) when the text has more than one line.
