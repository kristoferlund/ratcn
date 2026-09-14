//! Two one-line fields: a name, and a greeting that starts filled.
//!
//! Type into the focused field. Tab moves between them. The empty name shows
//! its placeholder; the greeting already has a value, so the caret starts at
//! the end. Click to place the caret. Enter and Esc are left to the app.

use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    text::Line,
};
use ratcn::{
    Input, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, TabWrap},
};

const DEMO_WIDTH: u16 = 34;
const DEMO_HEIGHT: u16 = 7;
const CONTENT_PADDING: Margin = Margin::new(2, 1);

struct AppState {
    focus: FocusState,
    name: String,
    greeting: String,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            focus: FocusState::default(),
            name: String::new(),
            greeting: "Hello".into(),
        }
    }
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Name(String),
    Greeting(String),
}

pub struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

impl App {
    pub fn new() -> Self {
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
            Msg::Name(name) => self.state.name = name,
            Msg::Greeting(greeting) => self.state.greeting = greeting,
        }
    }
}

impl demo_shared::Demo for App {
    const PASTE: bool = true;

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

    /// The caret blinks on whichever field holds focus. One of the two
    /// always does — default focus lands on the first — so this demo is
    /// never fully idle.
    fn wake(&self) -> Option<Duration> {
        Some(Duration::from_millis(80))
    }

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        buffer.set_style(area, Style::default().bg(theme.background));

        let state = &self.state;
        self.ratcn.render_into(buffer, area, state, theme, |ctx| {
            let demo = area.centered(
                Constraint::Length(DEMO_WIDTH),
                Constraint::Length(DEMO_HEIGHT),
            );
            ctx.paint(move |ctx| {
                let surface = ctx.theme.surface;
                ctx.with_buffer(|buf| {
                    buf.set_style(demo, Style::default().bg(surface));
                });
            });

            let inner = demo.inner(CONTENT_PADDING);
            let [name_block, greet_block] = Layout::vertical([Constraint::Length(2); 2])
                .spacing(1)
                .areas(inner);
            let [name_label, name_row] =
                Layout::vertical([Constraint::Length(1); 2]).areas(name_block);
            let [greet_label, greet_row] =
                Layout::vertical([Constraint::Length(1); 2]).areas(greet_block);

            let muted = Style::default().fg(theme.muted_foreground);
            ctx.paint_widget(Line::from("Name").style(muted), name_label);
            ctx.component(
                "name",
                Input::new()
                    .placeholder("Your name")
                    .value(|s: &AppState| s.name.as_str(), Msg::Name),
                name_row,
            );
            ctx.paint_widget(Line::from("Greeting").style(muted), greet_label);
            ctx.component(
                "greeting",
                Input::new().value(|s: &AppState| s.greeting.as_str(), Msg::Greeting),
                greet_row,
            );
        });
    }
}
