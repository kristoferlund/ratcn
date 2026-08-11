//! LEDGER-93: a nineties double-entry bookkeeping terminal, and a worked example
//! of structuring a multi-view RATCN app without one giant state blob.
//!
//! Three tabs share one theme:
//!
//! - **Ledger** — the book of transactions, amounts in the current currency.
//! - **Report** — expenses by category as a bar chart, with a local sort toggle.
//! - **Settings** — edits the shared currency preference that re-formats the
//!   other two tabs.
//!
//! The structure is the point. `AppState` is subdivided by ownership
//! (orchestration, shared state, one struct per view); each view module owns its
//! own `State`/`Msg`/`update`/`view()`; and the shell's top-level `update` only
//! routes messages to their owner. See `app.rs`, `shared.rs`, and `screens/`.
//!
//! Controls:
//! - `Tab` / `Shift+Tab`: move focus between the tab row and the open view
//! - `←` / `→`, `Home` / `End`: move tab focus; `Enter` or a click opens it
//! - `Enter` / `Space`: press a button or choose a list row

use std::io;

#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

use app::App;

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;

#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

mod app;
mod nav;
mod screens;
mod shared;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> io::Result<()> {
    let mut app = App::new();
    ratatui::run(|terminal| {
        // Keep mouse reporting enabled until the terminal exits.
        let _input_modes = ratcn::crossterm::InputModes::new()
            .mouse_capture()
            .enable()?;
        loop {
            terminal.draw(|frame| app.draw(frame))?;
            let event = event::read()?;
            if demo_shared::is_quit(&event) {
                break Ok(());
            }
            app.handle_event(event);
        }
    })
}

#[cfg(target_arch = "wasm32")]
fn main() -> io::Result<()> {
    let app = Rc::new(RefCell::new(App::new()));
    let backend = demo_shared::web_backend(app::THEME.background)?;
    let mut terminal = ratatui::Terminal::new(backend)?;

    terminal
        .on_key_event({
            let app = Rc::clone(&app);
            move |key_event| app.borrow_mut().handle_event(key_event)
        })
        .map_err(|e| io::Error::other(e.to_string()))?;
    terminal
        .on_mouse_event({
            let app = Rc::clone(&app);
            move |mouse_event| app.borrow_mut().handle_event(mouse_event)
        })
        .map_err(|e| io::Error::other(e.to_string()))?;

    terminal.draw_web(move |frame| app.borrow_mut().draw(frame));
    Ok(())
}
