//! Tabs with manual activation: browsing and committing are separate.
//!
//! Arrow keys move the tab cursor without changing the page; Enter, Space, or
//! a click commits the focused tab. Selecting also moves the cursor to the
//! acted-on tab, so a direct click needs no preceding move.

use std::io;

#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

use ratatui::{
    layout::{Constraint, Layout, Margin},
    style::Style,
    widgets::{Paragraph, Wrap},
};
use ratcn::{
    Tab, Tabs, Theme,
    runtime::{self, EventResult, FocusState, Ratcn, TabWrap},
};

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;

#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

const THEME: Theme = Theme::default_dark();
const DEMO_WIDTH: u16 = 54;
const DEMO_HEIGHT: u16 = 13;
const CONTENT_PADDING: Margin = Margin::new(4, 2);

mod ids {
    pub const TABS: &str = "tabs";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Screen {
    #[default]
    Overview,
    Analytics,
    Reports,
}

impl Screen {
    const fn content(self) -> &'static str {
        match self {
            Self::Overview => {
                "Overview\n\nTrack key metrics and recent project activity.\n\n12 active projects, 3 pending tasks."
            }
            Self::Analytics => {
                "Analytics\n\nMonitor performance and user engagement.\n\nPage views are up 25% this month."
            }
            Self::Reports => {
                "Reports\n\nGenerate and export detailed reports.\n\n5 reports are ready to export."
            }
        }
    }
}

#[derive(Default)]
struct AppState {
    focus: FocusState,
    selected: Screen,
    focused: Screen,
}

impl AppState {
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => self.focus = focus,
            Msg::ScreenFocusChanged(screen) => self.focused = screen,
            Msg::ScreenSelected(screen) => {
                // A click can select without a preceding move, so keep the
                // cursor with the tab the user acted on.
                self.focused = screen;
                self.selected = screen;
            }
        }
    }
}

enum Msg {
    FocusChanged(FocusState),
    ScreenFocusChanged(Screen),
    ScreenSelected(Screen),
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> io::Result<()> {
    let mut app = App::new();
    ratatui::run(|terminal| {
        let _input_modes = ratcn::crossterm::InputModes::new()
            .mouse_capture()
            .enable()?;
        loop {
            terminal.draw(|frame| app.draw(frame))?;
            let event = event::read()?;
            if demo_shared::is_quit(&event) {
                break Ok(());
            }
            app.handle_event(event);
        }
    })
}

#[cfg(target_arch = "wasm32")]
fn main() -> io::Result<()> {
    let backend = demo_shared::web_backend(THEME.background)?;
    let mut terminal = ratatui::Terminal::new(backend)?;
    let app = Rc::new(RefCell::new(App::new()));

    terminal
        .on_key_event({
            let app = Rc::clone(&app);
            move |key_event| app.borrow_mut().handle_event(key_event)
        })
        .map_err(|e| io::Error::other(e.to_string()))?;
    terminal
        .on_mouse_event({
            let app = Rc::clone(&app);
            move |mouse_event| app.borrow_mut().handle_event(mouse_event)
        })
        .map_err(|e| io::Error::other(e.to_string()))?;

    terminal.draw_web(move |frame| app.borrow_mut().draw(frame));
    Ok(())
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new()
                .focus(|s: &AppState| &s.focus, Msg::FocusChanged)
                .tab_wrap(TabWrap::Wrap),
        }
    }

    fn handle_event(&mut self, event: impl TryInto<runtime::Event>) {
        if let EventResult::Emit(msg) = self.ratcn.handle_event(event, &self.state) {
            self.state.update(msg);
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(THEME.background));
        let state = &self.state;
        self.ratcn.render(frame, state, &THEME, |ctx| {
            let tabs = Tabs::new([
                Tab::new(Screen::Overview, "Overview"),
                Tab::new(Screen::Analytics, "Analytics"),
                Tab::new(Screen::Reports, "Reports"),
            ])
            .tab_focus(|s: &AppState| Some(s.focused), Msg::ScreenFocusChanged)
            .selection(|s: &AppState| Some(s.selected), Msg::ScreenSelected);

            let demo = area.centered(
                Constraint::Length(DEMO_WIDTH),
                Constraint::Length(DEMO_HEIGHT),
            );
            let [tabs_area, content_area] =
                Layout::vertical([Constraint::Length(tabs.height()), Constraint::Min(0)])
                    .areas(demo);

            ctx.render_component(ids::TABS, tabs, tabs_area);

            let content = state.selected.content();
            ctx.paint(move |ctx| {
                ctx.with_buffer(|buf| {
                    buf.set_style(content_area, Style::default().bg(THEME.surface));
                });
                ctx.render_widget(
                    Paragraph::new(content).wrap(Wrap { trim: true }),
                    content_area.inner(CONTENT_PADDING),
                );
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_also_moves_the_cursor_to_the_selected_tab() {
        let mut state = AppState::default();

        state.update(Msg::ScreenSelected(Screen::Reports));

        assert_eq!(state.selected, Screen::Reports);
        assert_eq!(state.focused, Screen::Reports);
    }
}
