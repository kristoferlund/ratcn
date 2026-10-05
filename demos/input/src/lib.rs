//! A sign-up form: three [`Input`]s, one of them masked.
//!
//! Each field's text is an [`InputState`] in app state: a keystroke emits the
//! next state and `update` stores it. Tab moves between the fields, and Enter
//! signs up from any of them.
//!
//! The email and password are checked as soon as they have content, and a
//! failing field is drawn `invalid` with a message beneath it. The email is
//! checked against RFC 5322 by the `email_address` crate.

use email_address::EmailAddress;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Margin, Rect},
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
    /// What the last press of Enter came to.
    submitted: Option<Submitted>,
}

/// The answer to a sign-up. Nothing is sent anywhere: this is a demo.
enum Submitted {
    SignedUp(String),
    Incomplete,
}

impl State {
    /// What is wrong with the email, once it has content.
    fn email_error(&self) -> Option<&'static str> {
        let email = self.email.value();
        (!email.is_empty() && !EmailAddress::is_valid(email)).then_some("Not a valid email address")
    }

    /// What is wrong with the password, once it has content.
    fn password_error(&self) -> Option<&'static str> {
        let length = self.password.value().chars().count();
        (length > 0 && length < 6).then_some("At least 6 characters")
    }

    /// Whether every field is filled in and passes its check.
    fn complete(&self) -> bool {
        [&self.name, &self.email, &self.password]
            .iter()
            .all(|field| !field.value().is_empty())
            && self.email_error().is_none()
            && self.password_error().is_none()
    }
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
            Msg::SignUp => {
                self.state.submitted = Some(if self.state.complete() {
                    Submitted::SignedUp(self.state.name.value().to_owned())
                } else {
                    Submitted::Incomplete
                });
            }
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

    /// What a copy or cut in a field put on the clipboard, for the demo host
    /// to write out. To hook up the clipboard in an app of your own, see
    /// <https://ratcn.com/docs/concepts/host-integration#the-clipboard>.
    fn take_clipboard(&mut self) -> Option<String> {
        self.ratcn.take_clipboard()
    }

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        buffer.set_style(area, Style::default().bg(theme.background));
        let [column] = Layout::horizontal([Constraint::Length(40)])
            .flex(Flex::Center)
            .areas(area);
        // Each field has a row beneath it for its error message.
        let [
            name_area,
            _,
            email_area,
            email_error_area,
            password_area,
            password_error_area,
            _,
            status_area,
        ] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .flex(Flex::Center)
        .areas(column);

        let state = &self.state;
        let email_error = state.email_error();
        let password_error = state.password_error();

        self.ratcn.render_into(buffer, area, state, theme, |ctx| {
            ctx.component(
                "name",
                Input::new()
                    .value(|s: &State| &s.name, Msg::Name)
                    .title("Name")
                    .placeholder("Pablo Picasso")
                    .on_submit(|| Msg::SignUp),
                name_area,
            );
            ctx.component(
                "email",
                Input::new()
                    .value(|s: &State| &s.email, Msg::Email)
                    .title("Email")
                    .placeholder("picasso@louvre.fr")
                    .invalid(email_error.is_some())
                    .on_submit(|| Msg::SignUp),
                email_area,
            );
            ctx.component(
                "password",
                Input::new()
                    .value(|s: &State| &s.password, Msg::Password)
                    .title("Password")
                    .mask_char('•')
                    .invalid(password_error.is_some())
                    .on_submit(|| Msg::SignUp),
                password_area,
            );

            // An error starts under the field's text, past its border.
            for (error, area) in [
                (email_error, email_error_area),
                (password_error, password_error_area),
            ] {
                if let Some(error) = error {
                    ctx.paint_widget(
                        Line::from(error).style(theme.destructive),
                        area.inner(Margin::new(1, 0)),
                    );
                }
            }

            let status = match &state.submitted {
                Some(Submitted::SignedUp(name)) => {
                    Line::from(format!("Signed up {name}")).style(theme.foreground)
                }
                Some(Submitted::Incomplete) => {
                    Line::from("Fill in every field to sign up").style(theme.destructive)
                }
                None => return,
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

        type_text(&mut app, "Pablo");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "ada@example.com");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "hunter2");
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.state.email.value(), "ada@example.com");
        assert!(draw(&mut app).contains("Signed up Pablo"));
    }

    #[test]
    fn errors_show_only_once_a_field_has_content() {
        let mut app = App::new();
        let empty = draw(&mut app);
        assert!(!empty.contains("Not a valid") && !empty.contains("At least"));

        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "ada@");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "abc");
        let screen = draw(&mut app);
        assert!(screen.contains("Not a valid email address"));
        assert!(screen.contains("At least 6 characters"));

        press(&mut app, KeyCode::Enter);
        assert!(draw(&mut app).contains("Fill in every field to sign up"));

        type_text(&mut app, "def");
        assert!(!draw(&mut app).contains("At least"));
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
