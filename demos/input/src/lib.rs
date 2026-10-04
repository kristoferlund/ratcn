//! A sign-up form: three [`Input`]s, one of them masked.
//!
//! Each field's text is an [`InputState`] in app state: a keystroke emits the
//! next state and `update` stores it. Tab moves between the fields, and Enter
//! signs up from any of them.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    style::Style,
    text::Line,
};
use ratcn::{
    Input, InputState, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, TabWrap},
};

#[derive(Default)]
struct State {
    focus: FocusState,
    name: InputState,
    email: InputState,
    password: InputState,
    /// Who the last sign-up was for.
    signed_up: Option<String>,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Name(InputState),
    Email(InputState),
    Password(InputState),
    SignUp,
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
            Msg::Name(name) => self.state.name = name,
            Msg::Email(email) => self.state.email = email,
            Msg::Password(password) => self.state.password = password,
            Msg::SignUp => self.state.signed_up = Some(self.state.name.value().to_owned()),
        }
    }
}

impl demo_shared::Demo for App {
    /// Pastes, and a browser's copy and cut, reach the focused field.
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

    /// What a copy or cut in a field put on the clipboard.
    fn take_clipboard(&mut self) -> Option<String> {
        self.ratcn.take_clipboard()
    }

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        buffer.set_style(area, Style::default().bg(theme.background));
        let [column] = Layout::horizontal([Constraint::Length(40)])
            .flex(Flex::Center)
            .areas(area);
        let [name_area, email_area, password_area, status_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .spacing(1)
        .flex(Flex::Center)
        .areas(column);

        let state = &self.state;
        let email = state.email.value();
        let email_invalid = !email.is_empty() && !email.contains('@');

        self.ratcn.render_into(buffer, area, state, theme, |ctx| {
            ctx.component(
                "name",
                Input::new()
                    .value(|s: &State| &s.name, Msg::Name)
                    .title("Name")
                    .placeholder("Ada Lovelace")
                    .on_submit(|| Msg::SignUp),
                name_area,
            );
            ctx.component(
                "email",
                Input::new()
                    .value(|s: &State| &s.email, Msg::Email)
                    .title("Email")
                    .placeholder("ada@example.com")
                    .invalid(email_invalid)
                    .on_submit(|| Msg::SignUp),
                email_area,
            );
            ctx.component(
                "password",
                Input::new()
                    .value(|s: &State| &s.password, Msg::Password)
                    .title("Password")
                    .mask_char('•')
                    .on_submit(|| Msg::SignUp),
                password_area,
            );

            let status = match &state.signed_up {
                Some(name) => Line::from(format!("Signed up {name}")).style(theme.foreground),
                None => {
                    Line::from("Tab between fields, Enter to sign up").style(theme.muted_foreground)
                }
            };
            ctx.paint_widget(status, status_area);
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
        let area = Rect::new(0, 0, 60, 20);
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
    fn tab_moves_between_fields_and_enter_signs_up() {
        let mut app = App::new();
        draw(&mut app);

        type_text(&mut app, "Ada");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "ada@example.com");
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.state.email.value(), "ada@example.com");
        assert!(draw(&mut app).contains("Signed up Ada"));
    }

    #[test]
    fn the_password_is_masked_on_screen_and_kept_in_state() {
        let mut app = App::new();
        draw(&mut app);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);

        type_text(&mut app, "hunter2");

        let screen = draw(&mut app);
        assert!(screen.contains("•••••••"));
        assert!(!screen.contains("hunter2"));
        assert_eq!(app.state.password.value(), "hunter2");
    }
}
