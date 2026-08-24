//! One checkbox, three skins.
//!
//! The default boxes, an ASCII checklist pair, and a switch that
//! wears its state as words. All three are the same [`Checkbox`] component —
//! the markers are the only thing that differs, which is also how a checkbox
//! becomes a toggle: give the two states labels instead of glyphs.
//!
//! Tab moves between them; Enter, Space, or a click anywhere on a row flips
//! it. Hover and focus raise the row, exactly as they do on every other
//! control.

use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
};
use ratcn::{
    Checkbox, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, TabWrap},
};

const DEMO_WIDTH: u16 = 26;
const DEMO_HEIGHT: u16 = 7;
const CONTENT_PADDING: Margin = Margin::new(2, 1);

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
            ratcn: Ratcn::new()
                .focus(|s: &AppState| &s.focus, Msg::Focus)
                .tab_wrap(TabWrap::Wrap),
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

            // One row per checkbox, each exactly as wide as its content.
            let inner = demo.inner(CONTENT_PADDING);
            let [row_a, row_b, row_c] = Layout::vertical([Constraint::Length(1); 3])
                .spacing(1)
                .areas(inner);

            let vim = Checkbox::new("Vim bindings").checked(|s: &AppState| s.vim, Msg::Vim);
            let row_a = Rect {
                width: vim.width(),
                ..row_a
            };
            ctx.component("default", vim, row_a);

            let ascii = Checkbox::new("ASCII checklist")
                .checked_marker("[x]")
                .unchecked_marker("[ ]")
                .checked(|s: &AppState| s.mouse, Msg::Mouse);
            let row_b = Rect {
                width: ascii.width(),
                ..row_b
            };
            ctx.component("ascii", ascii, row_b);

            let bell = Checkbox::new("Terminal bell")
                .checked_marker("[ON]")
                .unchecked_marker("[off]")
                .checked(|s: &AppState| s.bell, Msg::Bell);
            let row_c = Rect {
                width: bell.width(),
                ..row_c
            };
            ctx.component("switch", bell, row_c);
        });
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
