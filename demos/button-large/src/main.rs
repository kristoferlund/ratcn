//! The five button variants at `ButtonSize::Large`.
//!
//! Deliberately minimal: the variants themselves, with focus and hover.

use std::io;

#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

use ratatui::{
    layout::{Constraint, Flex, Layout},
    style::Style,
};
use ratcn::{
    Button, ButtonSize, Theme,
    runtime::{self, EventResult, FocusState, Ratcn, TabWrap},
};

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;

#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

const THEME: Theme = Theme::default_dark();

/// Declaration order is also Tab order, so the split below keeps the two rows
/// contiguous rather than interleaving them.
const TOP_ROW: usize = 3;
const BOTTOM_ROW: usize = BUTTONS.len() - TOP_ROW;

/// One entry per button: the child id and the label it shows.
const BUTTONS: [(&str, &str); 5] = [
    ("default", "Default"),
    ("outline", "Outline"),
    ("secondary", "Secondary"),
    ("ghost", "Ghost"),
    ("destructive", "Destructive"),
];

#[derive(Default)]
struct AppState {
    focus: FocusState,
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

impl App {
    fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new()
                .focus(|s: &AppState| &s.focus, Msg::Focus)
                .tab_wrap(TabWrap::Wrap),
        }
    }

    fn handle_event(&mut self, event: impl TryInto<runtime::Event>) {
        if let EventResult::Emit(msg) = self.ratcn.handle_event(event, &self.state) {
            match msg {
                Msg::Focus(focus) => self.state.focus = focus,
                Msg::Pressed => {}
            }
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(THEME.background));

        let state = &self.state;
        self.ratcn.render(frame, state, &THEME, |ctx| {
            let buttons = BUTTONS.map(|(id, label)| {
                let button = Button::new(label)
                    .size(ButtonSize::Large)
                    .on_press(|| Msg::Pressed);
                let button = match id {
                    "outline" => button.outline(),
                    "secondary" => button.secondary(),
                    "ghost" => button.ghost(),
                    "destructive" => button.destructive(),
                    _ => button,
                };
                (id, button)
            });

            // Five large buttons in one row is wider than a comfortable demo
            // window, so they wrap onto a second row.
            let [top_row, bottom_row] = Layout::vertical([
                Constraint::Length(ButtonSize::Large.height()),
                Constraint::Length(ButtonSize::Large.height()),
            ])
            .flex(Flex::Center)
            .spacing(1)
            .areas(area);

            // Measure the components that will be declared below.
            let top_areas: [_; TOP_ROW] = Layout::horizontal(
                buttons[..TOP_ROW]
                    .iter()
                    .map(|(_, button)| Constraint::Length(button.width())),
            )
            .flex(Flex::Center)
            .spacing(2)
            .areas(top_row);
            let bottom_areas: [_; BOTTOM_ROW] = Layout::horizontal(
                buttons[TOP_ROW..]
                    .iter()
                    .map(|(_, button)| Constraint::Length(button.width())),
            )
            .flex(Flex::Center)
            .spacing(2)
            .areas(bottom_row);

            for ((id, button), button_area) in buttons
                .into_iter()
                .zip(top_areas.into_iter().chain(bottom_areas))
            {
                ctx.render_component(id, button, button_area);
            }
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> io::Result<()> {
    let mut app = App::new();
    ratatui::run(|terminal| {
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
    let backend = demo_shared::web_backend(THEME.background)?;
    let mut terminal = ratatui::Terminal::new(backend)?;
    let app = Rc::new(RefCell::new(App::new()));

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
