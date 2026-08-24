//! Settings rows: the setting's name on the left, a [`Cycle`] on the right.
//!
//! The pattern most settings screens want — each row reads "name: value",
//! and the value cycles in place through its options. A Cycle is exactly as
//! wide as the value it currently shows, so each row hugs its text; here they
//! are right-aligned against the panel's edge.
//!
//! Tab moves between rows. Enter, Space, an arrow, or a click advances the
//! focused one, wrapping at both ends.

use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin},
    style::Style,
    text::Line,
};
use ratcn::text_width;
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

            // Each row: the setting's name at the left edge, and the cycle
            // right-aligned, hugging its current value.
            let settings = [
                ("Text size", "size", SIZES[state.size], row_a),
                ("Refresh", "cadence", CADENCES[state.cadence], row_b),
                ("Key map", "keymap", KEYMAPS[state.keymap], row_c),
            ];

            for (name, id, value, row_area) in settings {
                let value_width = text_width::display_width_u16(value);
                let [label_area, value_area] =
                    Layout::horizontal([Constraint::Min(1), Constraint::Length(value_width)])
                        .areas(row_area);

                ctx.paint_widget(
                    Line::from(name).style(Style::default().fg(theme.foreground)),
                    label_area,
                );
                let cycle = match id {
                    "size" => Cycle::new(SIZES).selection(|s: &AppState| s.size, Msg::Size),
                    "cadence" => {
                        Cycle::new(CADENCES).selection(|s: &AppState| s.cadence, Msg::Cadence)
                    }
                    _ => Cycle::new(KEYMAPS).selection(|s: &AppState| s.keymap, Msg::Keymap),
                };
                ctx.component(id, cycle, value_area);
            }
        });
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
