---
description: "Wiring the ratcn runtime into a native crossterm loop or a browser app with ratzilla: redraw policy, terminal setup, and feeding backend events in."
---

# Host integration

Ratcn does not own the application loop. The host owns redraw policy, time,
terminal setup and restoration, backend listeners, and global shortcuts. Ratcn
only paints through `render` and routes normalized input through `handle_event`.

Async work follows the same boundary. The host executes app-defined effects and
sends their completion messages back through a queue: a native loop wakes or
polls while work is pending so it can drain the queue and redraw, and a browser
draw callback drains it before painting the next frame. The
[effects example](./state-and-messages#effects-and-result-messages) shows both
forms.

## Native crossterm

Enable Ratcn's `crossterm` feature to pass crossterm events directly:

```sh
cargo add ratcn --features crossterm
cargo add ratatui --no-default-features --features layout-cache,crossterm
```

A blocking loop can draw, wait, apply routed messages, and repeat. If visible
state changes with time, the wait must wake at the next deadline. Toast expiry
is the common case:

```rust
loop {
    let now = app_time();
    let _ = app.state.toasts.prune_expired(now);
    terminal.draw(|frame| app.draw(frame, now))?;

    if let Some(timeout) = app.state.toasts.time_until_next_expiry(app_time())
        && !event::poll(timeout)?
    {
        continue;
    }

    let event = event::read()?;
    if is_global_quit(&event) {
        break;
    }
    app.handle_event(event);
}
```

When nothing is time-dependent, skip `poll` and let `read` block. A timeout
wakeup just loops back to prune and redraw — no input event needs inventing.

Terminal modes are also host state. Enable raw mode, alternate screen, mouse
capture, and bracketed paste only as needed, and restore each on every exit
path — cleanup written after the loop is skipped by `?` and panics, which is
why the demos use the RAII helper `ratcn::crossterm::InputModes`. The host
still explicitly picks the modes; Ratcn never changes terminal state on its
own. See [Mouse Input](./mouse) for mouse capture.

```rust
let _input_modes = ratcn::crossterm::InputModes::new()
    .mouse_capture()
    .bracketed_paste()
    .enable()?;
```

Text-accepting components need bracketed paste for terminal paste to arrive as
one `crossterm::Event::Paste(String)`; without it, pasted text arrives as a
stream of ordinary key events, and behavior varies by terminal.

Events Ratcn does not handle — resize, terminal focus, key releases — come back
as `EventResult::Ignored` for the host to act on when relevant. For global
shortcuts, see
[App Shortcuts](./rendering-and-events#app-shortcuts).

## Browser and Ratzilla

Enable the `ratzilla` feature for ratzilla key and mouse conversions and the
typed browser paste helper:

```sh
cargo add ratcn --features ratzilla
cargo add ratatui --no-default-features --features layout-cache
cargo add ratzilla
```

Ratzilla drives callbacks rather than a blocking read loop. Shared mutable app
state therefore normally uses `Rc<RefCell<App>>`:

```rust
let app = Rc::new(RefCell::new(App::new()));

terminal.on_key_event({
    let app = Rc::clone(&app);
    move |event| app.borrow_mut().handle_event(event)
}).map_err(|error| io::Error::other(error.to_string()))?;

terminal.draw_web(move |frame| app.borrow_mut().draw(frame));
```

Register mouse callbacks the same way; see
[Mouse Input](./mouse#in-the-browser) for the details. Time-based cleanup,
including toast pruning, belongs in the draw callback or another host callback
that can cause a frame, since there is no blocking loop to return to.

`draw_web` renders on every animation frame, whether or not anything changed. A
host that would rather pay for frames it needs can keep the terminal and call
`Terminal::draw` itself: request one animation frame when an event routes to
something — any `EventResult` but `Ignored` — and one when a deadline the app
named passes, and none while the app is idle. Motion is always at least
`Consumed` once a surface exists, so hover stays live under that rule. The demos
run on such a host, in `demos/shared`.

For paste, register a DOM `paste` listener and forward `text/plain` clipboard
data as `Event::Paste` — ratzilla does not provide this callback itself. The
demos wrap the wiring in `demo_shared::BrowserPasteListener`. Before calling
`prevent_default()`, check `Ratcn::has_rendered()`: `handle_event` safely
ignores events before the first frame, but it cannot undo default prevention
the host already performed.
