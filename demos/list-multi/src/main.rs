//! Multi-selection: any number of rows chosen at once, checkbox style.
//!
//! The binding is a *predicate* rather than a collection — `List` asks "is this
//! one selected?" per row — so the app can store the selection however it likes.
//! Here that is a `HashSet`; a `Vec` or a flag on each record would work too.

use std::collections::HashSet;
use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::Paragraph,
};
use ratcn::{
    List, ListItem, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn, TabWrap},
};

const TOPICS: [&str; 10] = [
    "Releases",
    "Security advisories",
    "Pull requests",
    "Issues",
    "Discussions",
    "Mentions",
    "Review requests",
    "Deployments",
    "Workflow runs",
    "Sponsorships",
];
const LIST_WIDTH: u16 = 34;
const LIST_HEIGHT: u16 = 8;
const CONTENT_HEIGHT: u16 = LIST_HEIGHT + 2;
const THEME: Theme = Theme::default_dark();

mod ids {
    pub const LIST: &str = "topics";
}

#[derive(Default)]
struct AppState {
    focus: FocusState,
    focused_topic: Option<&'static str>,
    subscribed: HashSet<&'static str>,
    scroll: usize,
}

impl AppState {
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => self.focus = focus,
            Msg::TopicFocusChanged(topic, offset) => {
                self.focused_topic = Some(topic);
                self.scroll = offset;
            }
            Msg::TopicToggled(topic) => {
                // A click can select without a preceding move, so keep the
                // cursor with the row the user acted on.
                self.focused_topic = Some(topic);
                if !self.subscribed.remove(topic) {
                    self.subscribed.insert(topic);
                }
            }
            Msg::TopicScrollChanged(offset) => self.scroll = offset,
        }
    }
}

enum Msg {
    FocusChanged(FocusState),
    TopicFocusChanged(&'static str, usize),
    TopicToggled(&'static str),
    TopicScrollChanged(usize),
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
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
    fn background(&self) -> Color {
        THEME.background
    }

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

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(THEME.background));

        let state = &self.state;
        self.ratcn.render(frame, state, &THEME, |ctx| {
            let muted = ctx.theme.muted_foreground;
            let list = List::new(TOPICS.map(|label| ListItem::new(label, label)))
                .item_focus(|s: &AppState| s.focused_topic, Msg::TopicFocusChanged)
                // The predicate is asked once per row, so any container works.
                .multi_selection(
                    |s: &AppState, topic| s.subscribed.contains(topic),
                    Msg::TopicToggled,
                )
                .scroll(|s: &AppState| s.scroll, Msg::TopicScrollChanged);

            let content_area = area.centered(
                Constraint::Length(LIST_WIDTH),
                Constraint::Length(CONTENT_HEIGHT),
            );
            let [list_area, count_area] = content_area.layout(
                &Layout::vertical([Constraint::Length(LIST_HEIGHT), Constraint::Length(1)])
                    .spacing(1),
            );

            ctx.component(ids::LIST, list, list_area);

            let count = format!(
                "Subscribed to {} of {}",
                state.subscribed.len(),
                TOPICS.len()
            );
            ctx.paint_widget(
                Paragraph::new(count).style(Style::default().fg(muted)),
                count_area,
            );
        });
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_the_same_topic_twice_returns_to_unsubscribed() {
        let mut state = AppState::default();

        state.update(Msg::TopicToggled(TOPICS[1]));
        assert!(state.subscribed.contains(TOPICS[1]));

        state.update(Msg::TopicToggled(TOPICS[1]));
        assert!(!state.subscribed.contains(TOPICS[1]));
    }

    #[test]
    fn toggling_moves_the_cursor_to_the_acted_on_row() {
        let mut state = AppState::default();

        state.update(Msg::TopicToggled(TOPICS[3]));

        assert_eq!(state.focused_topic, Some(TOPICS[3]));
    }
}
