---
description: "A multi-line text field for Ratatui apps: app-owned text, soft wrapping, a titled border and placeholder, Ctrl+Enter to submit, and click, drag, and wheel with the mouse."
---

# TextArea

A multi-line text field: the text in a well as tall as the area it is given, a
block cursor while focused, and an optional titled border around it. It wraps a
line longer than it is wide and scrolls to keep the cursor in view.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 330px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p textarea</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/textarea-demo/index.html" title="ratcn textarea demo"></iframe>
  </div>
</div>

```rust
use ratcn::{TextArea, TextAreaState};

#[derive(Default)]
struct AppState {
    notes: TextAreaState,
}

enum Msg {
    Notes(TextAreaState),
    Save,
}

// In draw(), declare the field:
ctx.component(
    "notes",
    TextArea::new()
        .value(|state: &AppState| &state.notes, Msg::Notes)
        .title("Notes")
        .placeholder("What happened today?")
        .on_submit(|| Msg::Save),
    area,
);

// In update(), store what it emits:
Msg::Notes(notes) => state.notes = notes,
```

It is the multi-line sibling of [Input](./input), bound the same way. Unlike an
Input, it fills the whole area it is declared in. The text sits one cell in from
each side of the well, and a click on that cell still lands on the field.

## State

The text lives in your state as a `TextAreaState`, not in the component. It
holds the lines, the cursor, and any selection, which is why it is a type of
its own rather than a `String`.

`.value(read, on_change)` binds it. `read` returns the state the field shows,
and `on_change` receives a new one after every edit, every cursor movement, and
every scroll of the wheel. Store it as it is. Without the binding the field
paints empty, is not focusable, and answers no events.

```rust
let empty = TextAreaState::default();
let filled = TextAreaState::new("Met Ada today.\nShe counts."); // cursor at the end

filled.value();  // "Met Ada today.\nShe counts.", lines joined with \n
filled.lines();  // ["Met Ada today.", "She counts."]
filled.cursor(); // (1, 11): the line, then the character within it
```

`TextAreaState::new` splits at `\n`, `\r\n`, and `\r`, keeps tabs, and drops
every other control character, as a paste does. To clear or replace the text,
assign a new state. There is no `PartialEq`; compare `.value()`.

## Submitting

<kbd>Enter</kbd> inserts a line break, so <kbd>Ctrl+Enter</kbd> is the chord
that emits `.on_submit(...)`, and so is <kbd>Ctrl+J</kbd>. Without `on_submit`
both bubble.

::: warning Ctrl+Enter needs a terminal that reports it
ratcn's terminal session does not enable the kitty keyboard protocol, so most
terminals send <kbd>Ctrl+Enter</kbd> as a plain Enter, which inserts a line
break. Terminals that send a line feed instead deliver it as
<kbd>Ctrl+J</kbd>, which the field treats as the same submit chord. In the
browser it is the other way round: <kbd>Ctrl+Enter</kbd> arrives, and the
browser keeps <kbd>Ctrl+J</kbd> for itself. Give a form a second way to submit
— the demo has a Save button, and its help line names the chord that works on
each host.
:::

## Placeholder and title

`.placeholder(...)` is the muted text shown while the field is empty, focused
or not, where the text would start; a focused field shows its cursor on its
first character. `.title(...)` draws a border with the title on it, which takes
a row and a column on each side from the text.

## Wrapping

A line longer than the field breaks over several rows, at a word boundary
where there is one and inside a word that is wider than the field.
<kbd>↑</kbd> and <kbd>↓</kbd> then move by the rows on screen. Wrapping is
paint only: the state's lines stay as typed.

`.wrap_mode(WrapMode::None)` keeps one row per line and scrolls sideways to the
cursor instead:

```rust
use ratcn::text_edit::WrapMode;

TextArea::new()
    .value(|state: &AppState| &state.notes, Msg::Notes)
    .wrap_mode(WrapMode::None)
```

## Invalid and disabled

`.invalid(true)` paints the text, the border, and the title in the theme's
destructive color. It is a look and nothing more: the field stays editable, and
what counts as invalid is your decision.

`.disabled(true)` mutes the field, hides the cursor, takes it out of Tab
traversal, and makes it ignore every event.

## Styling

Colors derive from the theme, exactly as an [Input](./input#styling)'s do.
Override one field with `.style(...)`. The closure receives the active theme
each render, so a style built from it follows theme switches:

```rust
use ratcn::TextAreaStyle;

TextArea::new()
    .value(|state: &AppState| &state.notes, Msg::Notes)
    .style(|theme| {
        let mut style = TextAreaStyle::from_theme(theme);
        style.selection_background = theme.accent;
        style
    })
```

`TextAreaStyle::fallback()` is the no-theme starting point: plain ANSI colors
that render on any terminal.

## Keyboard

Editing comes from [`ratatui-textarea`](https://crates.io/crates/ratatui-textarea),
so its readline-style keys apply:

| Keys | Does |
|---|---|
| `←` `→` `↑` `↓` &nbsp;`Ctrl+B` `Ctrl+F` `Ctrl+P` `Ctrl+N` | Move one character or one row |
| `Ctrl+←` `Ctrl+→` &nbsp;`Alt+B` `Alt+F` | Move one word |
| `Home` `End` &nbsp;`Ctrl+A` `Ctrl+E` | Move to the start / end of the line |
| `Ctrl+↑` `Ctrl+↓` | Move one paragraph |
| `Alt+<` `Alt+>` | Move to the top / bottom of the text |
| `Page Up` `Page Down` &nbsp;`Alt+V` `Ctrl+V` | Move one page up / down |
| `Shift` + a movement | Select |
| `Backspace` `Delete` &nbsp;`Ctrl+H` `Ctrl+D` | Delete one character |
| `Ctrl+W` `Alt+Backspace` `Alt+H` &nbsp;/&nbsp; `Alt+D` `Alt+Delete` | Delete the word before / after |
| `Ctrl+K` | Delete to the end of the line |
| `Ctrl+C` `Ctrl+X` `Ctrl+Y` | Copy, cut, and paste within the field |
| `Enter` | Insert a line break |
| `Ctrl+Enter` `Ctrl+J` | Submit |

`Ctrl+C`, `Ctrl+X`, and `Ctrl+Y` use the editor's own buffer, not the system
clipboard — and a host that quits on `Ctrl+C`, as the demos do, takes that key
before the field sees it.

Keys route by binding, not by effect. A key the editor binds is the field's
even where it changes nothing — <kbd>↑</kbd> on the first line,
<kbd>Ctrl+K</kbd> at the end of a line — and every unmodified letter, `j` and
`k` included, is typing. A chord the editor does not bind bubbles, which is how
`Ctrl+S` reaches your save handler through a focused field; that includes
<kbd>Ctrl+U</kbd> and <kbd>Ctrl+R</kbd>, undo and redo upstream, since a state
keeps no history.

The field also leaves <kbd>Tab</kbd> and <kbd>Shift+Tab</kbd>, <kbd>Esc</kbd>,
and the function keys alone, so they reach focus traversal, an enclosing
dialog, and your app. Tab moves focus; it does not indent. See
[Keyboard](../concepts/keyboard) for the rules the other components follow.

## Mouse

A click focuses the field and places the cursor on the character clicked,
counting wrapped rows and scrolled lines as they are on screen. The cursor
moves on the release, not on the press. A drag selects from the character
pressed to the one under the pointer, across lines, and keeps extending —
scrolling the text — while the pointer moves on past an edge of the field.

The wheel scrolls the text three rows a notch and needs no focus. The cursor
has to stay in view, so scrolling it off the edge moves it along with the text.
Once the text has no further to go the wheel is left to whatever encloses the
field, so a form scrolls on from there.

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## Paste

A paste is inserted at the cursor with its line breaks and tabs kept, whichever
line ending the terminal sent; every other control character is dropped. Pastes
only arrive as such when the host asks for them — bracketed paste in a
terminal, a paste listener in the browser. See
[Host integration](../concepts/host-integration). Without it, a terminal
delivers a paste as keystrokes.

## Paint-only widget

`TextAreaWidget` draws a field without focus or events. It is an ordinary
Ratatui widget, so it works in a plain Ratatui app with no `Ratcn` runtime: it
paints the `TextAreaState` it is given over the whole area, scrolled to keep
the cursor in view, and you supply the interaction states.

```rust
use ratcn::TextAreaWidget;

frame.render_widget(
    TextAreaWidget::new(&state.notes)
        .title("Notes")
        .placeholder("What happened today?")
        .focused(is_focused)
        .hovered(is_hovered)
        .themed(&theme),
    area,
);
```

`.wrap_mode(...)`, `.invalid(...)`, and `.disabled(...)` match the component's.
Replace `.themed(...)` with `.style(...)` to supply exact colors.

Driving the editing is then yours, and `ratcn::text_edit` holds the key
conversion and the editor's binding table. Only the editor that was painted
knows the view — how far it is scrolled, how tall a page is, and where lines
wrap — so paint with `.paint(...)`, which hands that editor back, edit it, and
store the result. Which keys reach the editor is your policy; this is the
least of one:

```rust
use ratcn::{
    TextAreaState,
    runtime::KeyCode,
    text_edit::{Editor, editor_input, is_editor_binding},
};

// Beside the state, the editor the last paint handed back, with the version
// of the state it was painted from:
painted: Option<(u64, Editor<'static>)>,

// In draw():
let editor = TextAreaWidget::new(&self.notes)
    .focused(true)
    .paint(area, frame.buffer_mut());
self.painted = Some((self.notes.version(), editor));

// On a key:
match key.code {
    // Enter is a line break, so submitting takes a chord, shifted or not;
    // Ctrl+J is how a terminal that sends a line feed reports Ctrl+Enter.
    KeyCode::Enter | KeyCode::Char('j' | 'J') if key.modifiers.ctrl => self.save(),
    // Focus traversal and the enclosing view.
    KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc => {}
    _ => {
        if let Some(input) = editor_input(&key).filter(is_editor_binding) {
            // The painted editor, while the state is still the one it was
            // painted from.
            let mut editor = match self.painted.take() {
                Some((version, editor)) if version == self.notes.version() => editor,
                _ => self.notes.editor().clone(),
            };
            editor.input(input);
            self.notes = TextAreaState::from_editor(editor);
        }
    }
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
- **Ctrl+Enter** submits only on a terminal that reports it, and **Ctrl+J**
  not in the browser, as above.
- Text fields currently build against a fork of `ratatui-textarea`, until
  upstream releases the changes they depend on.

## Full API

Every method, with binding requirements and edge-case detail:
[`TextArea`](https://docs.rs/ratcn/latest/ratcn/struct.TextArea.html),
[`TextAreaWidget`](https://docs.rs/ratcn/latest/ratcn/struct.TextAreaWidget.html),
[`TextAreaStyle`](https://docs.rs/ratcn/latest/ratcn/struct.TextAreaStyle.html),
[`TextAreaState`](https://docs.rs/ratcn/latest/ratcn/struct.TextAreaState.html).

## See also

Use [Input](./input) for a single line: Enter submits there, and a paste is
flattened to one line.
