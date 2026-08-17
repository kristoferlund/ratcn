//! The five button variants at `ButtonSize::Small`.
//!
//! Deliberately minimal: the variants themselves, with focus and hover.
//!
//! `Outline` is omitted on purpose. A small button is a single row with no space
//! for a border, so `Outline` draws none — leaving it visually identical to
//! `Ghost` at rest, and with no fill change when focused.

use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout},
    style::{Color, Style},
};
use ratcn::{
    Button, ButtonSize, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, TabWrap},
};

const THEME: Theme = Theme::default_dark();

/// One entry per button: the child id and the label it shows.
const BUTTONS: [(&str, &str); 4] = [
    ("default", "Default"),
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

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(focus) => self.state.focus = focus,
            Msg::Pressed => {}
        }
    }
}

impl demo_shared::Demo for App {
    fn background(&self) -> Color {
        THEME.background
    }

    fn handle_event(&mut self, event: Event) -> bool {
        match self.ratcn.handle_event(event, &self.state) {
            EventResult::Emit(msg) => {
                self.update(msg);
                true
            }
            EventResult::Consumed => true,
            EventResult::Ignored => false,
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(THEME.background));

        let state = &self.state;
        self.ratcn.render(frame, state, &THEME, |ctx| {
            let buttons = BUTTONS.map(|(id, label)| {
                let button = Button::new(label)
                    .size(ButtonSize::Small)
                    .on_press(|| Msg::Pressed);
                let button = match id {
                    "secondary" => button.secondary(),
                    "ghost" => button.ghost(),
                    "destructive" => button.destructive(),
                    _ => button,
                };
                (id, button)
            });

            let [row_area] = Layout::vertical([Constraint::Length(ButtonSize::Small.height())])
                .flex(Flex::Center)
                .spacing(1)
                .areas(area);

            // Measure the components that will be declared below.
            let areas: [_; BUTTONS.len()] = Layout::horizontal(
                buttons
                    .iter()
                    .map(|(_, button)| Constraint::Length(button.width())),
            )
            .flex(Flex::Center)
            .spacing(2)
            .areas(row_area);

            for ((id, button), button_area) in buttons.into_iter().zip(areas) {
                ctx.render_component(id, button, button_area);
            }
        });
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
