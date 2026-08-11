//! A four-step wizard that walks through starting a RATCN app, and a worked
//! example of splitting a multi-screen app by ownership rather than by widget.
//!
//! The steps:
//!
//! - **Create a project** — a plain binary crate, no scaffold.
//! - **Pick a backend** — a Select whose choice rewrites the install command.
//! - **Pick a theme** — a List whose choice re-colors the wizard as it is made.
//! - **Done** — what the choices added up to.
//!
//! The structure is the point. `AppState` is subdivided by ownership
//! (orchestration, shared choices, one struct per step); each step module owns
//! its own `State`/`Msg`/`update`/`render`, and steps that choose nothing own
//! nothing; and the shell's top-level `update` only routes messages to their
//! owner. See `app.rs`, `nav.rs`, `shared.rs`, and `steps/`.
//!
//! Controls:
//! - `Enter` / `Space`: press the focused button, or open and choose in a panel
//! - `Tab` / `Shift+Tab`: move focus between the open step and the buttons
//! - `↑` / `↓`: move within a list or an open panel

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
mod shared;
mod steps;

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
    // Canvas padding is fixed at construction, so it tracks the starting theme.
    // Switching themes at runtime leaves it on the previous background.
    let backend = demo_shared::web_backend(app.borrow().palette().background)?;
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
