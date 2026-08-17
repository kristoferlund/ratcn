use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout},
    style::{Color, Style},
};
use ratcn::{
    ListItem, Select, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn},
};

const THEME: Theme = Theme::default_dark();
const FRUITS: [&str; 10] = [
    "Mango",
    "Papaya",
    "Pineapple",
    "Guava",
    "Passion fruit",
    "Dragon fruit",
    "Lychee",
    "Starfruit",
    "Rambutan",
    "Coconut",
];

#[derive(Default)]
struct State {
    focus: FocusState,
    selected: Option<&'static str>,
    cursor: Option<&'static str>,
    open: bool,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Open(bool),
    Focused(&'static str),
    Selected(&'static str),
}

struct App {
    state: State,
    ratcn: Ratcn<State, Msg>,
}

impl App {
    fn new() -> Self {
        Self {
            state: State::default(),
            ratcn: Ratcn::new().focus(|s: &State| &s.focus, Msg::Focus),
        }
    }
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(v) => self.state.focus = v,
            Msg::Open(v) => self.state.open = v,
            Msg::Focused(v) => self.state.cursor = Some(v),
            Msg::Selected(v) => {
                self.state.selected = Some(v);
                self.state.cursor = Some(v);
                self.state.open = false;
            }
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
        let [column] = Layout::horizontal([Constraint::Length(28)])
            .flex(Flex::Center)
            .areas(area);
        let [select_area] = Layout::vertical([Constraint::Length(1)])
            .flex(Flex::Center)
            .areas(column);
        self.ratcn.render(frame, &self.state, &THEME, |ctx| {
            ctx.render_component(
                "fruit",
                Select::new(FRUITS.map(|fruit| ListItem::new(fruit, fruit)))
                    .placeholder("Pick a fruit...")
                    .open(|s: &State| s.open, Msg::Open)
                    .item_focus(|s: &State| s.cursor, Msg::Focused)
                    .selection(|s: &State| s.selected, Msg::Selected),
                select_area,
            );
        });
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
