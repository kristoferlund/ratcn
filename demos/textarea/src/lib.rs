//! A notes field: one [`TextArea`] with a line of stats beneath it.
//!
//! The text is a [`TextAreaState`] in app state: a keystroke emits the next
//! state and `update` stores it. The stats are read from that same state each
//! frame.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    style::Style,
    text::Line,
};
use ratcn::{
    TextArea, TextAreaState, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn},
};

#[derive(Default)]
struct State {
    focus: FocusState,
    notes: TextAreaState,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Notes(TextAreaState),
}

pub struct App {
    state: State,
    ratcn: Ratcn<State, Msg>,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: State::default(),
            ratcn: Ratcn::new().focus(|s: &State| &s.focus, Msg::Focus),
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(focus) => self.state.focus = focus,
            Msg::Notes(notes) => self.state.notes = notes,
        }
    }
}

impl demo_shared::Demo for App {
    /// Pastes, and a browser's copy and cut, reach the field.
    const CLIPBOARD: bool = true;

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

    /// What a copy or cut in the field put on the clipboard.
    fn take_clipboard(&mut self) -> Option<String> {
        self.ratcn.take_clipboard()
    }

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        buffer.set_style(area, Style::default().bg(theme.background));
        let [column] = Layout::horizontal([Constraint::Length(44)])
            .flex(Flex::Center)
            .areas(area);
        let [notes_area, stats_area] =
            Layout::vertical([Constraint::Length(8), Constraint::Length(1)])
                .spacing(1)
                .flex(Flex::Center)
                .areas(column);

        let lines = self.state.notes.lines();
        let chars: usize = lines.iter().map(|line| line.chars().count()).sum();
        let stats = format!("{chars} chars · {} rows", lines.len());

        self.ratcn
            .render_into(buffer, area, &self.state, theme, |ctx| {
                ctx.component(
                    "notes",
                    TextArea::new()
                        .value(|s: &State| &s.notes, Msg::Notes)
                        .title("Notes")
                        .placeholder("What happened today?"),
                    notes_area,
                );
                ctx.paint_widget(
                    Line::from(stats.clone()).style(theme.muted_foreground),
                    stats_area,
                );
            });
    }
}

#[cfg(test)]
mod tests {
    use demo_shared::Demo as _;
    use ratcn::runtime::{KeyCode, KeyEvent};

    use super::*;

    /// The app with a frame drawn: the runtime routes nothing before one.
    fn draw(app: &mut App) -> String {
        let area = Rect::new(0, 0, 60, 16);
        let mut buffer = Buffer::empty(area);
        app.draw(&mut buffer, area, &Theme::default_dark());
        buffer.content.iter().map(|cell| cell.symbol()).collect()
    }

    fn press(app: &mut App, code: KeyCode) {
        assert!(app.handle_event(Event::Key(KeyEvent::new(code))));
        draw(app);
    }

    #[test]
    fn enter_breaks_the_line_and_the_stats_follow() {
        let mut app = App::new();
        assert!(draw(&mut app).contains("0 chars · 1 rows"));

        "Met Ada"
            .chars()
            .for_each(|c| press(&mut app, KeyCode::Char(c)));
        press(&mut app, KeyCode::Enter);
        "She counts"
            .chars()
            .for_each(|c| press(&mut app, KeyCode::Char(c)));

        assert_eq!(app.state.notes.lines(), ["Met Ada", "She counts"]);
        assert!(draw(&mut app).contains("17 chars · 2 rows"));
    }
}
