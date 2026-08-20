//! The five button variants at `ButtonSize::Large`.
//!
//! Deliberately minimal: the variants themselves, with focus and hover.

use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout},
    style::Style,
};
use ratcn::{
    Button, ButtonSize, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, TabWrap},
};

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

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(focus) => self.state.focus = focus,
            Msg::Pressed => {}
        }
    }
}

impl demo_shared::Demo for App {
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

    fn draw(&mut self, frame: &mut Frame, theme: &Theme) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(theme.background));

        let state = &self.state;
        self.ratcn.render(frame, state, theme, |ctx| {
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
                ctx.component(id, button, button_area);
            }
        });
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
