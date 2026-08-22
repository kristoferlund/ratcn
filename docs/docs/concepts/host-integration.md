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
    let _ = app.state.toasts.prune_expired(now);
    let theme = session.theme();
    session.terminal_mut().draw(|frame| app.draw(frame, &theme, now))?;

    let timeout = app.state.toasts.time_until_next_expiry(app_time());
    let Some(event) = session.next(timeout)? else {
        // The deadline arrived. Loop back to prune and redraw.
        continue;
    };

    if let SessionEvent::Input(event) = event {
        if is_global_quit(&event) {
            break;
        }
        app.handle_event(event);
    }
}
```

When nothing is time-dependent, pass `None` and let the wait block.

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
typed browser paste helper:

```sh
cargo add ratcn --features ratzilla
cargo add ratatui --no-default-features --features layout-cache,std
cargo add ratzilla
```

Ratzilla drives callbacks, so shared mutable app state normally uses
`Rc<RefCell<App>>`:

```rust
let app = Rc::new(RefCell::new(App::new()));

terminal.on_key_event({
    let app = Rc::clone(&app);
    move |event| app.borrow_mut().handle_event(event)
}).map_err(|error| io::Error::other(error.to_string()))?;

terminal.draw_web(move |frame| app.borrow_mut().draw(frame));
```

Wire mouse callbacks the same way; see
[Mouse Input](./mouse#in-the-browser) for the details. Time-based cleanup,
including toast pruning, belongs in the draw callback or another host callback
that can cause a frame.

`draw_web` renders on every animation frame. A host that draws only the frames
it needs can keep the terminal and call `Terminal::draw` itself: request one
animation frame when an event routes to something — any `EventResult` but
`Ignored` — and one when a deadline the app named passes. Motion is always at
least `Consumed` once a surface exists, so hover stays live under that rule. The
demos run on such a host, in `demos/shared`.

For paste, add a DOM `paste` listener and forward `text/plain` clipboard
data as `Event::Paste`. The demos wrap the wiring in
`demo_shared::BrowserPasteListener`. Guard `prevent_default()` behind
`Ratcn::has_rendered()`, so the host takes the event over once the first frame
is on screen.
