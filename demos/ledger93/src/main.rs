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

use app::App;

mod app;
mod nav;
mod screens;
mod shared;

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
