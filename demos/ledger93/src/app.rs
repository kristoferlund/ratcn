//! The app shell owns orchestration and routes messages to the state owner.

use ratatui::{
    layout::{Constraint, Layout, Margin},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use ratcn::{
    TabsSize, Theme,
    runtime::{EventResult, FocusState, Ratcn, ScopeOptions, TabWrap},
};

use crate::nav::{self, Nav, NavMsg, Screen};
use crate::screens;
use crate::shared::{PrefsMsg, Shared};

/// A warm amber palette that suits a terminal from 1993.
pub const THEME: Theme = Theme::gruvbox();
const DEMO_WIDTH: u16 = 60;
const DEMO_HEIGHT: u16 = 30;
const PADDING_X: u16 = 2;
const PADDING_Y: u16 = 2;

mod ids {
    pub const TABS: &str = "tabs";
}

#[derive(Default)]
pub struct AppState {
    pub focus: FocusState,
    pub nav: Nav,
    pub shared: Shared,
    pub ledger: screens::ledger::State,
    pub report: screens::report::State,
    pub settings: screens::settings::State,
}

#[derive(Clone)]
pub enum Msg {
    Focus(FocusState),
    Nav(NavMsg),
    Ledger(screens::ledger::Msg),
    Report(screens::report::Msg),
    Settings(screens::settings::Msg),
    Prefs(PrefsMsg),
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

    pub fn handle_event(&mut self, event: impl TryInto<ratcn::runtime::Event>) {
        if let EventResult::Emit(msg) = self.ratcn.handle_event(event, &self.state) {
            match msg {
                Msg::Focus(focus) => self.state.focus = focus,
                Msg::Nav(NavMsg::Focused(screen)) => {
                    self.state.nav.update(NavMsg::Focused(screen));
                }
                Msg::Nav(NavMsg::Selected(screen)) => {
                    self.state.nav.update(NavMsg::Selected(screen));
                    self.state.focus = FocusState::intent([screen_id(screen)]);
                }
                Msg::Ledger(msg) => self.state.ledger.update(msg),
                Msg::Report(msg) => self.state.report.update(msg),
                Msg::Settings(msg) => self.state.settings.update(msg),
                Msg::Prefs(PrefsMsg::SetCurrency(currency)) => {
                    self.state
                        .settings
                        .update(screens::settings::Msg::CurrencyFocused(
                            currency,
                            self.state.settings.list_scroll,
                        ));
                    self.state
                        .shared
                        .prefs
                        .update(PrefsMsg::SetCurrency(currency));
                }
            }
        }
    }

    pub fn draw(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(THEME.background));
        let area = area
            .centered(
                Constraint::Length(DEMO_WIDTH),
                Constraint::Length(DEMO_HEIGHT),
            )
            .inner(Margin::new(PADDING_X, PADDING_Y));
        let state = &self.state;
        self.ratcn.render(frame, state, &THEME, |ctx| {
            let [title, _gap, tabs, content, status] = area.layout(&shell_layout());

            ctx.paint(move |ctx| {
                ctx.render_widget(
                    Line::from(Span::styled(
                        "LEDGER 1993 · Modern Double-Entry Bookkeeping",
                        Style::default()
                            .fg(THEME.accent)
                            .add_modifier(Modifier::BOLD),
                    )),
                    title,
                );
            });

            ctx.render_component(ids::TABS, nav::tabs(), tabs);

            ctx.scope(
                screen_id(state.nav.selected),
                content,
                ScopeOptions::default(),
                |ctx| match state.nav.selected {
                    Screen::Ledger => screens::ledger::render(ctx),
                    Screen::Report => screens::report::render(ctx),
                    Screen::Settings => screens::settings::render(ctx),
                },
            );

            let status_line = format!(
                " {} · Tab/←→ to navigate ",
                state.shared.prefs.currency.code()
            );
            ctx.paint(move |ctx| {
                ctx.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        status_line,
                        Style::default().fg(THEME.muted_foreground),
                    )))
                    .centered(),
                    status,
                );
            });
        });
    }
}

const fn screen_id(screen: Screen) -> &'static str {
    match screen {
        Screen::Ledger => screens::ledger::SCREEN_ID,
        Screen::Report => screens::report::SCREEN_ID,
        Screen::Settings => screens::settings::SCREEN_ID,
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

fn shell_layout() -> Layout {
    Layout::vertical([
        Constraint::Length(1),                        // title bar
        Constraint::Length(1),                        // gap
        Constraint::Length(TabsSize::Small.height()), // tab row
        Constraint::Min(0),                           // active view
        Constraint::Length(1),                        // status footer
    ])
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};
    use ratcn::runtime::{ChildId, Event, KeyCode, KeyEvent};

    use super::*;
    use crate::{
        screens::{ledger, report, settings},
        shared,
    };

    fn draw(app: &mut App, terminal: &mut Terminal<TestBackend>) {
        terminal.draw(|frame| app.draw(frame)).expect("draw");
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code))
    }

    fn select(app: &mut App, screen: Screen) {
        app.state.nav.update(NavMsg::Selected(screen));
        app.state.focus = FocusState::intent([screen_id(screen)]);
    }

    #[test]
    fn selected_scope_intent_descends_to_each_panes_first_child() {
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).expect("terminal");

        let mut ledger_app = App::new();
        select(&mut ledger_app, Screen::Ledger);
        assert_eq!(
            ledger_app.state.focus.path(),
            &[ChildId::Static(ledger::SCREEN_ID)]
        );
        draw(&mut ledger_app, &mut terminal);
        ledger_app.handle_event(key(KeyCode::Down));
        assert_eq!(ledger_app.state.ledger.row, Some(shared::SEED[0].label));

        let mut report_app = App::new();
        select(&mut report_app, Screen::Report);
        assert_eq!(
            report_app.state.focus.path(),
            &[ChildId::Static(report::SCREEN_ID)]
        );
        draw(&mut report_app, &mut terminal);
        report_app.handle_event(key(KeyCode::Enter));
        assert_eq!(report_app.state.report.sort, report::Sort::Name);

        let mut settings_app = App::new();
        select(&mut settings_app, Screen::Settings);
        assert_eq!(
            settings_app.state.focus.path(),
            &[ChildId::Static(settings::SCREEN_ID)]
        );
        draw(&mut settings_app, &mut terminal);
        settings_app.handle_event(key(KeyCode::Down));
        assert_eq!(
            settings_app.state.settings.currency_cursor,
            Some(shared::Currency::Usd)
        );
    }

    #[test]
    fn only_the_selected_panes_child_routes_events() {
        let mut terminal = Terminal::new(TestBackend::new(60, 30)).expect("terminal");

        for selected in [Screen::Ledger, Screen::Report, Screen::Settings] {
            let mut app = App::new();
            select(&mut app, selected);
            draw(&mut app, &mut terminal);

            app.state.focus = FocusState::intent([ledger::SCREEN_ID, ledger::LIST_ID]);
            app.handle_event(key(KeyCode::Down));

            app.state.focus = FocusState::intent([report::SCREEN_ID, report::SORT_ID]);
            app.handle_event(key(KeyCode::Enter));

            app.state.focus = FocusState::intent([settings::SCREEN_ID, settings::CURRENCY_ID]);
            app.handle_event(key(KeyCode::Down));

            assert_eq!(
                app.state.ledger.row,
                (selected == Screen::Ledger).then_some(shared::SEED[0].label)
            );
            assert_eq!(
                app.state.report.sort,
                if selected == Screen::Report {
                    report::Sort::Name
                } else {
                    report::Sort::Amount
                }
            );
            assert_eq!(
                app.state.settings.currency_cursor,
                (selected == Screen::Settings).then_some(shared::Currency::Usd)
            );
        }
    }
}
