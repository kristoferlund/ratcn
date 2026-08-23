//! Settings rows: the setting's name on the left, a [`Cycle`] right-aligned.
//!
//! The pattern most settings screens want — each row reads "name: value", and
//! the value cycles in place through its options. Tab moves between rows;
//! Enter, Space, an arrow, or a click advances the focused one.

use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::Style,
};
use ratcn::{
    Cycle, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn},
};

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
                .tab_wrap(ratcn::runtime::TabWrap::Wrap),
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
            // One column per row, split into label on the left and cycle on
            // the right, with the widest option reserved so every value has
            // the same hit target.
            let span = SIZES
                .iter()
                .chain(&CADENCES)
                .chain(&KEYMAPS)
                .map(|option| option.len() as u16)
                .max()
                .unwrap_or(0);
            let [rows_area] = Layout::vertical([Constraint::Length(3)])
                .flex(Flex::Center)
                .areas(area);

            for (row, (name, id)) in [
                ("Text size", "size"),
                ("Refresh cadence", "cadence"),
                ("Key map", "keymap"),
            ]
            .into_iter()
            .enumerate()
            {
                let row_area = Rect::new(rows_area.x, rows_area.y + row as u16, rows_area.width, 1);
                let [label_area, value_area] =
                    Layout::horizontal([Constraint::Min(1), Constraint::Length(span + 2)])
                        .areas(row_area);

                ctx.paint_widget(
                    ratatui::text::Line::from(format!("{name}:"))
                        .style(Style::default().fg(theme.foreground)),
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
