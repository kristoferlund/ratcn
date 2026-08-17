//! Tooltips on a row of buttons: hover or Tab to one and its bubble floats
//! beside it.
//!
//! Two things are on show. First, `open_when` combines the hover the runtime
//! keeps with the focus path the app keeps, counting focus only while the
//! keyboard is what is driving — so a tooltip appears on hover *and* on Tab,
//! and goes away again when the pointer or focus moves on. Nothing is stored
//! for the bubble itself and nothing in `update` writes it. Second, placement:
//! each button in the row
//! is named for the side its bubble prefers, and the one pinned to the frame's
//! top row has no room above it, so its bubble flips below.

use std::io;

#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

use ratatui::{
    layout::{Constraint, Flex, Layout},
    style::Style,
};
use ratcn::{
    Button, Theme, Tooltip, TooltipSide,
    runtime::{self, EventResult, FocusState, Ratcn, TabWrap},
};

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;
#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

const THEME: Theme = Theme::default_dark();

/// The child id every Tooltip gives the button inside it. Unique among its own
/// siblings, which is all an id has to be.
const TRIGGER: &str = "button";

mod ids {
    pub const LEFT: &str = "left";
    pub const TOP: &str = "top";
    pub const BOTTOM: &str = "bottom";
    pub const RIGHT: &str = "right";
    pub const EDGE: &str = "edge";
}

/// What every button in the row explains. Only the edge one differs, because
/// it is there to show the bubble flipping rather than the text.
const TIP: &str = "Hi!";

/// One explained button: the Tooltip's id, the button's label, the explanation,
/// and the side the bubble prefers.
type Explained = (&'static str, &'static str, &'static str, TooltipSide);

/// The centered row. Each button is labelled with the side its bubble prefers.
const ROW: [Explained; 4] = [
    (ids::LEFT, "Left", TIP, TooltipSide::Left),
    (ids::TOP, "Top", TIP, TooltipSide::Top),
    (ids::BOTTOM, "Bottom", TIP, TooltipSide::Bottom),
    (ids::RIGHT, "Right", TIP, TooltipSide::Right),
];

/// Pinned to the frame's top row, so `Top` has nowhere to go and flips.
const EDGE: Explained = (
    ids::EDGE,
    "Edge",
    "This one prefers the top, but there is no room above it, so the bubble flips below",
    TooltipSide::Top,
);

#[derive(Default)]
struct AppState {
    focus: FocusState,
    /// Whether the last input came from the keyboard.
    ///
    /// A click focuses what it hits, so focus alone cannot say whether the
    /// user is navigating by keyboard: pairing it with this is what keeps a
    /// bubble from lingering after a click, the same distinction the web
    /// draws between `:focus` and `:focus-visible`. The app owns it because
    /// the app sees every event.
    keyboard: bool,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Pressed,
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

/// The button a Tooltip explains. Built twice per frame — once to measure, once
/// to declare — so its width and its area cannot disagree.
fn button(label: &'static str) -> Button<Msg> {
    Button::new(label).on_press(|| Msg::Pressed)
}

/// Wrap `label`'s button in the tooltip that explains it.
///
/// `id` is the Tooltip's own id at the root, which makes it the first element
/// of the focus path of everything inside it — so a prefix query answers "is
/// the keyboard on this button?"
fn explained((id, label, tip, side): Explained) -> Tooltip<AppState, Msg> {
    Tooltip::new(tip)
        .side(side)
        // The hover half is the runtime's answer, handed to the reader; only
        // the keyboard half is the app's. Focus counts only while the keyboard
        // is driving: a click focuses what it hits, and a bubble that outlived
        // the click would sit there until focus moved on.
        .open_when(move |state: &AppState, hovered| {
            hovered || (state.keyboard && state.focus.contains_path([id]))
        })
        .trigger(move |ctx| {
            let area = ctx.area();
            ctx.render_component(TRIGGER, button(label), area);
        })
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new()
                .focus(|state: &AppState| &state.focus, Msg::Focus)
                .tab_wrap(TabWrap::Wrap),
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(focus) => self.state.focus = focus,
            Msg::Pressed => {}
        }
    }

    fn handle_event(&mut self, event: impl TryInto<runtime::Event>) {
        let Ok(event) = event.try_into() else {
            return;
        };
        // Record which device the user is on before routing, so the tooltip
        // reader can tell keyboard focus from the focus a click leaves behind.
        self.state.keyboard = !matches!(event, runtime::Event::Mouse(_));
        if let EventResult::Emit(msg) = self.ratcn.handle_event(event, &self.state) {
            self.update(msg);
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(THEME.background));

        let [edge_area, rest_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
        let [row_area] = Layout::vertical([Constraint::Length(1)])
            .flex(Flex::Center)
            .areas(rest_area);

        let state = &self.state;
        self.ratcn.render(frame, state, &THEME, |ctx| {
            let [edge_button_area] =
                Layout::horizontal([Constraint::Length(button(EDGE.1).width())])
                    .flex(Flex::Center)
                    .areas(edge_area);
            ctx.render_component(EDGE.0, explained(EDGE), edge_button_area);

            let areas: [_; ROW.len()] = Layout::horizontal(
                ROW.map(|(_, label, _, _)| Constraint::Length(button(label).width())),
            )
            .flex(Flex::Center)
            .spacing(2)
            .areas(row_area);
            for (entry, button_area) in ROW.into_iter().zip(areas) {
                ctx.render_component(entry.0, explained(entry), button_area);
            }
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> io::Result<()> {
    let mut app = App::new();
    ratatui::run(|terminal| {
        let _modes = ratcn::crossterm::InputModes::new()
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
    let backend = demo_shared::web_backend(THEME.background)?;
    let mut terminal = ratatui::Terminal::new(backend)?;
    let app = Rc::new(RefCell::new(App::new()));
    terminal
        .on_key_event({
            let app = Rc::clone(&app);
            move |event| app.borrow_mut().handle_event(event)
        })
        .map_err(|e| io::Error::other(e.to_string()))?;
    terminal
        .on_mouse_event({
            let app = Rc::clone(&app);
            move |event| app.borrow_mut().handle_event(event)
        })
        .map_err(|e| io::Error::other(e.to_string()))?;
    terminal.draw_web(move |frame| app.borrow_mut().draw(frame));
    Ok(())
}
