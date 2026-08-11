//! Multi-selection with two-line custom rows.
//!
//! `render_item` returns a `Text` rather than a `Line`, and `row_height(2)`
//! tells the list that every item is two rows tall. That pairing is what keeps
//! clicking exact — without it, a click on a job title would toggle the person
//! below.

use std::collections::HashSet;
use std::io;

#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

use ratatui::{
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
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

const PEOPLE: [(&str, &str); 6] = [
    ("Jane Doe", "Product Manager"),
    ("Bill Withers", "Soul Manager"),
    ("Ada Lovelace", "Principal Engineer"),
    ("Grace Hopper", "Compiler Architect"),
    ("Alan Turing", "Research Lead"),
    ("Katherine Johnson", "Flight Analyst"),
];
/// Each item is a name line plus a title line.
const ROW_HEIGHT: u16 = 2;
const VISIBLE_PEOPLE: u16 = 4;
const LIST_WIDTH: u16 = 34;
const LIST_HEIGHT: u16 = ROW_HEIGHT * VISIBLE_PEOPLE;
const CONTENT_HEIGHT: u16 = LIST_HEIGHT + 2;
const THEME: Theme = Theme::default_dark();

mod ids {
    pub const LIST: &str = "people";
}

#[derive(Default)]
struct AppState {
    focus: FocusState,
    hover: HoverState,
    focused_person: Option<&'static str>,
    invited: HashSet<&'static str>,
    scroll: usize,
}

impl AppState {
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => self.focus = focus,
            Msg::HoverChanged(hover) => self.hover = hover,
            Msg::PersonFocusChanged(name, offset) => {
                self.focused_person = Some(name);
                self.scroll = offset;
            }
            Msg::PersonToggled(name) => {
                self.focused_person = Some(name);
                if !self.invited.remove(name) {
                    self.invited.insert(name);
                }
            }
            Msg::PersonScrollChanged(offset) => self.scroll = offset,
        }
    }

    fn title_for(&self, name: &str) -> &'static str {
        PEOPLE
            .iter()
            .find(|(person, _)| *person == name)
            .map_or("", |(_, title)| *title)
    }
}

enum Msg {
    FocusChanged(FocusState),
    HoverChanged(HoverState),
    PersonFocusChanged(&'static str, usize),
    PersonToggled(&'static str),
    PersonScrollChanged(usize),
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
                .hover(|s: &AppState| &s.hover, Msg::HoverChanged)
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
            let list = List::new(PEOPLE.map(|(name, _)| ListItem::new(name, name)))
                .item_focus(|s: &AppState| s.focused_person, Msg::PersonFocusChanged)
                .multi_selection(
                    |s: &AppState, name| s.invited.contains(name),
                    Msg::PersonToggled,
                )
                .scroll(|s: &AppState| s.scroll, Msg::PersonScrollChanged)
                // Two lines per item, so the list must be told the height.
                .row_height(ROW_HEIGHT)
                .render_item(move |state: &AppState, row| {
                    // ASCII checkbox: renders identically everywhere.
                    let marker = if row.selected { "[x]" } else { "[ ]" };
                    Text::from(vec![
                        Line::from(format!("{marker}  {}", row.label)),
                        // Row colors replace span colors, but modifiers remain.
                        Line::from(Span::styled(
                            format!("     {}", state.title_for(row.label)),
                            Style::default().add_modifier(Modifier::DIM),
                        )),
                    ])
                });

            let content_area = area.centered(
                Constraint::Length(LIST_WIDTH),
                Constraint::Length(CONTENT_HEIGHT),
            );
            let [list_area, count_area] = content_area.layout(
                &Layout::vertical([Constraint::Length(LIST_HEIGHT), Constraint::Length(1)])
                    .spacing(1),
            );

            ctx.render_component(ids::LIST, list, list_area);
            ctx.render_widget(
                Paragraph::new(format!(
                    "Invited {} of {}",
                    state.invited.len(),
                    PEOPLE.len()
                ))
                .style(Style::default().fg(muted)),
                count_area,
            );
        });
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_twice_removes_the_invitation() {
        let mut state = AppState::default();
        let (name, _) = PEOPLE[0];

        state.update(Msg::PersonToggled(name));
        assert!(state.invited.contains(name));

        state.update(Msg::PersonToggled(name));
        assert!(!state.invited.contains(name));
    }

    #[test]
    fn every_person_has_a_title_for_the_second_line() {
        let state = AppState::default();
        for (name, title) in PEOPLE {
            assert_eq!(state.title_for(name), title);
        }
    }
}
