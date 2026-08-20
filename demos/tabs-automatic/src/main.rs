//! Tabs with automatic activation: focusing a tab selects it.
//!
//! `TabsActivation::Automatic` collapses the cursor into the selection, which
//! is why this app state has no focused-tab field and no cursor message —
//! `selection` is the only binding needed.

use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin},
    style::Style,
    widgets::{Paragraph, Wrap},
};
use ratcn::{
    Tab, Tabs, TabsActivation, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, TabWrap},
};

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
}

impl AppState {
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => self.focus = focus,
            Msg::ScreenSelected(screen) => self.selected = screen,
        }
    }
}

enum Msg {
    FocusChanged(FocusState),
    ScreenSelected(Screen),
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
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
}

impl demo_shared::Demo for App {
    fn handle_event(&mut self, event: Event) -> bool {
        match self.ratcn.handle_event(event, &self.state) {
            EventResult::Emit(msg) => {
                self.state.update(msg);
                true
            }
            EventResult::Consumed => true,
            EventResult::Ignored => false,
        }
    }

    fn draw(&mut self, frame: &mut Frame, theme: &Theme) {
        // Copied: the paint closures below outlive this borrow.
        let theme = *theme;
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(theme.background));
        let state = &self.state;
        self.ratcn.render(frame, state, &theme, |ctx| {
            let tabs = Tabs::new([
                Tab::new(Screen::Overview, "Overview"),
                Tab::new(Screen::Analytics, "Analytics"),
                Tab::new(Screen::Reports, "Reports"),
            ])
            .selection(|s: &AppState| Some(s.selected), Msg::ScreenSelected)
            .activation(TabsActivation::Automatic);

            let demo = area.centered(
                Constraint::Length(DEMO_WIDTH),
                Constraint::Length(DEMO_HEIGHT),
            );
            let [tabs_area, content_area] =
                Layout::vertical([Constraint::Length(tabs.height()), Constraint::Min(0)])
                    .areas(demo);

            ctx.component(ids::TABS, tabs, tabs_area);

            let content = state.selected.content();
            ctx.paint(move |ctx| {
                ctx.with_buffer(|buf| {
                    buf.set_style(content_area, Style::default().bg(theme.surface));
                });
                ctx.widget(
                    Paragraph::new(content).wrap(Wrap { trim: true }),
                    content_area.inner(CONTENT_PADDING),
                );
            });
        });
    }
}
