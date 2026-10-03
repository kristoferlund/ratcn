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
//! as one line. Ctrl+C and Ctrl+X copy and cut a selection to the system
//! clipboard, except from the masked password.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    text::{Line, Text},
};
use ratcn::{
    Input, InputState, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, TabWrap},
    text_width::display_width,
};

const DEMO_WIDTH: u16 = 44;
const DEMO_HEIGHT: u16 = 18;
const CONTENT_PADDING: Margin = Margin::new(2, 1);
const HELP: &str = "Tab between fields, Enter to sign up";

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
    /// How many fields, from the top, the last frame had room for.
    fields_shown: usize,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new()
                .focus(|s: &AppState| &s.focus, Msg::Focus)
                .tab_wrap(TabWrap::Wrap),
            fields_shown: 0,
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
                let failing = [
                    (ids::NAME, state.name_problem()),
                    (ids::EMAIL, state.email_problem()),
                ]
                .map(|(id, problem)| problem.map(|_| id));
                if failing.iter().all(Option::is_none) {
                    let sent = (
                        state.name.value().to_owned(),
                        state.email.value().to_owned(),
                    );
                    state.signed_up = Some(sent);
                    return;
                }
                state.attempted = true;
                // Send the user to the first field that needs them, if the
                // screen has room for it: focus on a field that is not shown
                // would swallow what is typed next.
                if let Some(id) = failing.into_iter().take(self.fields_shown).flatten().next() {
                    state.focus = FocusState::intent([id]);
                }
            }
        }
    }
}

impl demo_shared::Demo for App {
    /// A paste reaches the focused field as one event, and so do a browser's
    /// copy and cut.
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

        let state = &self.state;
        let name_problem = state.name_problem().filter(|_| state.attempted);
        let email_problem = state.email_problem().filter(|_| state.attempted);

        let mut fields_shown = 0;
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

            let name = Input::new()
                .value(|s: &AppState| &s.name, Msg::Name)
                .title("Name")
                .placeholder("Ada Lovelace")
                .invalid(name_problem.is_some())
                .on_submit(|| Msg::Submit);
            let email = Input::new()
                .value(|s: &AppState| &s.email, Msg::Email)
                .title("Email")
                .placeholder("ada@example.com")
                .invalid(email_problem.is_some())
                .on_submit(|| Msg::Submit);
            let password = Input::new()
                .value(|s: &AppState| &s.password, Msg::Password)
                .title("Password")
                .placeholder("Choose a password")
                .mask_char('•')
                .on_submit(|| Msg::Submit);

            // Each field has a row beneath it: a hint's, or the gap before the
            // footer. On a short screen the padding gives way first, then the
            // footer, then those rows, all together so the gaps stay even, and
            // then the fields from the bottom: a field is shown whole or not
            // at all.
            let field = name.height();
            let content = if demo.height >= DEMO_HEIGHT {
                demo.inner(CONTENT_PADDING)
            } else {
                demo.inner(Margin::new(CONTENT_PADDING.horizontal, 0))
            };
            let gap = u16::from(content.height >= 3 * (field + 1));
            let shown = (content.height / (field + gap)).min(3);
            fields_shown = usize::from(shown);
            let slot = |index: u16| Rect {
                y: content.y + index * (field + gap),
                height: field,
                ..content
            };
            for (index, (id, input)) in (0..shown).zip([
                (ids::NAME, name),
                (ids::EMAIL, email),
                (ids::PASSWORD, password),
            ]) {
                ctx.component(id, input, slot(index));
            }
            // Without room for the gaps there is none for the footer.
            if gap == 0 {
                return;
            }

            // A hint starts under the field's text: past the border, and past
            // the cell the field leaves before its text. It may run on under
            // the right border, which a narrow screen needs.
            for (index, problem) in [(0, name_problem), (1, email_problem)] {
                if let Some(problem) = problem {
                    let hint_area = Rect {
                        x: content.x + 2,
                        y: slot(index).bottom(),
                        width: content.width.saturating_sub(2),
                        height: 1,
                    };
                    ctx.paint_widget(Line::from(problem).style(theme.destructive), hint_area);
                }
            }

            let [_, footer] =
                Layout::vertical([Constraint::Length(3 * (field + gap)), Constraint::Fill(1)])
                    .areas(content);
            // "Signed up" holds only while the fields still say what was sent.
            let signed_up = state
                .signed_up
                .as_ref()
                .filter(|(name, email)| name == state.name.value() && email == state.email.value());
            // The summary answers what the user just did, so it has its two
            // rows first; the help line has the last row only with one to
            // spare above it, and only if it fits whole.
            let status_rows = if signed_up.is_some() {
                footer.height.min(2)
            } else {
                0
            };
            let help_rows = u16::from(
                footer.height > status_rows + u16::from(status_rows > 0)
                    && display_width(HELP) <= usize::from(footer.width),
            );
            let [status_area, _, help_area] = Layout::vertical([
                Constraint::Length(status_rows),
                Constraint::Fill(1),
                Constraint::Length(help_rows),
            ])
            .areas(footer);
            if let Some((name, email)) = signed_up {
                ctx.paint_widget(
                    Text::from(vec![
                        Line::from(format!("Signed up {name}.")).style(theme.foreground),
                        Line::from(email.clone()).style(theme.muted_foreground),
                    ]),
                    status_area,
                );
            }
            ctx.paint_widget(Line::from(HELP).style(theme.muted_foreground), help_area);
        });
        self.fields_shown = fields_shown;
    }
}

#[cfg(test)]
mod tests {
    use demo_shared::Demo as _;
    use ratcn::runtime::{KeyCode, KeyEvent, Modifiers};

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
        assert!(shows(&screen, "│ Ada "), "{screen:#?}");
        assert!(shows(&screen, "│ ada@example.com "), "{screen:#?}");
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
    /// starts in the column the field's text does, so it reads as part of
    /// the field rather than of the border.
    #[test]
    fn a_hint_sits_on_its_own_row_under_its_field() {
        let mut app = app();
        press(&mut app, KeyCode::Enter);

        let screen = screen(&mut app);
        let hint_row = screen.iter().position(|row| row.contains(NAME_HINT));
        let hint_row = hint_row.expect("the name hint is shown");
        assert!(screen[hint_row - 1].contains('└'), "{screen:#?}");
        let column = |row: &str, text| row.find(text).map(|at| row[..at].chars().count());
        assert_eq!(
            column(&screen[hint_row], NAME_HINT),
            column(&screen[hint_row - 2], "Ada Lovelace"),
            "the hint starts where the placeholder does: {screen:#?}"
        );
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
        assert!(shows(&screen, "│ ••••••• "), "{screen:#?}");
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

    /// On a short screen the padding, the footer, and the hint rows give
    /// way, in that order, and then the fields themselves, from the bottom.
    /// A field is shown whole or not at all: one squeezed shorter than its
    /// border draws nothing and leaves a hole Tab skips. The hint rows go all
    /// together: one field with a gap beneath it and one without would look
    /// like a layout bug, and a field drawn invalid with its hint dropped
    /// would not say what is wrong.
    #[test]
    fn a_short_screen_shows_whole_fields_from_the_top_spaced_evenly() {
        let mut app = app();
        press(&mut app, KeyCode::Enter);

        for width in [30, 40] {
            for height in 1..=20 {
                let screen = screen_of(&mut app, Rect::new(0, 0, width, height));
                let at = format!("{width}x{height}");
                let rows = |symbol| {
                    screen
                        .iter()
                        .enumerate()
                        .filter(|(_, row)| row.contains(symbol))
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>()
                };
                let (tops, bottoms) = (rows('┌'), rows('└'));
                let shown = usize::from(height / 3).min(3);
                assert_eq!(tops.len(), shown, "{at}: {screen:#?}");
                for (index, (top, title)) in tops
                    .iter()
                    .zip(["┌Name", "┌Email", "┌Password"])
                    .enumerate()
                {
                    assert!(screen[*top].contains(title), "{at}: {screen:#?}");
                    assert_eq!(
                        bottoms[index],
                        top + 2,
                        "{at}: a field cut short: {screen:#?}"
                    );
                }
                if shown == 3 {
                    assert_eq!(
                        tops[1] - bottoms[0],
                        tops[2] - bottoms[1],
                        "uneven gaps at {at}: {screen:#?}"
                    );
                }
                let hints = shows(&screen, NAME_HINT) && shows(&screen, EMAIL_HINT);
                if height >= 12 {
                    assert!(hints, "a hint dropped at {at}: {screen:#?}");
                }
                if shows(&screen, "Tab between") {
                    assert!(hints, "help kept over the hints at {at}: {screen:#?}");
                    assert!(
                        screen.iter().any(|row| row.trim() == HELP),
                        "help cut short at {at}: {screen:#?}"
                    );
                }
            }
        }
    }

    /// A turned-down sign-up sends focus to the first field that needs work
    /// only if that field is on screen: focus on a field the screen is too
    /// short for would swallow what is typed next.
    #[test]
    fn a_sign_up_on_a_short_screen_keeps_focus_on_a_shown_field() {
        let tiny = Rect::new(0, 0, 30, 5);
        let mut app = App::new();
        let key = |app: &mut App, code| {
            assert!(
                app.handle_event(Event::Key(KeyEvent::new(code))),
                "{code:?} changed nothing"
            );
            screen_of(app, tiny)
        };
        let screen = screen_of(&mut app, tiny);
        assert!(!shows(&screen, "┌Email"), "{screen:#?}");
        for char in "Ada".chars() {
            key(&mut app, KeyCode::Char(char));
        }

        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char('!'));

        assert!(app.state.signed_up.is_none());
        assert_eq!(app.state.name.value(), "Ada!");
    }

    /// A sign-up's summary is the answer to what the user just did, so on a
    /// short screen it keeps its rows and the static help line gives way.
    #[test]
    fn a_short_screen_drops_the_help_before_the_summary() {
        let mut app = app();
        type_text(&mut app, "Ada");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "ada@example.com");
        press(&mut app, KeyCode::Enter);

        for height in 13..=20 {
            let screen = screen_of(&mut app, Rect::new(0, 0, 40, height));
            assert!(
                shows(&screen, "Signed up Ada."),
                "{height} rows: {screen:#?}"
            );
            if height >= 14 {
                assert!(
                    screen.iter().any(|row| row.trim() == "ada@example.com"),
                    "{height} rows: {screen:#?}"
                );
            }
        }
    }

    /// The summary is two lines of its own, set off from the help line
    /// beneath it.
    #[test]
    fn the_summary_is_set_off_from_the_help_line() {
        let mut app = app();
        type_text(&mut app, "Ada");
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "ada@example.com");
        press(&mut app, KeyCode::Enter);

        let screen = screen(&mut app);
        let email_row = screen
            .iter()
            .position(|row| row.trim() == "ada@example.com")
            .expect("the summary shows the address");
        assert!(
            screen[email_row - 1].contains("Signed up Ada."),
            "{screen:#?}"
        );
        assert_eq!(screen[email_row + 1].trim(), "", "{screen:#?}");
        assert!(
            screen[email_row + 2].contains("Tab between fields"),
            "{screen:#?}"
        );
    }

    /// The host delivers pastes because the demo asks for them, and a field
    /// holds one line whatever the clipboard held.
    #[test]
    fn a_paste_lands_in_the_focused_field_as_one_line() {
        const { assert!(<App as demo_shared::Demo>::CLIPBOARD) };
        let mut app = app();

        assert!(app.handle_event(Event::Paste("Ada\nLovelace".to_owned())));

        assert_eq!(app.state.name.value(), "Ada Lovelace");
    }

    /// A selection in a field is copied for the host to write out, but never
    /// one in the masked password: the copy would leak the secret.
    #[test]
    fn ctrl_c_copies_a_selection_but_never_the_password() {
        let shift_home = Event::Key(KeyEvent {
            code: KeyCode::Home,
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        });
        let ctrl_c = Event::Key(KeyEvent {
            code: KeyCode::Char('c'),
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        });
        let mut app = app();
        type_text(&mut app, "Ada");
        assert!(app.handle_event(shift_home.clone()));
        screen(&mut app);
        assert!(app.handle_event(ctrl_c.clone()));
        assert_eq!(app.take_clipboard().as_deref(), Some("Ada"));

        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Tab);
        type_text(&mut app, "hunter2");
        assert!(app.handle_event(shift_home));
        screen(&mut app);
        assert!(!app.handle_event(ctrl_c), "the copy is left to the host");
        assert_eq!(app.take_clipboard(), None);
    }
}
