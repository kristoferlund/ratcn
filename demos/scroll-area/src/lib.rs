//! Ten boxes in a `ScrollArea` three of them tall.
//!
//! Click a box to focus it, or step through them with Tab and Shift+Tab. Focus
//! landing on a box the viewport is clipping scrolls that box into view. Page
//! Up and Page Down scroll the viewport without moving focus.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    style::Style,
};
use ratcn::{
    Button, ButtonSize, ScrollArea, Theme,
    runtime::{Event, EventResult, FocusState, Ratcn},
};

/// One entry per box: its child id, which is also its label.
const BOXES: [&str; 10] = [
    "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten",
];

/// Blank cells between the boxes and the edges of the content.
const PAD: u16 = 1;
/// A box and the blank row under it.
const STEP: u16 = ButtonSize::Large.height() + 1;
/// Every box, padded above the first and below the last.
const CONTENT_HEIGHT: u16 = PAD + BOXES.len() as u16 * STEP - 1 + PAD;
/// The viewport shows three boxes and their padding.
const VIEWPORT_HEIGHT: u16 = PAD + 3 * STEP - 1 + PAD;
/// Box width, its padding on both sides, and the scrollbar gutter column.
const VIEWPORT_WIDTH: u16 = 24 + 2 * PAD + 1;

#[derive(Default)]
struct AppState {
    focus: FocusState,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Pressed,
}

pub struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new().focus(|s: &AppState| &s.focus, Msg::Focus),
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(focus) => self.state.focus = focus,
            Msg::Pressed => {}
        }
    }
}

impl demo_shared::Demo for App {
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
            let [column] = Layout::horizontal([Constraint::Length(VIEWPORT_WIDTH)])
                .flex(Flex::Center)
                .areas(area);
            let [viewport] = Layout::vertical([Constraint::Length(VIEWPORT_HEIGHT)])
                .flex(Flex::Center)
                .areas(column);

            let scroll = ScrollArea::new(CONTENT_HEIGHT).content(|ctx| {
                let content = ctx.area();
                for (index, label) in BOXES.into_iter().enumerate() {
                    let top = content.y + PAD + index as u16 * STEP;
                    ctx.component(
                        label,
                        Button::new(label)
                            .size(ButtonSize::Large)
                            .secondary()
                            .on_press(|| Msg::Pressed),
                        Rect::new(
                            content.x + PAD,
                            top,
                            content.width - 2 * PAD,
                            ButtonSize::Large.height(),
                        ),
                    );
                }
            });
            ctx.component("boxes", scroll, viewport);
        });
    }
}

#[cfg(test)]
mod tests {
    use demo_shared::Demo as _;
    use ratatui::{Terminal, backend::TestBackend};
    use ratcn::runtime::{KeyCode, KeyEvent};

    use super::*;

    fn app() -> (App, Terminal<TestBackend>) {
        (
            App::new(),
            Terminal::new(TestBackend::new(VIEWPORT_WIDTH, VIEWPORT_HEIGHT)).expect("terminal"),
        )
    }

    fn draw(app: &mut App, terminal: &mut Terminal<TestBackend>) {
        terminal
            .draw(|frame| {
                let area = frame.area();
                app.draw(frame.buffer_mut(), area, &Theme::default_dark());
            })
            .expect("draw");
    }

    fn press(app: &mut App, code: KeyCode) -> bool {
        app.handle_event(Event::Key(KeyEvent::new(code)))
    }

    fn rendered(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn arrows_and_vi_keys_do_not_traverse_independent_buttons() {
        let (mut app, mut terminal) = app();
        let focused = FocusState::intent(["boxes", "One"]);
        app.state.focus = focused.clone();
        draw(&mut app, &mut terminal);

        for code in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Char('h'),
            KeyCode::Char('j'),
            KeyCode::Char('k'),
            KeyCode::Char('l'),
        ] {
            assert!(
                !press(&mut app, code),
                "{code:?} must remain available to the host"
            );
            assert_eq!(app.state.focus, focused, "{code:?} moved button focus");
        }
    }

    #[test]
    fn tab_traverses_buttons_and_reveals_a_clipped_destination() {
        let (mut app, mut terminal) = app();
        app.state.focus = FocusState::intent(["boxes", "One"]);
        draw(&mut app, &mut terminal);

        for label in &BOXES[1..] {
            assert!(press(&mut app, KeyCode::Tab));
            assert_eq!(app.state.focus, FocusState::intent(["boxes", *label]));
            draw(&mut app, &mut terminal);
        }

        let screen = rendered(&terminal);
        assert!(
            screen.contains("Ten"),
            "focused last button was not revealed"
        );
        assert!(
            !screen.contains("One"),
            "viewport did not scroll to the last button"
        );
        assert!(press(&mut app, KeyCode::BackTab));
        assert_eq!(app.state.focus, FocusState::intent(["boxes", "Nine"]));
    }

    #[test]
    fn page_keys_scroll_the_component_without_traversing_focus() {
        let (mut app, mut terminal) = app();
        let focused = FocusState::intent(["boxes", "One"]);
        app.state.focus = focused.clone();
        draw(&mut app, &mut terminal);

        assert!(press(&mut app, KeyCode::PageDown));
        assert_eq!(app.state.focus, focused);
        draw(&mut app, &mut terminal);

        let screen = rendered(&terminal);
        assert!(
            screen.contains("Four"),
            "PageDown did not scroll the viewport"
        );
        assert!(
            !screen.contains("One"),
            "PageDown left the first page visible"
        );

        assert!(press(&mut app, KeyCode::Tab));
        assert_eq!(app.state.focus, FocusState::intent(["boxes", "Two"]));
        draw(&mut app, &mut terminal);
        assert!(
            rendered(&terminal).contains("Two"),
            "Tab did not reveal its target"
        );
    }
}
