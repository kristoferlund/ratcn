---
description: "A multi-line text field for Ratatui apps: app-owned text, soft wrapping, a titled border and placeholder, Ctrl+Enter to submit, and click, drag, and wheel with the mouse."
---

# TextArea

A multi-line text field: the text in a well as tall as the area it is given, a
block cursor while focused, and an optional titled border around it. It wraps a
line longer than it is wide and scrolls to keep the cursor in view.

Built on [ratatui-textarea](https://docs.rs/ratatui-textarea/), which handles
editing, cursor movement, selection, wrapping, and scrolling. The library adds
theming and app-state binding.

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
Input, it fills the whole area it is declared in. The text runs from edge to
edge, inside the border when there is one.

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

`TextAreaState::new` splits lines at any line ending, keeps tabs, and drops
every other control character, as a paste does. To clear or replace the text,
assign a new state. There is no `PartialEq`; compare `.value()`.

## Submitting

<kbd>Enter</kbd> inserts a line break, so <kbd>Ctrl+Enter</kbd> is the chord
that emits `.on_submit(...)`, and so is <kbd>Ctrl+J</kbd>. Without `on_submit`
both bubble.

::: warning Ctrl+Enter needs a terminal that reports it
The terminal session ratcn sets up does not enable the kitty keyboard protocol, so most
terminals send <kbd>Ctrl+Enter</kbd> as a plain Enter, which inserts a line
break. Terminals that send a line feed instead deliver it as
<kbd>Ctrl+J</kbd>, which the field treats as the same submit chord. In the
browser it is the other way round: <kbd>Ctrl+Enter</kbd> arrives, and the
browser keeps <kbd>Ctrl+J</kbd> for itself. Give a form a second way to submit.
The demo has a Save button, and its help line names the chord that works on
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

Driving the editing is then yours. `ratcn::text_edit` holds the key conversion
and the editor's binding table, and `.paint(...)` paints the field and hands
back the editor it painted, which knows the view: how far the text is
scrolled, how tall a page is, and where lines wrap. Edit that editor and store
the result with `TextAreaState::from_editor`.
[`TextAreaWidget::paint`](https://docs.rs/ratcn/latest/ratcn/struct.TextAreaWidget.html#method.paint)
has a minimal key handler to start from.

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
| `Ctrl+K` | Delete to the end of the line; at its end, join the next line |
| `Ctrl+C` `Ctrl+X` | Copy / cut the selection to the clipboard |
| `Ctrl+Y` | Paste the field's last copy or cut |
| `Enter` | Insert a line break |
| `Ctrl+Enter` `Ctrl+J` | Submit |

`Ctrl+C` and `Ctrl+X` act only on a selection. With nothing selected they
bubble, so an app that quits on `Ctrl+C` still does with a field focused. In
the browser the platform's copy and cut chords arrive as events rather than
keys; see [Copy and paste](#copy-and-paste).

Every unmodified letter is typing, `j` and `k` included, and a key the editor
binds belongs to the field even where it changes nothing, such as `↑` on the
first line. Everything else bubbles to your app:

- chords the editor does not bind, such as `Ctrl+S` for your save handler, and
  `Ctrl+U` and `Ctrl+R`, since a state keeps no undo history;
- `Tab`, `Shift+Tab`, `Esc`, and the function keys. Tab moves focus; it does
  not indent.

See [Keyboard](../concepts/keyboard) for the rules the other components follow.

## Mouse

A click focuses the field and places the cursor on the character clicked,
when the button is released. A drag selects from the character pressed to the one under the pointer, across
lines, and keeps scrolling the text while the pointer moves on past an edge of
the field.

The wheel scrolls the text without needing focus, and carries the cursor along
to keep it in view. Once the text has no further to go, the wheel is left to
whatever encloses the field, so a form scrolls on from there.

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
those events rather than the keys. On a Mac, `Ctrl+C` and `Ctrl+X` stay
ordinary keys. A copy or cut also fills the field's own buffer, which `Ctrl+Y`
pastes back.

A paste is inserted at the cursor with its line breaks and tabs kept; every
other control character is dropped. Without bracketed paste, a terminal
delivers a paste as keystrokes.

The host carries the clipboard both ways. Pastes arrive as pastes only when it
asks for them: bracketed paste in a terminal, `BrowserClipboard` in the
browser. Copies go out when it writes what `Ratcn::take_clipboard` returns
after each event. See [Host integration](../concepts/host-integration#the-clipboard).

## Limits

- **No undo or redo.** A state is replaced on every keystroke, so an app that
  wants undo keeps its own stack of states.
- **No maximum length.** Check the value in `update` and keep the previous
  state if the new one is too long.
- **No double-click word selection.**
- **No copy in macOS Terminal.app or the VTE terminals** (GNOME Terminal,
  xfce4-terminal, Tilix), which ignore the OSC 52 sequence a terminal app
  writes the clipboard with. iTerm2 honors it only with its clipboard-access
  setting on, and tmux only with `set -g set-clipboard on`.
- **No in-app paste in a terminal.** Terminals do not let an app read the
  clipboard, so `Ctrl+V` cannot paste it; the terminal's own paste does.
- **No Linux primary selection** (select, then middle-click).
- **Cmd editing chords do nothing in the browser on a Mac** (`Cmd+←`, `Cmd+→`,
  `Cmd+Backspace` and the like). Use `Home`, `End`, and the `Ctrl` chords.
- **A page selection survives a click into the field in the browser.** It
  stays highlighted, but a copy in the field never copies it.
- **Safari is unverified.**
- **Ctrl+Enter submits only on a terminal that reports it**, and Ctrl+J not in
  the browser. See [Submitting](#submitting).
- **Clicks after a joined emoji** (such as 👩‍💻 or 👩🏽) may place the cursor
  away from the character clicked. Keyboard editing is unaffected.
- **Built against a fork of `ratatui-textarea`** until upstream releases the
  changes text fields depend on.

## Full API

See
[`TextArea`](https://docs.rs/ratcn/latest/ratcn/struct.TextArea.html),
[`TextAreaWidget`](https://docs.rs/ratcn/latest/ratcn/struct.TextAreaWidget.html),
[`TextAreaStyle`](https://docs.rs/ratcn/latest/ratcn/struct.TextAreaStyle.html),
and [`TextAreaState`](https://docs.rs/ratcn/latest/ratcn/struct.TextAreaState.html)
on docs.rs for the full API.

## See also

Use [Input](./input) for a single line: Enter submits there, and a paste is
flattened to one line.
