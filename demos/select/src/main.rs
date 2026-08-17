use std::io;

#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

use ratatui::{
    layout::{Constraint, Flex, Layout},
    style::Style,
};
use ratcn::{
    ListItem, Select, Theme,
    runtime::{self, EventResult, FocusState, Ratcn},
};

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;
#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

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
    fn handle_event(&mut self, event: impl TryInto<runtime::Event>) {
        if let Ok(event) = event.try_into()
            && let EventResult::Emit(msg) = self.ratcn.handle_event(event, &self.state)
        {
            self.update(msg);
        }
    }
    fn draw(&mut self, frame: &mut ratatui::Frame) {
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
