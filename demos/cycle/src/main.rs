//! Settings rows: the setting's name on the left, a [`Cycle`] on the right.
//!
//! The pattern most settings screens want — each row reads "name value",
//! and the value cycles in place through its options. A Cycle is exactly as
//! wide as the value it currently shows; `.align(Alignment::Right)` hugs it
//! against the panel's edge, so a row is one declaration, unmeasured.
//!
//! Tab moves between rows. Enter, Space, an arrow, or a click advances the
//! focused one, wrapping at both ends.

use std::io;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Margin},
    style::Style,
    text::Line,
};
use ratcn::{
    Cycle, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, TabWrap},
};

const DEMO_WIDTH: u16 = 34;
const DEMO_HEIGHT: u16 = 7;
const CONTENT_PADDING: Margin = Margin::new(2, 1);

const SIZES: [&str; 3] = ["Small", "Medium", "Large"];
const CADENCES: [&str; 4] = ["Live", "Every minute", "Hourly", "Off"];
const KEYMAPS: [&str; 2] = ["Readline", "Vim"];

#[derive(Default)]
struct AppState {
    focus: FocusState,
    size: usize,
    cadence: usize,
    keymap: usize,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Size(usize),
    Cadence(usize),
    Keymap(usize),
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
            Msg::Size(index) => self.state.size = index,
            Msg::Cadence(index) => self.state.cadence = index,
            Msg::Keymap(index) => self.state.keymap = index,
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

            let inner = demo.inner(CONTENT_PADDING);
            let [row_a, row_b, row_c] = Layout::vertical([Constraint::Length(1); 3])
                .spacing(1)
                .areas(inner);

            // Each row is one declaration: the setting's name painted at the
            // left edge, and the cycle hugging the right edge of the same
            // row — only the value's own columns respond.
            let name = Style::default().fg(theme.foreground);

            ctx.paint_widget(Line::from("Text size").style(name), row_a);
            ctx.component(
                "size",
                Cycle::new(SIZES)
                    .selection(|s: &AppState| s.size, Msg::Size)
                    .align(Alignment::Right),
                row_a,
            );

            ctx.paint_widget(Line::from("Refresh").style(name), row_b);
            ctx.component(
                "cadence",
                Cycle::new(CADENCES)
                    .selection(|s: &AppState| s.cadence, Msg::Cadence)
                    .align(Alignment::Right),
                row_b,
            );

            ctx.paint_widget(Line::from("Key map").style(name), row_c);
            ctx.component(
                "keymap",
                Cycle::new(KEYMAPS)
                    .selection(|s: &AppState| s.keymap, Msg::Keymap)
                    .align(Alignment::Right),
                row_c,
            );
        });
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
