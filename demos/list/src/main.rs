//! Single selection: a tracked cursor and a committed choice, kept separate.
//!
//! `item_focus` is the cursor — it fires on every move and carries the
//! resulting scroll offset, so one message persists both and consecutive
//! events before a redraw see synchronized state. `selection` is the commit:
//! Enter or a click. Selecting also moves the cursor to the acted-on row, so
//! a direct click needs no preceding move.

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

const FOLDERS: [&str; 16] = [
    "Inbox",
    "Starred",
    "Drafts",
    "Sent",
    "Archive",
    "Projects",
    "Settings",
    "Trash",
    "Spam",
    "Snoozed",
    "Important",
    "Scheduled",
    "All Mail",
    "Chats",
    "Labels",
    "Templates",
];
const LIST_WIDTH: u16 = 30;
const LIST_HEIGHT: u16 = 8;
const CONTENT_HEIGHT: u16 = LIST_HEIGHT + 3;
const THEME: Theme = Theme::default_dark();

/// Child ids, named once so declarations and retained identity can't drift.
mod ids {
    pub const LIST: &str = "folders";
}

#[derive(Default)]
struct AppState {
    focus: FocusState,
    focused_folder: Option<&'static str>,
    selected_folder: Option<&'static str>,
    scroll: usize,
}

impl AppState {
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => self.focus = focus,
            Msg::FolderFocusChanged(folder, offset) => {
                self.focused_folder = Some(folder);
                self.scroll = offset;
            }
            Msg::FolderSelected(folder) => {
                // A click can select without a preceding move, so keep the
                // cursor with the row the user acted on.
                self.focused_folder = Some(folder);
                self.selected_folder = Some(folder);
            }
            Msg::FolderScrollChanged(offset) => self.scroll = offset,
        }
    }
}

enum Msg {
    FocusChanged(FocusState),
    FolderFocusChanged(&'static str, usize),
    FolderSelected(&'static str),
    FolderScrollChanged(usize),
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
                .focus(|state: &AppState| &state.focus, Msg::FocusChanged)
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
            let list = List::new(FOLDERS.map(|label| ListItem::new(label, label)))
                .item_focus(
                    |state: &AppState| state.focused_folder,
                    Msg::FolderFocusChanged,
                )
                .selection(
                    |state: &AppState| state.selected_folder,
                    Msg::FolderSelected,
                )
                .scroll(|state: &AppState| state.scroll, Msg::FolderScrollChanged);
            let content_area = area.centered(
                Constraint::Length(LIST_WIDTH),
                Constraint::Length(CONTENT_HEIGHT),
            );
            let [list_area, focused_area, selected_area] = content_area.layout(
                &Layout::vertical([
                    Constraint::Length(LIST_HEIGHT),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .spacing(1),
            );

            ctx.render_component(ids::LIST, list, list_area);
            let focused = format!("Focus: {}", state.focused_folder.unwrap_or("None"));
            let selected = format!("Selected: {}", state.selected_folder.unwrap_or("None"));
            ctx.paint(move |ctx| {
                ctx.render_widget(
                    Paragraph::new(focused).style(Style::default().fg(muted)),
                    focused_area,
                );
                ctx.render_widget(
                    Paragraph::new(selected).style(Style::default().fg(muted)),
                    selected_area,
                );
            });
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
    fn selecting_also_moves_the_cursor_to_the_selected_row() {
        let mut state = AppState::default();

        state.update(Msg::FolderSelected(FOLDERS[2]));

        assert_eq!(state.selected_folder, Some(FOLDERS[2]));
        assert_eq!(state.focused_folder, Some(FOLDERS[2]));
    }

    #[test]
    fn a_focus_change_persists_cursor_and_scroll_in_one_message() {
        let mut state = AppState::default();

        state.update(Msg::FolderFocusChanged(FOLDERS[9], 4));

        assert_eq!(state.focused_folder, Some(FOLDERS[9]));
        assert_eq!(state.scroll, 4);
    }
}
