//! Single selection: a tracked cursor and a committed choice, kept separate.
//!
//! `item_focus` is the cursor — it fires on every move and carries the
//! resulting scroll offset, so one message persists both and consecutive
//! events before a redraw see synchronized state. `selection` is the commit:
//! Enter or a click. Selecting also moves the cursor to the acted-on row, so
//! a direct click needs no preceding move.

use std::io;

#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
    widgets::Paragraph,
};
use ratcn::{
    List, ListItem, Theme,
    runtime::{self, EventResult, FocusState, HoverState, Ratcn, TabWrap},
};

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;

#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

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
    hover: HoverState,
    focused_folder: Option<&'static str>,
    selected_folder: Option<&'static str>,
    scroll: usize,
}

impl AppState {
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => self.focus = focus,
            Msg::HoverChanged(hover) => self.hover = hover,
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
    HoverChanged(HoverState),
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
                .hover(|state: &AppState| &state.hover, Msg::HoverChanged)
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

#[cfg(not(target_arch = "wasm32"))]
fn main() -> io::Result<()> {
    let mut app = App::new();
    ratatui::run(|terminal| {
        // RAII: mouse reporting is restored on every exit path (a `?` or panic).
        let _input_modes = ratcn::crossterm::InputModes::new()
            .mouse_capture()
            .enable()?;
        loop {
            terminal.draw(|frame| app.draw(frame))?;
            let event = event::read()?;
            if demo_shared::is_quit(&event) {
                break Ok(());
            }
            // Hand the backend event straight to the app — no conversion step.
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
            move |key_event| {
                app.borrow_mut().handle_event(key_event);
            }
        })
        .map_err(|e| io::Error::other(e.to_string()))?;

    terminal
        .on_mouse_event({
            let app = Rc::clone(&app);
            move |mouse_event| {
                app.borrow_mut().handle_event(mouse_event);
            }
        })
        .map_err(|e| io::Error::other(e.to_string()))?;

    terminal.draw_web(move |frame| {
        app.borrow_mut().draw(frame);
    });

    Ok(())
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
