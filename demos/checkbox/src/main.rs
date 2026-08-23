//! One checkbox, three skins.
//!
//! The default ballot-box pair, an ASCII checklist pair, and a switch that
//! wears its state as words. All three are the same [`Checkbox`] component —
//! the markers are the only thing that differs, which is also how a checkbox
//! becomes a toggle: give the two states labels instead of glyphs.

use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout},
    style::Style,
};
use ratcn::{
    Checkbox, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn},
};

#[derive(Default)]
struct AppState {
    focus: FocusState,
    vim: bool,
    mouse: bool,
    bell: bool,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Vim(bool),
    Mouse(bool),
    Bell(bool),
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new().focus(|s: &AppState| &s.focus, Msg::Focus),
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(focus) => self.state.focus = focus,
            Msg::Vim(checked) => self.state.vim = checked,
            Msg::Mouse(checked) => self.state.mouse = checked,
            Msg::Bell(checked) => self.state.bell = checked,
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
            let [row_a, row_b, row_c] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .flex(Flex::Start)
            .spacing(1)
            .areas(area);

            ctx.component(
                "default",
                Checkbox::new("Vim bindings").checked(|s: &AppState| s.vim, Msg::Vim),
                row_a,
            );
            ctx.component(
                "ascii",
                Checkbox::new("ASCII checklist")
                    .checked_marker("[x]")
                    .unchecked_marker("[ ]")
                    .checked(|s: &AppState| s.mouse, Msg::Mouse),
                row_b,
            );
            ctx.component(
                "switch",
                Checkbox::new("Terminal bell")
                    .checked_marker("[ON]")
                    .unchecked_marker("[off]")
                    .checked(|s: &AppState| s.bell, Msg::Bell),
                row_c,
            );
        });
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
