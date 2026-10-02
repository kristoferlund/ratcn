//! A sign-up form: three [`Input`]s, two of them checked and one masked.
//!
//! Each field's text is an [`InputState`] in app state. A keystroke emits the
//! next state and `update` stores it, which is the whole of the wiring; the
//! field keeps nothing between frames that the app would miss.
//!
//! What counts as valid is the app's call, and so is when to say so. Nothing
//! is flagged while the form is being filled in. Enter on an incomplete form
//! marks it attempted and moves focus to the first field that needs work;
//! from then on each failing field is drawn `invalid`, with a hint beneath it
//! saying what it needs. The password is masked on screen only — the state
//! holds what was typed.
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
const DEMO_HEIGHT: u16 = 17;
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
    /// Whether a sign-up has been turned down. Until then no field is
    /// flagged: an address half typed is not yet wrong.
    attempted: bool,
    /// The name and email the last sign-up sent.
    signed_up: Option<(String, String)>,
}

impl AppState {
    /// What the name field needs, if anything.
    fn name_problem(&self) -> Option<&'static str> {
        self.name
            .value()
            .trim()
            .is_empty()
            .then_some("Enter your name.")
    }

    /// What the email field needs, if anything.
    fn email_problem(&self) -> Option<&'static str> {
        let email = self.email.value();
        if email.is_empty() {
            Some("Enter an email address.")
        } else if !plausible_email(email) {
            Some("That is not an email address.")
        } else {
            None
        }
    }
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
            Msg::Submit => {
                let first_failing = [
                    (ids::NAME, state.name_problem()),
                    (ids::EMAIL, state.email_problem()),
                ]
                .into_iter()
                .find_map(|(id, problem)| problem.map(|_| id));
                match first_failing {
                    // Send the user to the first field that needs them.
                    Some(id) => {
                        state.attempted = true;
                        state.focus = FocusState::intent([id]);
                    }
                    None => {
                        let sent = (
                            state.name.value().to_owned(),
                            state.email.value().to_owned(),
                        );
                        state.signed_up = Some(sent);
                    }
                }
            }
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
        let name_problem = state.name_problem().filter(|_| state.attempted);
        let email_problem = state.email_problem().filter(|_| state.attempted);

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
            // Each has a row beneath it for its hint. On a short screen the
            // footer gives way first, then the hints; the fields keep their
            // rows the longest.
            let [
                name_area,
                name_hint,
                email_area,
                email_hint,
                password_area,
                _,
                footer,
            ] = Layout::vertical([
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .areas(demo.inner(CONTENT_PADDING));

            ctx.component(
                ids::NAME,
                Input::new()
                    .value(|s: &AppState| &s.name, Msg::Name)
                    .title("Name")
                    .placeholder("Ada Lovelace")
                    .invalid(name_problem.is_some())
                    .on_submit(|| Msg::Submit),
                name_area,
            );

            ctx.component(
                ids::EMAIL,
                Input::new()
                    .value(|s: &AppState| &s.email, Msg::Email)
                    .title("Email")
                    .placeholder("ada@example.com")
                    .invalid(email_problem.is_some())
                    .on_submit(|| Msg::Submit),
                email_area,
            );

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

            // A hint starts under the field's text, one column in from its
            // border.
            for (problem, hint_area) in [(name_problem, name_hint), (email_problem, email_hint)] {
                if let Some(problem) = problem {
                    ctx.paint_widget(
                        Line::from(problem).style(theme.destructive),
                        hint_area.inner(Margin::new(1, 0)),
                    );
                }
            }

            // "Signed up" holds only while the fields still say what was sent.
            let signed_up = state
                .signed_up
                .as_ref()
                .filter(|(name, email)| name == state.name.value() && email == state.email.value());
            let [status_area, help_area] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(footer);
            if let Some((name, email)) = signed_up {
                ctx.paint_widget(
                    Text::from(vec![
                        Line::from(format!("Signed up {name}.")).style(theme.foreground),
                        Line::from(email.clone()).style(theme.muted_foreground),
                    ]),
                    status_area,
                );
            }
            ctx.paint_widget(
                Line::from("Tab between fields, Enter to sign up.").style(theme.muted_foreground),
                help_area,
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use demo_shared::Demo as _;
    use ratcn::runtime::{KeyCode, KeyEvent};

    use super::*;

    const SCREEN: Rect = Rect::new(0, 0, 60, 20);
    const NAME_HINT: &str = "Enter your name.";
    const EMAIL_HINT: &str = "Enter an email address.";

    /// The app with its first frame drawn: the runtime routes nothing before
    /// one.
    fn app() -> App {
        let mut app = App::new();
        screen(&mut app);
        app
    }

    /// Draw a frame and hand back its rows.
    fn screen(app: &mut App) -> Vec<String> {
        screen_of(app, SCREEN)
    }

    fn screen_of(app: &mut App, area: Rect) -> Vec<String> {
        let mut buffer = Buffer::empty(area);
        app.draw(&mut buffer, area, &Theme::default_dark());
        buffer
            .content
            .chunks(area.width.into())
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

    /// A field half filled in is work in progress, not a mistake: nothing is
    /// flagged until a sign-up is turned down.
    #[test]
    fn nothing_is_flagged_until_a_sign_up_is_attempted() {
        let mut app = app();
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "a");
        assert!(!shows(&screen(&mut app), "That is not"));

        press(&mut app, KeyCode::Enter);

        let screen = screen(&mut app);
        assert!(shows(&screen, NAME_HINT), "{screen:#?}");
        assert!(
            shows(&screen, "That is not an email address."),
            "{screen:#?}"
        );
    }

    /// An empty sign-up names every missing field and takes the user to the
    /// first of them.
    #[test]
    fn an_empty_sign_up_is_turned_down_and_focus_goes_to_the_name() {
        let mut app = app();
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);

        press(&mut app, KeyCode::Enter);

        assert!(app.state.signed_up.is_none());
        let screen = screen(&mut app);
        assert!(shows(&screen, NAME_HINT), "{screen:#?}");
        assert!(shows(&screen, EMAIL_HINT), "{screen:#?}");
        type_text(&mut app, "x");
        assert_eq!(app.state.name.value(), "x", "typing now goes to the name");
    }

    /// The name is required too, and once a sign-up has been attempted a
    /// hint goes the moment its field is fixed.
    #[test]
    fn a_sign_up_without_a_name_is_turned_down_until_one_is_typed() {
        let mut app = app();
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "ada@example.com");

        press(&mut app, KeyCode::Enter);
        assert!(app.state.signed_up.is_none());
        assert!(shows(&screen(&mut app), NAME_HINT));

        type_text(&mut app, "Ada");
        assert!(!shows(&screen(&mut app), NAME_HINT));
        press(&mut app, KeyCode::Enter);
        assert!(shows(&screen(&mut app), "Signed up Ada."));
    }

    /// A hint has a row of its own, between its field and the next, and
    /// starts under the field's text rather than against the border.
    #[test]
    fn a_hint_sits_on_its_own_row_under_its_field() {
        let mut app = app();
        press(&mut app, KeyCode::Enter);

        let screen = screen(&mut app);
        let hint_row = screen.iter().position(|row| row.contains(NAME_HINT));
        let hint_row = hint_row.expect("the name hint is shown");
        let border_column = screen[hint_row - 1].find('└').unwrap();
        assert_eq!(screen[hint_row].find(NAME_HINT), Some(border_column + 1));
        assert!(screen[hint_row + 1].contains("┌Email"), "{screen:#?}");
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

    /// The summary says who signed up, and stops saying it once the fields
    /// no longer hold what was sent.
    #[test]
    fn enter_signs_up_and_the_summary_lasts_until_the_next_edit() {
        let mut app = app();
        type_text(&mut app, "Ada");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "ada@example.com");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "hunter2");

        press(&mut app, KeyCode::Enter);

        let summary = screen(&mut app);
        assert!(shows(&summary, "Signed up Ada."), "{summary:#?}");
        assert!(
            summary.iter().any(|row| row.trim() == "ada@example.com"),
            "{summary:#?}"
        );
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "!");
        assert!(!shows(&screen(&mut app), "Signed up"));
    }

    /// On a short screen the help line and the hints give way; the three
    /// fields are what the demo is for, so they stay whole.
    #[test]
    fn a_short_screen_keeps_every_field() {
        let mut app = app();
        press(&mut app, KeyCode::Enter);

        let screen = screen_of(&mut app, Rect::new(0, 0, 40, 12));

        for title in ["┌Name", "┌Email", "┌Password"] {
            assert!(shows(&screen, title), "{title} is missing: {screen:#?}");
        }
        assert_eq!(screen.iter().filter(|row| row.contains('└')).count(), 3);
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
