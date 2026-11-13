---
description: "Wiring the ratcn runtime into a host loop: a terminal Session that opens and restores the terminal and sets the pointer, progress, title, program status, and clipboard; the event loop; a crossterm or browser host."
---

# Host integration

The host owns the application loop: redraw policy, time, terminal setup and
restoration, backend listeners, and global shortcuts. The runtime declares and
paints a frame through `render`, and routes normalized input through
`handle_event`.

Async work follows the same boundary. The host executes app-defined effects and
sends their completion messages back through a queue: a native loop wakes or
polls while work is pending so it can drain the queue and redraw, and a browser
draw callback drains it before painting the next frame. The
[effects example](./state-and-messages#effects-and-result-messages) shows both
forms.

This page covers three hosts: a terminal opened by ratcn's own `Session`, an
app on crossterm that owns its terminal, and a browser app on Ratzilla. It ends
with the clipboard, which each host carries out in its own way.

## A terminal session

`ratcn::terminal::Session` is the terminal half of the host. It opens the
terminal (raw mode, the alternate screen, the input modes the app asked for)
and puts every one of them back on the way out. It comes with the `termina`
feature:

```sh
cargo add ratcn --features termina
cargo add ratatui --no-default-features --features layout-cache,std
```

The feature re-exports termina as `ratcn::terminal::termina`, so its types, such
as the `termina::Event` inside `SessionEvent::Input`, are named through ratcn at
the version ratcn already builds against. The session's backend covers
Ratatui's default features; keep `scrolling-regions` off.

### What a session does

Automatically:

- **[Synchronized output](#synchronized-output):** each frame reaches the
  screen whole.
- **[Color depth](#color-depth):** frames use the colors the terminal has.
- **[Restoring the terminal](#restoring-the-terminal):** everything the
  session changed goes back on exit, including after a panic.

On request:

| Method | Sets | Details |
|---|---|---|
| `set_pointer_shape` | The mouse pointer shape (OSC 22) | [Pointer shape](./mouse#pointer-shape) |
| `set_progress` | The terminal's progress indicator (OSC 9;4) | [Progress in the terminal](../components/progress#progress-in-the-terminal) |
| `set_title` | The window and tab title (OSC 2) | [The window title](#the-window-title) |
| `set_program_status` | What the app is doing (OSC 7501) | [Program status](#program-status) |
| `set_clipboard` | The system clipboard (OSC 52) | [The terminal clipboard](#the-terminal-clipboard) |

Call the first four after each frame; each writes only when its value changes.
Call `set_clipboard` when the app copies.

### Options

`SessionOptions::new()` opens the alternate screen, and each builder adds a
mode:

- `.mouse()` reports movement, clicks, and scrolling as events, which every part
  of ratcn's mouse handling needs. While it is on, the terminal's own text
  selection usually stops working.
- `.paste()` delivers a whole paste as one `SessionEvent::Input`. Converted with
  `Event::try_from`, it becomes one `Event::Paste`, so a pasted newline stays
  distinct from the user hitting Enter.
- `.adaptive()` follows the terminal's colors; see
  [Opening adaptively](#opening-adaptively).

### Synchronized output

Each frame drawn through `session.terminal_mut()` goes out as one synchronized
update (mode 2026): the terminal shows it once it has arrived whole, so large
redraws and resizes stay clean. A frame left open by a failed draw, or by
`clear` on its own, closes at the next `session.next`.

### Color depth

Each frame is drawn in the colors the terminal has: 24-bit, the 256-color
palette, or the 16 named colors. With 16, each theme color keeps its hue, so a
destructive red stays red. The session reads the depth from `COLORTERM` and
`TERM`, and when they leave it open, asks the terminal while opening (one round
trip). Over SSH, which drops `COLORTERM`, a terminal that stays silent gets 256
colors; set `COLORTERM=truecolor` to use 24-bit.

### Opening with a preset theme

Choose a preset and paint every frame with it:

```rust
use ratcn::{Theme, terminal::{Session, SessionOptions}};

let mut session = Session::open(SessionOptions::new().mouse())?;
let theme = Theme::gruvbox();

loop {
    session.terminal_mut().draw(|frame| app.draw(frame, &theme))?;
    let _event = session.next(None)?;
}
```

`terminal_mut` hands back an ordinary Ratatui `Terminal`, so drawing is the
drawing you already write.

### Opening adaptively

`.adaptive()` makes a session follow the terminal. It asks the terminal what
colors it uses while opening, subscribes to the changes it reports, and asks
again when the window regains focus, shortly after every change signal, and when
input resumes after a pause. That way a change still reaches the app when the
terminal was recolored from outside, or has no change notification to send.

`session.theme()` is the theme the terminal's answer becomes, falling back to
`Theme::default_dark()` where the terminal keeps quiet.
`session.theme_with_fallback(fallback)` uses a preset of the app's own as that
fallback. Read it each frame and paint from it: the loop redraws after every
`next`, so the frame after a change wears it.

```rust
use ratcn::{Theme, terminal::{Session, SessionEvent, SessionOptions}};

let mut session = Session::open(SessionOptions::new().mouse().adaptive())?;

loop {
    let theme = session.theme_with_fallback(Theme::gruvbox());
    session.terminal_mut().draw(|frame| app.draw(frame, &theme))?;

    match session.next(None)? {
        Some(SessionEvent::Input(event)) => app.handle_event(event),
        Some(SessionEvent::ThemeChanged(theme)) => app.remember_theme(theme),
        None => {}
    }
}
```

The `ThemeChanged` arm is where an app acts on the change itself: persist the
user's choice, or animate the transition.

### Restoring the terminal

Dropping the `Session` switches every mode it turned on back off, in the reverse
of the order it turned them on, and shows the cursor again. That happens on
every path out: a `break`, a `?`, and an unwinding panic, which the session
installs a hook for. It also gives back what the app changed outside the grid:
the pointer shape, the progress indicator, the window title, and the program
status.

## The event loop

`session.next(timeout)` waits at most `timeout`, or indefinitely when it is
`None`, and answers with the next thing worth telling the app about. `Ok(None)`
means the wait ran out with nothing to report, which is how state that changes
with time gets its frame. Toast expiry is the common case:

```rust
loop {
    let now = app_time();
    app.state.toasts.prune_expired(now);
    let theme = session.theme();
    session.terminal_mut().draw(|frame| app.draw(frame, &theme, now))?;
    session.set_pointer_shape(app.ratcn.pointer_shape())?;

    let timeout = app.state.toasts.time_until_next_expiry(app_time());
    let Some(event) = session.next(timeout)? else {
        // The deadline arrived. Loop back to prune and redraw.
        continue;
    };

    if let SessionEvent::Input(event) = event {
        let quit = is_quit(&event);
        app.handle_event(event);
        // A text field copies its selection on Ctrl+C; only a Ctrl+C that
        // copied nothing quits.
        match app.ratcn.take_clipboard() {
            Some(text) => session.set_clipboard(&text)?,
            None if quit => break,
            None => {}
        }
    }
}
```

When nothing is time-dependent, pass `None` and let the wait block. The line
after the draw shows the frame's [pointer shape](./mouse#pointer-shape), and the
`take_clipboard` match carries out [the clipboard](#the-clipboard) write.

A `termina::Event` goes straight to `Ratcn::handle_event`, through a `TryFrom`
conversion the runtime provides. Resize, terminal focus, and key releases come
back as `EventResult::Ignored` for the host to act on. For global shortcuts, see
[App shortcuts](./rendering-and-events#app-shortcuts).

### The window title

`session.set_title` sets the terminal's window and tab title. Hand it the app's
title after each frame, like the pointer shape; it writes only when the title
changes:

```rust
session.set_title(&format!("notes — {}", app.state.file_name))?;
```

Control characters are left out, so a file name cannot break the sequence.
Before the first title, the session saves the user's own on the terminal's
title stack, and puts it back on exit. Terminals without the stack keep the
app's title until something else sets one, usually the shell's next prompt.
The tabs-basic demo shows the committed tab in the title.

### Program status

`session.set_program_status` tells the terminal what the app is doing, with the
OSC 7501 [Program Status Protocol](https://www.superlogical.com/rex/docs/build/program-status):
idle, working (with optional progress), blocked on the user, done, or failed,
with a message. The terminal decides how to show it: a tab glyph, a session
list, a notification.

```rust
use ratcn::terminal::{ProgramState, ProgramStatus};

let status = ProgramStatus::new(ProgramState::Working { progress: Some(40) })
    .app("deploy")
    .message("Pushing image");
session.set_program_status(&status)?;
```

Hand it the app's status after each frame; a report the terminal already shows
is not written again. Tasks of their own get records of their own with
`.id("build/test")`, and `clear_program_status("build/test")` removes one.

The terminal keeps the records after the app has gone: working and blocked
ones until the shell exits or prompts again, done and error until the user has
seen them. So on exit the session takes back what is still in progress, and an
app that wants to leave a result behind reports `Done` or `Error` before
quitting. Report `Idle` when the user cancels.

A title or message is made to fit: control characters become spaces, and text
past the protocol's limit is cut. An `id` or `app` the terminal would drop
returns `InvalidInput`.

| Terminal | Program status |
|---|---|
| Rex | The reference implementation |
| kitty | From 0.50.0, in the tab bar |
| Ghostty | Parsed, not shown yet |
| tmux | Not forwarded |

It complements
[the terminal's progress indicator](../components/progress#progress-in-the-terminal),
which more terminals show today; an app can send both. The `program-status`
demo does.

## Apps on a crossterm backend

A host that runs on crossterm and owns its own terminal lifecycle uses
`ratcn::crossterm::InputModes` to switch mouse capture and bracketed paste on.
It hands back a guard that switches them off when dropped.

```sh
cargo add ratcn --features crossterm
cargo add ratatui --no-default-features --features layout-cache,std,crossterm
```

```rust
let _input_modes = ratcn::crossterm::InputModes::new()
    .mouse()
    .paste()
    .enable()?;
```

Bind the guard to a name, as above: the modes stay on for as long as it lives.
The host owns raw mode and the alternate screen. See [Mouse input](./mouse) for
mouse capture.

## In the browser

Enable the `ratzilla` feature for Ratzilla key and mouse conversions, the
browser input and clipboard listeners, and the browser pointer shape:

```sh
cargo add ratcn --features ratzilla
cargo add ratatui --no-default-features --features layout-cache,std
cargo add ratzilla
```

The browser-only API (`BrowserInput`, `BrowserClipboard`,
`set_browser_pointer_shape`) exists
only on `wasm32`. Read its rustdoc with
`cargo doc -p ratcn --features ratzilla --target wasm32-unknown-unknown`.

### Routing events

Ratzilla drives callbacks, so shared mutable app state normally lives in an
`Rc<RefCell<App>>`. Its key and mouse callbacks hand you Ratzilla events, so the
app's `handle_event` takes anything that converts to an `Event`, applies an
`Emit`, and returns the result:

```rust
impl App {
    fn handle_event(&mut self, event: impl TryInto<Event>) -> EventResult<Msg> {
        let Ok(event) = event.try_into() else {
            return EventResult::Ignored;
        };
        let result = self.ratcn.handle_event(event, &self.state);
        if let EventResult::Emit(msg) = &result {
            self.update(msg.clone());
        }
        result
    }
}

let app = Rc::new(RefCell::new(App::new()));

terminal.on_key_event({
    let app = Rc::clone(&app);
    move |event| app.borrow_mut().handle_event(event)
}).map_err(|error| io::Error::other(error.to_string()))?;
```

Wire mouse callbacks the same way; see [Mouse input](./mouse#in-the-browser).
An app with text fields routes both through the `route` function in
[The browser clipboard](#the-browser-clipboard) instead, so a copy reaches the
clipboard.

Ratzilla misses some input. It reports a key that types a non-ASCII character
(`ä`, `é`, `€`) as unidentified, so a text field cannot type it, and it reports
no wheel. `ratcn::runtime::BrowserInput` adds both. Install it with the app's
element, chosen as for [the browser clipboard](#the-browser-clipboard), and a
closure that routes an event and returns whether the app took it. Route
Ratzilla's mouse events through its `mouse_handler`, which keeps the pointer's
cell for the wheel:

```rust
let input = BrowserInput::install(&element, {
    let app = Rc::clone(&app);
    move |event| !matches!(app.borrow_mut().handle_event(event), EventResult::Ignored)
})?;
terminal
    .on_mouse_event(input.mouse_handler())
    .map_err(|error| io::Error::other(error.to_string()))?;
// `main` returns once the app is running; the listeners have to outlive it.
std::mem::forget(input);
```

The wheel scrolls once a notch, at the pointer's cell. A wheel the app takes
does not also scroll the page, and `Ctrl`+wheel stays the browser's zoom. An app
that writes the clipboard writes it from this closure too, as `route` in
[the browser clipboard](#the-browser-clipboard) does.

Ratzilla's canvas takes focus but leaves Tab to the browser, which moves focus
off the canvas: keys, and the clipboard chords with them, then go to the page.
An app that uses Tab for its own focus traversal stops Tab's default while the
canvas has focus, as the demos' `demos/shared/ratatui-keyboard-capture.js`
does.

A character composed with Option on a Mac, or with AltGr elsewhere, arrives
with Alt (or Ctrl+Alt) held: `@` from Option+2 on a Swedish Mac, `€` from
AltGr+E. A text field takes Alt with a character as a chord, so the demos'
script forwards a composed character without those modifiers, and matches
only the page's own Alt hotkeys by physical key, which macOS would otherwise
compose into another character. Off a Mac, Alt alone is always a chord. Text
wins over a hotkey: where Option types punctuation, `@` from Option+2 on a
Swedish Mac, a field types it rather than the hotkey firing.

### Drawing

`draw_web` renders on every animation frame. After the app draws,
`set_browser_pointer_shape` shows the frame's
[pointer shape](./mouse#pointer-shape) as the CSS `cursor` of the element the
app is drawn in:

```rust
use ratcn::runtime::set_browser_pointer_shape;
use ratzilla::web_sys::{self, wasm_bindgen::JsCast};

// The element the app is drawn in, whose CSS cursor shows its pointer shape:
// here the container given to the backend as `grid_id`.
let element: web_sys::HtmlElement = web_sys::window()
    .and_then(|window| window.document())
    .and_then(|document| document.get_element_by_id("app"))
    .and_then(|element| element.dyn_into().ok())
    .ok_or_else(|| io::Error::other("no #app element"))?;
terminal.draw_web(move |frame| {
    app.borrow_mut().draw(frame);
    set_browser_pointer_shape(&element, app.borrow().ratcn.pointer_shape());
});
```

Time-based cleanup, including toast pruning, belongs in the draw callback or
another host callback that can cause a frame.

A host that draws only the frames it needs can keep the terminal and call
`Terminal::draw` itself. It requests one animation frame when an event routes to
something (any `EventResult` but `Ignored`) and one when a deadline the app
named passes. Motion is always at least `Consumed` once a surface exists, so
hover stays live under that rule. The demos run on such a host, in
`demos/shared`.

## The clipboard

A component writes the system clipboard with `EventCtx::set_clipboard`, as
[Input](../components/input#copy-and-paste) and
[TextArea](../components/textarea#copy-and-paste) do on a copy or cut. The
runtime keeps the latest write, and the host takes it with
`Ratcn::take_clipboard` and carries it out after each event, whatever the
event's result.

### The terminal clipboard

`Session::set_clipboard` writes the text with the OSC 52 escape sequence, which
works over SSH too. Most terminals honor it. iTerm2 needs its clipboard-access
setting, and tmux needs `set -g set-clipboard on`. macOS Terminal.app and the
VTE terminals (GNOME Terminal, xfce4-terminal, Tilix) ignore it.

A field copies on `Ctrl+C` only when it has a selection, and lets the key bubble
otherwise. So route `Ctrl+C` before treating it as quit, and quit only when the
event put nothing on the clipboard. Checking for `Ignored` is not enough: an
open modal consumes every key. Here `app` is your app, holding its `Ratcn` as
`ratcn`, and `is_quit` says whether an event is `Ctrl+C`
([the event loop](#the-event-loop) does the same):

```rust
let quit = is_quit(&event);
app.handle_event(event);
match app.ratcn.take_clipboard() {
    Some(text) => session.set_clipboard(&text)?,
    None if quit => return Ok(()),
    None => {}
}
```

An app on `ratcn::crossterm` writes the text with crossterm's
`CopyToClipboard` instead, behind its `osc52` feature. Add the crossterm that
ratatui uses (`cargo add crossterm@0.29 --features osc52`) and replace the
`session.set_clipboard` arm:

```rust
Some(text) => execute!(io::stdout(), CopyToClipboard::to_clipboard_from(text))?,
```

### The browser clipboard

Ratzilla has no clipboard callback. Instead,
`ratcn::runtime::BrowserClipboard` listens for the document's `paste`, `copy`,
and `cut` events and hands them to your first closure as `Event::Paste`,
`Event::Copy`, and `Event::Cut`; the closure returns whether the app took the
event. On a copy or cut, the listener asks your second closure for the text the
app wrote and puts it on the clipboard (a paste's write goes out too).

The listener also stops clipboard chords from reaching Ratzilla as keys.
`Cmd+C`, `Cmd+X`, and `Cmd+V` on a Mac, and `Ctrl+C`, `Ctrl+X`, and `Ctrl+V`
elsewhere, become `Event::Copy`, `Event::Cut`, and `Event::Paste`, so an app
binds those events rather than the keys. Off a Mac, `Shift+Delete` becomes
`Event::Cut` too. Every other `Cmd` or `Super` chord is dropped on every
platform and never reaches the app: the Mac's `Cmd` editing chords do nothing in
a field, and `Cmd+A` does not select the page.

Install one listener per app, given the app's element. An event belongs to the
app while focus is on that element or inside it, and to the page otherwise, so
the page's own inputs, buttons, and shortcuts keep their keys, and two apps on
one page each get only their own. For a canvas backend (`WebGl2Backend`,
`CanvasBackend`), pass the canvas it draws on or the container you gave it as
`grid_id`. For `DomBackend`, pass the element whose id you gave as `grid_id`.

A copy or cut the app answers with nothing copies nothing, unless text inside
its element is selected. The runtime ignores events before the first render, so
an early paste is left to the page.

Dropping the guard uninstalls the listener, so keep it for as long as the app
runs. A wasm `main` returns once `draw_web` is running, which would drop a local
binding at once, so leak it there:

```rust
use ratzilla::web_sys;

// The element whose id the backend was given as `grid_id`.
let element = web_sys::window()
    .and_then(|window| window.document())
    .and_then(|document| document.get_element_by_id("app"))
    .ok_or_else(|| io::Error::other("no #app element"))?;
let clipboard = BrowserClipboard::install(
    &element,
    {
        let app = Rc::clone(&app);
        move |event| !matches!(app.borrow_mut().handle_event(event), EventResult::Ignored)
    },
    {
        let app = Rc::clone(&app);
        move || app.borrow_mut().ratcn.take_clipboard()
    },
)?;
// `main` returns once the app is running; the listener has to outlive it.
std::mem::forget(clipboard);
```

A write after any other event, such as `Ctrl+C` on a Mac or a click on a "Copy"
button, goes out through `BrowserClipboard::write`. The browser allows it while
it handles the user's key press or click, on https or localhost, and in a
cross-origin iframe only with `allow="clipboard-write"`. Anywhere else the
write is skipped.

Either a key or a click can write, so route both through one function that
writes afterward:

```rust
fn route(app: &RefCell<App>, event: impl TryInto<Event>) {
    app.borrow_mut().handle_event(event);
    let written = app.borrow_mut().ratcn.take_clipboard();
    if let Some(text) = written {
        BrowserClipboard::write(&text);
    }
}

terminal.on_key_event({
    let app = Rc::clone(&app);
    move |event| route(&app, event)
}).map_err(|error| io::Error::other(error.to_string()))?;
terminal.on_mouse_event({
    let app = Rc::clone(&app);
    move |event| route(&app, event)
}).map_err(|error| io::Error::other(error.to_string()))?;
```
