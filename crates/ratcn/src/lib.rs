//! Themeable terminal UI components for [`ratatui`], plus the runtime that makes
//! them interactive.
//!
//! Ratatui gives you widgets that draw. `ratcn` adds the parts a real app needs
//! on top: components that can be focused, hovered, and clicked, in a
//! shadcn-inspired visual style you can theme.
//!
//! Themes do not have to be picked from a list: [`Theme::adaptive`] solves a
//! whole palette — wells, surfaces, text, lines, accents — from a background and
//! a foreground someone else chose. `ratcn::terminal` (feature `termina`) asks
//! the terminal for that pair and re-solves when the user changes it.
//!
//! # Preview status
//!
//! This is a preview release.
//!
//! - **The API will break.** Pin an exact version.
//! - **There is no install command.** Each component module is self-contained
//!   and meant to be copied into your own project, but copying is a manual file
//!   copy today. A CLI is intended and does not exist.
//! - **The component set is small and growing.** Nine ship today: [`Button`],
//!   [`List`], [`Select`], [`Tabs`], [`Dialog`], [`Toaster`](ToasterWidget),
//!   [`BarChartWidget`], [`Tooltip`], and [`ScrollArea`]. Text input and a
//!   multi-line text area are planned next; there is no text entry component at
//!   all right now.
//!
//! # Up to two halves, usable alone
//!
//! Components have up to two cooperating halves:
//!
//! - **Paint widgets** ([`ButtonWidget`], [`ListWidget`], [`TabsWidget`], …)
//!   are ordinary ratatui `Widget`s. Tell one what to look like and render it.
//! - **Interactive components** ([`Button`], [`List`], [`Tabs`], …) add focus
//!   and event handling, and paint through the widget half. These are declared
//!   through [`runtime::Ratcn`].
//!
//! [`ToasterWidget`] is paint-only and stops at the widget half.
//! [`BarChartWidget`] is paint-only too, but it is also not a component in the
//! sense used above: it is a themed adapter over ratatui's own `BarChart`,
//! adding theme colors, grouping, and a value-display switch to a chart ratatui
//! draws. [`Dialog`] and [`ScrollArea`] are interactive composites that stop at
//! the component half: their frame and their viewport are geometry the
//! component paints itself.
//!
//! The paint widgets drop straight into a plain ratatui app. They take a theme
//! and some bools, and `frame.render_widget(...)` is the whole integration —
//! no `Ratcn`, no declaration pass, no message type. If you already have focus
//! and event handling you are happy with, take the components' *look* and leave
//! the runtime alone. Where both halves exist, the interactive half paints
//! through the same widget.
//!
//! # It does not take over your app
//!
//! This is a toolkit, not a framework. Your app keeps its event loop, its state,
//! and its update function. The runtime enters at exactly two call sites, and
//! both can be removed again:
//!
//! - [`runtime::Ratcn::render`] — declare and paint this frame's components.
//! - [`runtime::Ratcn::handle_event`] — route one event, get back a message.
//!
//! State stays yours throughout. Components read it and return messages asking
//! for changes; nothing writes your state but you.
//!
//! # Where things live
//!
//! Components, themes, and the state types you store ([`ToasterState`],
//! [`Theme`]) are at the crate root. Runtime types — the
//! engine, focus, events, and the traits for writing your own components — are
//! under [`runtime`].
//!
//! Beside them sit the copy-support modules: [`button_shape`], [`color`],
//! [`geometry`], [`linear_nav`], [`list_core`], [`selection_indicator`], and
//! [`text_width`]. They hold the pieces more than one component needs — the
//! button idiom's cap and fill rows, the color arithmetic every focus, hover,
//! and disabled state derives through, area arithmetic, item-index movement,
//! value-keyed items and their row viewport, the radio and checkbox markers,
//! display-width measurement — so a component module depends on the crate root
//! and these, and on no sibling component. That is what lets you copy one
//! component module into your own project and have it compile against `ratcn`
//! alone.
//!
//! # Examples
//! ```no_run
//! use ratatui::{Terminal, backend::TestBackend};
//! use ratcn::{
//!     Button, Theme,
//!     runtime::{Event, EventResult, FocusState, KeyCode, KeyEvent, Ratcn},
//! };
//!
//! #[derive(Default)]
//! struct AppState {
//!     focus: FocusState,
//!     saving: bool,
//! }
//!
//! enum Msg {
//!     FocusChanged(FocusState),
//!     Save,
//! }
//! # fn update(state: &mut AppState, msg: Msg) {
//! #     match msg {
//! #         Msg::FocusChanged(focus) => state.focus = focus,
//! #         Msg::Save => state.saving = true,
//! #     }
//! # }
//!
//! let mut state = AppState::default();
//! let mut ratcn = Ratcn::new()
//!     .focus(|state: &AppState| &state.focus, Msg::FocusChanged);
//! let theme = Theme::default_dark();
//! let mut terminal = Terminal::new(TestBackend::new(20, 3)).expect("terminal");
//!
//! // Declare the current component surface as part of every frame.
//! terminal.draw(|frame| {
//!     let area = frame.area();
//!     ratcn.render(frame, &state, &theme, |ctx| {
//!         ctx.component(
//!             "save",
//!             Button::new("Save")
//!                 .disabled(state.saving)
//!                 .on_press(|| Msg::Save),
//!             area,
//!         );
//!     });
//! }).expect("draw");
//!
//! // Hand backend events to the retained surface from the last successful frame.
//! let event = Event::Key(KeyEvent::new(KeyCode::Enter));
//! match ratcn.handle_event(event, &state) {
//!     EventResult::Emit(msg) => update(&mut state, msg),
//!     EventResult::Consumed | EventResult::Ignored => {}
//! }
//! ```

mod backdrop;
pub mod button_shape;
pub mod color;
mod components;
#[cfg(feature = "crossterm")]
pub mod crossterm;
pub mod geometry;
pub mod linear_nav;
pub mod list_core;
pub mod runtime;
pub mod selection_indicator;
#[cfg(feature = "termina")]
pub mod terminal;
#[cfg(test)]
mod test_support;
pub mod text_width;
pub mod theme;
pub mod toast;

#[doc(inline)]
pub use components::{
    barchart::{BarChartGroup, BarChartStyle, BarChartWidget},
    button::{Button, ButtonSize, ButtonStyle, ButtonVariant, ButtonWidget},
    dialog::{Dialog, DialogStyle},
    list::{List, ListStyle, ListWidget},
    scroll_area::{ScrollArea, ScrollAreaStyle},
    select::{Select, SelectStyle, SelectWidget},
    tabs::{Tab, Tabs, TabsActivation, TabsSize, TabsStyle, TabsWidget},
    toast::{ToastPosition, ToasterStyle, ToasterWidget},
    tooltip::{Tooltip, TooltipSide, TooltipStyle, TooltipWidget},
};
#[doc(inline)]
pub use list_core::{ListItem, ListItemState};
#[doc(inline)]
pub use theme::{BorderStyle, Theme};
#[doc(inline)]
pub use toast::{Toast, ToastEntry, ToastKind, ToasterState};
