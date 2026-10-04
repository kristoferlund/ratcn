//! A notes field: one [`TextArea`] and a Save button.
//!
//! The text is a [`TextAreaState`] in app state: a keystroke emits the next
//! state and `update` stores it. Enter is a line break, so saving is the
//! button, or Ctrl+Enter from the field. A terminal sends Ctrl+Enter as Ctrl+J,
//! which the field takes for submit too.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    style::Style,
    text::Line,
};
use ratcn::{
    Button, TextArea, TextAreaState, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, TabWrap},
};

// A browser opens its downloads on Ctrl+J, but passes Ctrl+Enter on.
const HELP: &str = if cfg!(target_arch = "wasm32") {
    "Ctrl+Enter or Tab to Save"
} else {
    "Ctrl+J or Tab to Save"
};

#[derive(Default)]
struct State {
    focus: FocusState,
    notes: TextAreaState,
    /// The text as it was last saved.
    saved: Option<String>,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Notes(TextAreaState),
    Save,
}

pub struct App {
    state: State,
    ratcn: Ratcn<State, Msg>,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: State::default(),
            ratcn: Ratcn::new()
                .focus(|s: &State| &s.focus, Msg::Focus)
                .tab_wrap(TabWrap::Wrap),
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(focus) => self.state.focus = focus,
            Msg::Notes(notes) => self.state.notes = notes,
            Msg::Save => self.state.saved = Some(self.state.notes.value()),
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
        let [notes_area, footer] = Layout::vertical([Constraint::Length(8), Constraint::Length(1)])
            .spacing(1)
            .flex(Flex::Center)
            .areas(column);

        let state = &self.state;
        self.ratcn.render_into(buffer, area, state, theme, |ctx| {
            ctx.component(
                "notes",
                TextArea::new()
                    .value(|s: &State| &s.notes, Msg::Notes)
                    .title("Notes")
                    .placeholder("What happened today?")
                    .on_submit(|| Msg::Save),
                notes_area,
            );

            let save = Button::new("Save").on_press(|| Msg::Save);
            let [status_area, save_area] =
                Layout::horizontal([Constraint::Fill(1), Constraint::Length(save.width())])
                    .areas(footer);
            let saved = state.saved.as_deref() == Some(state.notes.value().as_str());
            let status = if saved {
                Line::from("Saved").style(theme.foreground)
            } else {
                Line::from(HELP).style(theme.muted_foreground)
            };
            ctx.paint_widget(status, status_area);
            ctx.component("save", save, save_area);
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

    fn type_text(app: &mut App, text: &str) {
        text.chars()
            .for_each(|char| press(app, KeyCode::Char(char)));
    }

    #[test]
    fn enter_breaks_the_line_and_the_button_saves() {
        let mut app = App::new();
        draw(&mut app);

        type_text(&mut app, "Met Ada");
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "She counts");
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.state.notes.lines(), ["Met Ada", "She counts"]);
        assert!(draw(&mut app).contains("Saved"));
    }
}
