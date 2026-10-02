//! A sign-up form: three [`Input`]s, one validated and one masked.
//!
//! Each field's text is an [`InputState`] in app state. A keystroke emits the
//! next state and `update` stores it, which is the whole of the wiring; the
//! field keeps nothing between frames that the app would miss.
//!
//! What counts as a valid address is the app's call: the email field is told
//! it is `invalid` while its text is not a plausible one, and a hint is
//! painted beneath it. The password is masked on screen only — the state holds
//! what was typed.
//!
//! Tab moves between the fields and Enter signs up from any of them. A click
//! places the cursor, a drag selects, and a paste lands in the focused field
//! as one line.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    text::{Line, Text},
};
use ratcn::{
    Input, InputState, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, TabWrap},
};

const DEMO_WIDTH: u16 = 44;
const DEMO_HEIGHT: u16 = 16;
const CONTENT_PADDING: Margin = Margin::new(2, 1);

/// Child ids, named once so declarations and focus jumps can't drift.
mod ids {
    pub const NAME: &str = "name";
    pub const EMAIL: &str = "email";
    pub const PASSWORD: &str = "password";
}

#[derive(Default)]
struct AppState {
    focus: FocusState,
    name: InputState,
    email: InputState,
    password: InputState,
    /// What the last sign-up sent, shown beneath the form.
    submitted: Option<String>,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Name(InputState),
    Email(InputState),
    Password(InputState),
    Submit,
}

/// Whether `email` could be an address: something, an `@`, and a dotted
/// domain. Deliberately loose — the demo is about the look, not about RFC 5322.
fn plausible_email(email: &str) -> bool {
    email.split_once('@').is_some_and(|(local, domain)| {
        !local.is_empty()
            && domain
                .split_once('.')
                .is_some_and(|(host, rest)| !host.is_empty() && !rest.is_empty())
    })
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
        let state = &mut self.state;
        match msg {
            Msg::Focus(focus) => state.focus = focus,
            Msg::Name(name) => state.name = name,
            Msg::Email(email) => state.email = email,
            Msg::Password(password) => state.password = password,
            Msg::Submit if plausible_email(state.email.value()) => {
                state.submitted = Some(format!(
                    "Signed up {}.\n{}, {}-character password",
                    state.name.value(),
                    state.email.value(),
                    state.password.value().chars().count(),
                ));
            }
            // Nothing to sign up with: send the user back to the address.
            Msg::Submit => state.focus = FocusState::intent([ids::EMAIL]),
        }
    }
}

impl demo_shared::Demo for App {
    /// A paste reaches the focused field as one event.
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

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        buffer.set_style(area, Style::default().bg(theme.background));

        let state = &self.state;
        // An empty field is not wrong yet; it only has nothing in it.
        let email = state.email.value();
        let email_invalid = !email.is_empty() && !plausible_email(email);

        self.ratcn.render_into(buffer, area, state, theme, |ctx| {
            let demo = area.centered(
                Constraint::Length(DEMO_WIDTH),
                Constraint::Length(DEMO_HEIGHT),
            );
            ctx.paint(move |ctx| {
                let surface = ctx.theme.surface;
                ctx.with_buffer(demo, |area, buf| {
                    buf.set_style(area, Style::default().bg(surface));
                });
            });

            // A titled field is three rows: its border, and the text inside.
            // One row separates them, and the email's hint is painted in its.
            let [name_area, email_area, password_area, status_area] = Layout::vertical([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(2),
            ])
            .spacing(1)
            .areas(demo.inner(CONTENT_PADDING));
            let hint_area = Rect::new(email_area.x, email_area.bottom(), email_area.width, 1);

            ctx.component(
                ids::NAME,
                Input::new()
                    .value(|s: &AppState| &s.name, Msg::Name)
                    .title("Name")
                    .placeholder("Ada Lovelace")
                    .on_submit(|| Msg::Submit),
                name_area,
            );

            ctx.component(
                ids::EMAIL,
                Input::new()
                    .value(|s: &AppState| &s.email, Msg::Email)
                    .title("Email")
                    .placeholder("ada@example.com")
                    .invalid(email_invalid)
                    .on_submit(|| Msg::Submit),
                email_area,
            );
            if email_invalid {
                ctx.paint_widget(
                    Line::from(" That is not an address yet.")
                        .style(Style::default().fg(theme.destructive)),
                    hint_area,
                );
            }

            ctx.component(
                ids::PASSWORD,
                Input::new()
                    .value(|s: &AppState| &s.password, Msg::Password)
                    .title("Password")
                    .placeholder("Choose a password")
                    .mask_char('•')
                    .on_submit(|| Msg::Submit),
                password_area,
            );

            let status = match &state.submitted {
                Some(submitted) => Text::from(submitted.clone()).style(theme.foreground),
                None => Text::from("Tab between fields, Enter to sign up.")
                    .style(theme.muted_foreground),
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

    const SCREEN: Rect = Rect::new(0, 0, 60, 20);

    /// The app with its first frame drawn: the runtime routes nothing before
    /// one.
    fn app() -> App {
        let mut app = App::new();
        screen(&mut app);
        app
    }

    /// Draw a frame and hand back its rows.
    fn screen(app: &mut App) -> Vec<String> {
        let mut buffer = Buffer::empty(SCREEN);
        app.draw(&mut buffer, SCREEN, &Theme::default_dark());
        buffer
            .content
            .chunks(SCREEN.width.into())
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect()
    }

    fn press(app: &mut App, code: KeyCode) {
        assert!(
            app.handle_event(Event::Key(KeyEvent::new(code))),
            "{code:?} changed nothing"
        );
        screen(app);
    }

    fn type_text(app: &mut App, text: &str) {
        for char in text.chars() {
            press(app, KeyCode::Char(char));
        }
    }

    fn shows(screen: &[String], text: &str) -> bool {
        screen.iter().any(|row| row.contains(text))
    }

    /// The form's reason to exist: what is typed lands in the focused field
    /// and nowhere else, and Tab is the form's key, not the field's.
    #[test]
    fn typing_fills_the_focused_field_and_tab_moves_to_the_next() {
        let mut app = app();

        type_text(&mut app, "Ada");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "ada@example.com");

        assert_eq!(app.state.name.value(), "Ada");
        assert_eq!(app.state.email.value(), "ada@example.com");
        let screen = screen(&mut app);
        assert!(shows(&screen, "│Ada "), "{screen:#?}");
        assert!(shows(&screen, "│ada@example.com "), "{screen:#?}");
    }

    /// The hint follows the text, keystroke by keystroke: an address that is
    /// half typed is flagged, and the flag goes the moment it is whole. An
    /// empty field is not an error.
    #[test]
    fn the_email_hint_shows_only_while_the_address_is_not_plausible() {
        const HINT: &str = "not an address yet";
        let mut app = app();
        press(&mut app, KeyCode::Tab);
        assert!(!shows(&screen(&mut app), HINT), "empty is not invalid");

        type_text(&mut app, "ada@example");
        assert!(shows(&screen(&mut app), HINT));

        type_text(&mut app, ".com");
        assert!(!shows(&screen(&mut app), HINT));
    }

    /// A mask that leaked the text into the buffer would be no mask, and one
    /// that changed the state would lose the password.
    #[test]
    fn the_password_is_masked_on_screen_and_kept_in_state() {
        let mut app = app();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);

        type_text(&mut app, "hunter2");

        let screen = screen(&mut app);
        assert!(shows(&screen, "│••••••• "), "{screen:#?}");
        assert!(!shows(&screen, "hunter2"));
        assert_eq!(app.state.password.value(), "hunter2");
    }

    #[test]
    fn enter_signs_up_with_what_the_fields_hold() {
        let mut app = app();
        type_text(&mut app, "Ada");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "ada@example.com");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "hunter2");

        press(&mut app, KeyCode::Enter);

        let screen = screen(&mut app);
        assert!(shows(&screen, "Signed up Ada."), "{screen:#?}");
        assert!(
            shows(&screen, "ada@example.com, 7-character password"),
            "{screen:#?}"
        );
    }

    /// Enter with no usable address must not report a sign-up; it takes the
    /// user to the field that needs them.
    #[test]
    fn enter_without_an_address_moves_focus_to_the_email_field() {
        let mut app = app();
        type_text(&mut app, "Ada");

        press(&mut app, KeyCode::Enter);

        assert!(app.state.submitted.is_none());
        type_text(&mut app, "x");
        assert_eq!(app.state.email.value(), "x", "typing now goes to the email");
    }

    /// The host delivers pastes because the demo asks for them, and a field
    /// holds one line whatever the clipboard held.
    #[test]
    fn a_paste_lands_in_the_focused_field_as_one_line() {
        const { assert!(<App as demo_shared::Demo>::PASTE) };
        let mut app = app();

        assert!(app.handle_event(Event::Paste("Ada\nLovelace".to_owned())));

        assert_eq!(app.state.name.value(), "Ada Lovelace");
    }
}
