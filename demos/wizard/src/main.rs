//! A four-step wizard that walks through starting a RATCN app, and a worked
//! example of splitting a multi-screen app by ownership rather than by widget.
//!
//! The steps:
//!
//! - **Create a project** — a plain binary crate, no scaffold.
//! - **Pick a backend** — a Select whose choice rewrites the dependency commands.
//! - **Pick a theme** — a List whose choice re-colors the wizard as it is made.
//! - **Done** — what the choices added up to.
//!
//! The structure is the point. `AppState` is subdivided by ownership
//! (orchestration, shared choices, one struct per step); each step module owns
//! its own `State`/`Msg`/`update`/`declare`, and steps that choose nothing own
//! nothing; and the shell's top-level `update` only routes messages to their
//! owner. See `app.rs`, `nav.rs`, `shared.rs`, and `steps/`.
//!
//! Controls:
//! - `Enter` / `Space`: press the focused button, or open and choose in a panel
//! - `Tab` / `Shift+Tab`: move focus between the open step and the buttons
//! - `↑` / `↓`: move within a list or an open panel

use std::io;

use app::App;

mod app;
mod nav;
mod shared;
mod steps;

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
