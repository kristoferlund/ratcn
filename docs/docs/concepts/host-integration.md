---
description: "Wiring the ratcn runtime into a host loop: a terminal Session that opens and restores the terminal, the event loop shape, and a browser app with ratzilla."
---

# Host integration

The host owns the application loop: redraw policy, time, terminal setup and
restoration, backend listeners, and global shortcuts. Ratcn declares and paints
a frame through `render`, and routes normalized input through `handle_event`.

Async work follows the same boundary. The host executes app-defined effects and
sends their completion messages back through a queue: a native loop wakes or
polls while work is pending so it can drain the queue and redraw, and a browser
draw callback drains it before painting the next frame. The
[effects example](./state-and-messages#effects-and-result-messages) shows both
forms.

## A terminal session

`ratcn::terminal::Session` is the terminal half of the host: it opens the
terminal — raw mode, the alternate screen, the input modes the app asked for —
and puts every one of them back on the way out. It comes with the `termina`
feature:

```sh
cargo add ratcn --features termina
cargo add ratatui --no-default-features --features layout-cache,std
```

The feature re-exports termina as `ratcn::terminal::termina`, so its types — the
`termina::Event` inside `SessionEvent::Input`, the escape sequences beneath it —
are named through ratcn, at the version ratcn already builds against.

`SessionOptions::new()` opens the alternate screen. `.mouse()` reports movement,
clicks, and scrolling as events, which every part of ratcn's mouse handling
needs; while it is on, the terminal's own text selection usually stops working.
`.paste()` delivers a whole paste as one `SessionEvent::Input`; converted with
`Event::try_from` it becomes one `Event::Paste`, so a pasted newline stays
distinct from the user hitting Enter.

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
colors it uses while opening, subscribes to changes it reports, and asks again
when the window regains focus, shortly after every change signal, and when input
resumes after a pause — which is how a change reaches an app whose terminal was
recoloured from outside, or has no change notification to send.
`session.theme()` is what that answer becomes, falling back to
`Theme::default_dark()` where the terminal keeps quiet;
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
user's choice, animate the transition.

## The event loop

`session.next(timeout)` waits at most `timeout`, or indefinitely when it is
`None`, and answers with the next thing worth telling the app about. `Ok(None)`
means the wait ran out with nothing to report — which is how state that changes
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
after the draw shows the frame's [pointer shape](./mouse#pointer-shape).

A `termina::Event` goes straight to `Ratcn::handle_event`; the conversion is a
`TryFrom` the runtime provides. Resize, terminal focus, and key releases come
back as `EventResult::Ignored` for the host to act on. For global shortcuts, see
[App Shortcuts](./rendering-and-events#app-shortcuts).

## Restoration is automatic

Dropping the `Session` switches every mode it turned on back off, in the reverse
of the order it turned them on, and shows the cursor again. That happens on
every path out: a `break`, a `?`, and an unwinding panic, which the session
installs a hook for.

## Apps on a crossterm backend

`ratcn::crossterm::InputModes` switches mouse capture and bracketed paste on for
a host that runs on crossterm and owns its own terminal lifecycle. It hands back
a guard that switches them off when dropped.

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
The host owns raw mode and the alternate screen. See [Mouse Input](./mouse) for
mouse capture.

## Browser and Ratzilla

Enable the `ratzilla` feature for ratzilla key and mouse conversions and the
browser clipboard listener:

```sh
cargo add ratcn --features ratzilla
cargo add ratatui --no-default-features --features layout-cache,std
cargo add ratzilla
```

The browser-only API (`BrowserClipboard`, `set_browser_pointer_shape`) exists only
on `wasm32`; read its
rustdoc with
`cargo doc -p ratcn --features ratzilla --target wasm32-unknown-unknown`.

Ratzilla drives callbacks, so shared mutable app state normally uses
`Rc<RefCell<App>>`. Its key and mouse callbacks hand you ratzilla events, so
the app's `handle_event` takes anything that converts to an `Event`, applies an
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

`set_browser_pointer_shape` shows the frame's [pointer shape](./mouse#pointer-shape)
as the element's CSS `cursor`. An app with text fields routes through the
`route` function in
[The clipboard](#the-clipboard) instead, so a copy reaches the clipboard. Wire
mouse callbacks the same way; see
[Mouse Input](./mouse#in-the-browser) for the details. Time-based cleanup,
including toast pruning, belongs in the draw callback or another host callback
that can cause a frame.

Ratzilla's canvas takes focus but leaves Tab to the browser, which moves focus
off the canvas: keys, and the clipboard chords with them, then go to the page.
An app that uses Tab for its own focus traversal stops Tab's default while the
canvas has focus, as the demos' `demos/shared/ratatui-keyboard-capture.js`
does.

`draw_web` renders on every animation frame. A host that draws only the frames
it needs can keep the terminal and call `Terminal::draw` itself: request one
animation frame when an event routes to something — any `EventResult` but
`Ignored` — and one when a deadline the app named passes. Motion is always at
least `Consumed` once a surface exists, so hover stays live under that rule. The
demos run on such a host, in `demos/shared`.

Ratzilla has no clipboard callback; see [The clipboard](#the-clipboard).

## The clipboard

A component writes the system clipboard with `EventCtx::set_clipboard`, as
[Input](../components/input#copy-and-paste) and
[TextArea](../components/textarea#copy-and-paste) do on a copy or cut. The
runtime keeps the latest write, and the host carries it out after each event,
whatever the event's result.

In a terminal, `Session::set_clipboard` writes it with the OSC 52 escape
sequence, which works over SSH too. Most terminals honor it; iTerm2 needs its
clipboard-access setting, tmux needs `set -g set-clipboard on`, and macOS
Terminal.app and the VTE terminals (GNOME Terminal, xfce4-terminal, Tilix)
ignore it. An app on `ratcn::crossterm` writes the text with crossterm's
`CopyToClipboard`, behind its `osc52` feature (add the crossterm that ratatui
uses: `cargo add crossterm@0.29 --features osc52`), in place of
`session.set_clipboard` below:

```rust
Some(text) => execute!(io::stdout(), CopyToClipboard::to_clipboard_from(text))?,
```

A field copies on `Ctrl+C` only with a selection, and lets it bubble without
one, so route `Ctrl+C` before treating it as quit. Quit only when the event put
nothing on the clipboard, not only when it came back `Ignored`: an open modal
consumes every key. Here `app` is your app, holding its `Ratcn` as `ratcn`, and
`is_quit` says whether an event is `Ctrl+C` (the loop in
[The event loop](#the-event-loop) is the same):

```rust
let quit = is_quit(&event);
app.handle_event(event);
match app.ratcn.take_clipboard() {
    Some(text) => session.set_clipboard(&text)?,
    None if quit => return Ok(()),
    None => {}
}
```

In the browser, `ratcn::runtime::BrowserClipboard` listens for the document's
`paste`, `copy`, and `cut` events and hands them to your first closure as
`Event::Paste`, `Event::Copy`, and `Event::Cut`; return whether the app took
the event. On a copy or cut it asks your second closure for the text the app
wrote, and puts it on the clipboard (a paste's write goes out too).

It also stops chords from reaching ratzilla as keys. `Cmd+C`, `Cmd+X`, and
`Cmd+V` on a Mac, and `Ctrl+C`, `Ctrl+X`, and `Ctrl+V` elsewhere, become
`Event::Copy`, `Event::Cut`, and `Event::Paste`, so an app binds those events,
not the keys; off a Mac, `Shift+Delete` becomes `Event::Cut` too. Every other
`Cmd` or `Super` chord is dropped, on every platform: it never reaches the app,
so the Mac's `Cmd` editing chords do nothing in a field, and `Cmd+A` does not
select the page.

Install one per app, given the app's element: an event is the app's while
focus is on that element or inside it, and the page's otherwise, so the page's
own inputs, buttons, and shortcuts keep their keys, and two apps on one page
each get only their own. Pass the canvas a canvas backend (`WebGl2Backend`,
`CanvasBackend`) draws on, or the container you gave it as `grid_id`; for
`DomBackend`, the element whose id you gave as `grid_id`. A copy or cut the
app answers with nothing copies nothing, unless text inside its element is
selected. The runtime ignores events before the first render, so an early
paste is left to the page.

Dropping the guard uninstalls the listener, so keep it for as long as the app
runs. A wasm `main` returns once `draw_web` is running, which would drop a
local binding at once, so leak it there, as below.

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

A write after any other event — `Ctrl+C` on a Mac, a "Copy" button — goes out
through `BrowserClipboard::write`. The browser allows it while it handles the
user's key press or click, on https or localhost, and in a cross-origin iframe
only with `allow="clipboard-write"`; anywhere else the write is skipped.

Route keys and the mouse through one function that writes afterwards, since
either can write — a click on a "Copy" button is a mouse event:

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
