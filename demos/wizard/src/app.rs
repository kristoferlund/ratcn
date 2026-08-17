//! The app shell owns orchestration and routes messages to the state owner.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin},
    style::{Color, Style},
};
use ratcn::{
    ButtonSize, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, ScopeOptions, TabWrap},
};

use crate::nav::{self, Nav, NavMsg, Step};
use crate::shared::{ChoiceMsg, Choices};
use crate::steps;

const DEMO_WIDTH: u16 = 60;
/// Sized so the tallest step — the seven-row theme list, with its caption and
/// its code line — fills the panel exactly.
const DEMO_HEIGHT: u16 = 23;
const PADDING_X: u16 = 2;
const PADDING_Y: u16 = 1;

#[derive(Default)]
pub struct AppState {
    pub focus: FocusState,
    pub nav: Nav,
    pub choices: Choices,
    pub backend: steps::backend::State,
    pub theme: steps::theme::State,
}

#[derive(Clone)]
pub enum Msg {
    Focus(FocusState),
    Nav(NavMsg),
    Backend(steps::backend::Msg),
    Theme(steps::theme::Msg),
    Choose(ChoiceMsg),
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
                .focus(|state: &AppState| &state.focus, Msg::Focus)
                .tab_wrap(TabWrap::Wrap),
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(focus) => self.state.focus = focus,
            Msg::Nav(msg) => {
                self.state.nav.update(msg);
                // Park focus on the button that continues, so Enter walks
                // the whole wizard. Without this the default lands on each
                // step's own control instead, and the next Enter opens a
                // panel rather than advancing. Tab still reaches the control.
                self.state.focus = FocusState::intent([if self.state.nav.step.is_last() {
                    nav::BACK_ID
                } else {
                    nav::NEXT_ID
                }]);
            }
            Msg::Backend(msg) => self.state.backend.update(msg),
            Msg::Theme(msg) => self.state.theme.update(msg),
            // A committed choice is two writes: the step closes or moves its
            // cursor, and the shared value the rest of the app reads changes.
            Msg::Choose(ChoiceMsg::SetBackend(backend)) => {
                self.state
                    .backend
                    .update(steps::backend::Msg::Committed(backend));
                self.state.choices.update(ChoiceMsg::SetBackend(backend));
            }
            Msg::Choose(ChoiceMsg::SetTheme(theme)) => {
                self.state.theme.update(steps::theme::Msg::Focused(theme));
                self.state.choices.update(ChoiceMsg::SetTheme(theme));
            }
        }
    }

    /// The palette currently chosen, which every frame renders with.
    pub fn palette(&self) -> Theme {
        self.state.choices.palette()
    }
}

impl demo_shared::Demo for App {
    /// The canvas padding is fixed at construction, so it tracks the starting
    /// theme; choosing another theme leaves it on the previous background.
    fn background(&self) -> Color {
        self.palette().background
    }

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

    fn draw(&mut self, frame: &mut Frame) {
        let theme = self.palette();
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(theme.background));
        let area = area
            .centered(
                Constraint::Length(DEMO_WIDTH),
                Constraint::Length(DEMO_HEIGHT),
            )
            .inner(Margin::new(PADDING_X, PADDING_Y));
        let state = &self.state;
        self.ratcn.render(frame, state, &theme, |ctx| {
            let [stepper, panel, buttons] = area.layout(&shell_layout());
            let step = state.nav.step;

            ctx.paint_widget(nav::stepper(step, ctx.theme), stepper);

            ctx.scope(
                screen_id(step),
                panel,
                ScopeOptions::default(),
                |ctx| match step {
                    Step::Project => steps::project::declare(ctx),
                    Step::Backend => steps::backend::declare(ctx),
                    Step::Theme => steps::theme::declare(ctx),
                    Step::Done => steps::done::declare(ctx),
                },
            );

            nav::declare(ctx, buttons, step);
        });
    }
}

const fn screen_id(step: Step) -> &'static str {
    match step {
        Step::Project => steps::project::SCREEN_ID,
        Step::Backend => steps::backend::SCREEN_ID,
        Step::Theme => steps::theme::SCREEN_ID,
        Step::Done => steps::done::SCREEN_ID,
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

fn shell_layout() -> Layout {
    Layout::vertical([
        Constraint::Length(1),                          // step dots
        Constraint::Min(0),                             // the open step
        Constraint::Length(ButtonSize::Large.height()), // Back / Next
    ])
    .spacing(1)
}

#[cfg(test)]
mod tests {
    use demo_shared::Demo as _;
    use ratatui::{Terminal, backend::TestBackend};
    use ratcn::runtime::{ChildId, KeyCode, KeyEvent};

    use super::*;
    use crate::shared::Backend;

    fn app() -> (App, Terminal<TestBackend>) {
        (
            App::new(),
            Terminal::new(TestBackend::new(DEMO_WIDTH, DEMO_HEIGHT)).expect("terminal"),
        )
    }

    fn draw(app: &mut App, terminal: &mut Terminal<TestBackend>) {
        terminal.draw(|frame| app.draw(frame)).expect("draw");
    }

    fn rendered_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer.cell((column, row)).expect("cell").symbol())
                    .collect()
            })
            .collect()
    }

    fn press(app: &mut App, terminal: &mut Terminal<TestBackend>, code: KeyCode) {
        draw(app, terminal);
        app.handle_event(Event::Key(KeyEvent::new(code)));
    }

    /// Project creation and dependency installation are separate steps, but the
    /// latter must still run inside the crate that `cargo new` creates.
    #[test]
    fn project_step_creates_and_enters_the_new_crate() {
        let (mut app, mut terminal) = app();

        draw(&mut app, &mut terminal);

        let rows = rendered_rows(&terminal);
        let command_rows = ["cargo new my-app", "cd my-app"].map(|command| {
            rows.iter()
                .position(|row| row.contains(command))
                .expect("rendered project command")
        });

        assert_eq!(command_rows, [command_rows[0], command_rows[0] + 1]);
        assert!(rows.iter().all(|row| !row.contains("cargo add")));
    }

    /// From a fresh app, with no focus set up first: Enter alone walks the whole
    /// wizard. Left to the default, focus would land on each step's own control
    /// and the next Enter would open a panel instead of advancing.
    #[test]
    fn enter_alone_walks_the_wizard_from_the_first_step_to_the_last() {
        let (mut app, mut terminal) = app();

        press(&mut app, &mut terminal, KeyCode::Enter);
        assert_eq!(app.state.nav.step, Step::Backend);

        press(&mut app, &mut terminal, KeyCode::Enter);
        assert_eq!(app.state.nav.step, Step::Theme);
        assert!(
            !app.state.backend.open,
            "Enter advanced rather than opening"
        );

        press(&mut app, &mut terminal, KeyCode::Enter);
        assert_eq!(app.state.nav.step, Step::Done);
    }

    /// Next is not declared on the last step, so focus has to be moved off it or
    /// it would point at a button that no longer exists.
    #[test]
    fn arriving_at_the_last_step_moves_focus_to_the_only_button_left() {
        let (mut app, mut terminal) = app();
        app.state.nav.step = Step::Theme;
        app.state.focus = FocusState::intent([nav::NEXT_ID]);

        press(&mut app, &mut terminal, KeyCode::Enter);

        assert_eq!(app.state.nav.step, Step::Done);
        assert_eq!(app.state.focus.path(), &[ChildId::Static(nav::BACK_ID)]);

        // And Back still works from there.
        press(&mut app, &mut terminal, KeyCode::Enter);
        assert_eq!(app.state.nav.step, Step::Theme);
    }

    /// Back is disabled on the first step rather than merely ignored, so it must
    /// not be reachable by keyboard at all.
    #[test]
    fn the_first_step_has_no_way_back() {
        let (mut app, mut terminal) = app();
        app.state.focus = FocusState::intent([nav::BACK_ID]);

        press(&mut app, &mut terminal, KeyCode::Enter);

        assert_eq!(app.state.nav.step, Step::Project);
    }

    /// A choice made on one step has to reach the steps that read it — that is
    /// the whole reason it lives in `Choices` and not in the step's own state.
    #[test]
    fn a_backend_chosen_on_its_step_reaches_the_summary_on_the_last_one() {
        let (mut app, mut terminal) = app();
        app.state.nav.step = Step::Backend;
        app.state.focus =
            FocusState::intent([steps::backend::SCREEN_ID, steps::backend::SELECT_ID]);

        // Open the panel, move to the second option, choose it.
        press(&mut app, &mut terminal, KeyCode::Enter);
        assert!(app.state.backend.open);
        press(&mut app, &mut terminal, KeyCode::Down);
        press(&mut app, &mut terminal, KeyCode::Enter);

        assert_eq!(app.state.choices.backend, Backend::Browser);
        assert!(!app.state.backend.open);

        app.state.nav.step = Step::Done;
        draw(&mut app, &mut terminal);
        let rendered = rendered_rows(&terminal)
            .concat()
            .chars()
            .filter(|character| character.is_ascii_graphic())
            .collect::<String>();

        for expected in [
            "Happy development!",
            "cargo new my-app",
            "cd my-app",
            "cargo add ratcn --features ratzilla",
            "cargo add ratatui --no-default-features --features layout-cache",
            "cargo add ratzilla",
            "let theme = Theme::default_dark();",
        ] {
            let expected = expected
                .chars()
                .filter(|character| character.is_ascii_graphic())
                .collect::<String>();
            assert!(
                rendered.contains(&expected),
                "missing {expected} in {rendered}"
            );
        }
    }

    /// Selecting a theme repaints the whole app, not just the step that owns the
    /// list — the shell reads the palette out of shared state every frame.
    #[test]
    fn choosing_a_theme_changes_the_palette_the_shell_draws_with() {
        let (mut app, mut terminal) = app();
        app.state.nav.step = Step::Theme;
        app.state.focus = FocusState::intent([steps::theme::SCREEN_ID, steps::theme::LIST_ID]);
        assert_eq!(app.state.choices.palette(), Theme::default_dark());

        press(&mut app, &mut terminal, KeyCode::Down);
        press(&mut app, &mut terminal, KeyCode::Enter);

        assert_ne!(app.state.choices.palette(), Theme::default_dark());
        assert_eq!(app.state.choices.palette().name, app.state.choices.theme);
    }
}
