//! Themeable terminal UI components for [`ratatui`], plus the runtime that makes
//! them interactive.
//!
//! Ratatui gives you widgets that draw. `ratcn` adds the parts a real app needs
//! on top: components that can be focused, hovered, and clicked, in a
//! shadcn-inspired visual style you can theme.
//!
//! # Preview status
//!
//! This is a preview release.
//!
//! - **The API will break.** The public surface is still moving: recent work
//!   has renamed methods, changed signatures, and removed components. Pin an
//!   exact version and expect to edit when you upgrade.
//! - **There is no install command.** Each component module is self-contained
//!   and meant to be copied into your own project, but copying is a manual file
//!   copy today. A CLI is intended and does not exist.
//! - **The component set is small and growing.** Eight ship today: [`Button`],
//!   [`List`], [`Select`], [`Tabs`], [`Dialog`], [`Toaster`](ToasterWidget),
//!   [`BarChartWidget`], and [`Tooltip`]. Text input, a multi-line text area,
//!   and a scroll area are planned next; there is no text entry component at
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
//! draws. [`Dialog`] is the opposite exception: it is an interactive composite
//! with no separate paint widget.
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
//!         ctx.render_component(
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
pub mod linear_nav;
pub mod list_core;
pub mod runtime;
pub mod selection_indicator;
pub mod text_width;
mod theme;
pub mod toast;

#[doc(inline)]
pub use components::{
    BarChartGroup, BarChartStyle, BarChartWidget, Button, ButtonRenderMode, ButtonSize,
    ButtonStyle, ButtonVariant, ButtonWidget, Dialog, DialogStyle, List, ListStyle, ListWidget,
    Select, SelectStyle, SelectWidget, Tab, Tabs, TabsActivation, TabsSize, TabsStyle, TabsWidget,
    ToastPosition, ToasterStyle, ToasterWidget, Tooltip, TooltipSide, TooltipStyle, TooltipWidget,
};
#[doc(inline)]
pub use list_core::{ListItem, ListItemState};
#[doc(inline)]
pub use theme::{BorderStyle, Theme};
#[doc(inline)]
pub use toast::{Toast, ToastEntry, ToastKind, ToasterState};
