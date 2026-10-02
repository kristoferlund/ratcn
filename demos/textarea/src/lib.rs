//! A notes field: one [`TextArea`], a live count beneath it, and a Save button.
//!
//! The text is a [`TextAreaState`] in app state. A keystroke emits the next
//! state and `update` stores it; the count under the field is read straight
//! from that state each frame, so it can never disagree with what is shown.
//!
//! The field wraps a line longer than it is wide and scrolls to keep the
//! cursor in view. Enter is a line break here, so saving is Ctrl+Enter. A
//! terminal reports that chord only under a keyboard protocol this host does
//! not turn on, so the form also has a button; Tab moves between the two.
//!
//! The mouse works as in any editor: a click places the cursor, a drag
//! selects, and the wheel scrolls the text. A paste keeps its line breaks.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    text::Line,
};
use ratcn::{
    Button, TextArea, TextAreaState, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, TabWrap},
    text_width::display_width_u16,
};

const DEMO_WIDTH: u16 = 48;
const DEMO_HEIGHT: u16 = 13;
const CONTENT_PADDING: Margin = Margin::new(2, 1);
const SAVED: &str = "Saved";

#[derive(Default)]
struct AppState {
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
        match msg {
            Msg::Focus(focus) => self.state.focus = focus,
            Msg::Notes(notes) => self.state.notes = notes,
            Msg::Save => self.state.saved = Some(self.state.notes.value()),
        }
    }
}

impl demo_shared::Demo for App {
    /// A paste reaches the field as one event, line breaks and all.
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

            // The field takes every row the footer and help line leave it.
            let [notes_area, _, footer, help_area] = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .areas(demo.inner(CONTENT_PADDING));

            ctx.component(
                "notes",
                TextArea::new()
                    .value(|s: &AppState| &s.notes, Msg::Notes)
                    .title("Notes")
                    .placeholder("What happened today?")
                    .on_submit(|| Msg::Save),
                notes_area,
            );

            let save = Button::new("Save").on_press(|| Msg::Save);
            let [count_area, saved_area, save_area] = Layout::horizontal([
                Constraint::Fill(1),
                Constraint::Length(display_width_u16(SAVED)),
                Constraint::Length(save.width()),
            ])
            .spacing(1)
            .areas(footer);

            let lines = state.notes.lines();
            let characters: usize = lines.iter().map(|line| line.chars().count()).sum();
            ctx.paint_widget(
                Line::from(format!("Lines: {} · Chars: {characters}", lines.len()))
                    .style(theme.muted_foreground),
                count_area,
            );
            // "Saved" is a claim about the text on screen, so it is checked
            // against it each frame.
            if state.saved.as_deref() == Some(state.notes.value().as_str()) {
                ctx.paint_widget(Line::from(SAVED).style(theme.foreground), saved_area);
            }
            ctx.component("save", save, save_area);

            ctx.paint_widget(
                Line::from("Tab to Save, Ctrl+Enter if supported").style(theme.muted_foreground),
                help_area,
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use demo_shared::Demo as _;
    use ratcn::runtime::{KeyCode, KeyEvent, Modifiers};

    use super::*;

    const SCREEN: Rect = Rect::new(0, 0, 60, 16);

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

    fn send(app: &mut App, event: Event) {
        assert!(app.handle_event(event.clone()), "{event:?} changed nothing");
        screen(app);
    }

    fn press(app: &mut App, code: KeyCode) {
        send(app, Event::Key(KeyEvent::new(code)));
    }

    fn type_text(app: &mut App, text: &str) {
        for char in text.chars() {
            press(app, KeyCode::Char(char));
        }
    }

    fn ctrl_enter() -> Event {
        Event::Key(KeyEvent {
            code: KeyCode::Enter,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
        })
    }

    fn shows(screen: &[String], text: &str) -> bool {
        screen.iter().any(|row| row.contains(text))
    }

    /// Enter is text in a multi-line field, and the count is read from the
    /// same state the field paints, so the two move together.
    #[test]
    fn enter_breaks_the_line_and_the_count_follows() {
        let mut app = app();
        assert!(shows(&screen(&mut app), "Lines: 1 · Chars: 0"));

        type_text(&mut app, "Met Ada");
        press(&mut app, KeyCode::Enter);
        type_text(&mut app, "She counts");

        let screen = screen(&mut app);
        assert!(shows(&screen, "│ Met Ada "), "{screen:#?}");
        assert!(shows(&screen, "│ She counts "), "{screen:#?}");
        assert!(shows(&screen, "Lines: 2 · Chars: 17"), "{screen:#?}");
    }

    /// A line longer than the field breaks over rows on screen and stays one
    /// line in the text: wrapping is paint, not an edit.
    #[test]
    fn a_long_line_wraps_on_screen_and_stays_one_line() {
        let mut app = app();

        type_text(
            &mut app,
            "The quick brown fox jumps over the lazy dog, twice over.",
        );

        let screen = screen(&mut app);
        assert!(shows(&screen, "│ The quick brown fox"), "{screen:#?}");
        assert!(shows(&screen, "│ dog, twice over."), "{screen:#?}");
        assert!(shows(&screen, "Lines: 1 · Chars: 56"), "{screen:#?}");
    }

    /// This host never receives Ctrl+Enter, but a terminal that reports it
    /// (and the browser) does: there it saves without also splitting the
    /// line. "Saved" is a claim about the text on screen, so one more
    /// keystroke makes it false.
    #[test]
    fn a_reported_ctrl_enter_saves_and_the_next_edit_is_unsaved() {
        let mut app = app();
        type_text(&mut app, "note");

        send(&mut app, ctrl_enter());
        assert_eq!(app.state.notes.lines(), ["note"]);
        assert_eq!(app.state.saved.as_deref(), Some("note"));
        assert!(shows(&screen(&mut app), "Saved"));

        type_text(&mut app, "s");
        assert!(!shows(&screen(&mut app), "Saved"));
    }

    /// The button is the way to save where Ctrl+Enter never arrives, so it
    /// has to be reachable from the field, and the screen has to say so.
    #[test]
    fn the_help_line_says_how_to_reach_the_save_button() {
        let mut app = app();
        type_text(&mut app, "note");
        assert!(shows(&screen(&mut app), "Tab to Save"));

        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Enter);

        assert_eq!(app.state.saved.as_deref(), Some("note"));
        assert_eq!(app.state.notes.lines(), ["note"], "Enter pressed Save");
    }

    /// On a small screen the field gives up rows and the footer keeps its
    /// own: the count, "Saved", the button and the help line all still read.
    #[test]
    fn a_small_screen_keeps_the_footer_whole() {
        let mut app = app();
        type_text(&mut app, "The quick brown fox jumps over the lazy dog");
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Enter);

        let screen = screen_of(&mut app, Rect::new(0, 0, 40, 12));

        for text in [
            "┌Notes",
            "Lines: 1 · Chars: 43",
            "Saved",
            "Save ",
            "Tab to Save, Ctrl+Enter if supported",
        ] {
            assert!(shows(&screen, text), "{text} is missing: {screen:#?}");
        }
    }

    /// The host delivers pastes because the demo asks for them, and a text
    /// area keeps the lines a paste arrives with.
    #[test]
    fn a_paste_keeps_its_line_breaks() {
        const { assert!(<App as demo_shared::Demo>::PASTE) };
        let mut app = app();

        send(&mut app, Event::Paste("one\r\ntwo\nthree".to_owned()));

        assert_eq!(app.state.notes.lines(), ["one", "two", "three"]);
    }
}
