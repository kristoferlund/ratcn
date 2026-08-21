//! Ten boxes in a `ScrollArea` three of them tall.
//!
//! Click a box to focus it, or step through them with Up and Down, which reach
//! the runtime as the traversal keys it already understands. Focus landing on a
//! box the viewport is clipping scrolls that box into view.

use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::Style,
};
use ratcn::{
    Button, ButtonSize, ScrollArea, Theme,
    runtime::{Event, EventResult, FocusState, KeyCode, KeyEvent, Ratcn},
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

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

impl App {
    fn new() -> Self {
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

/// Up and Down step focus, by handing the runtime Tab and BackTab.
fn as_traversal(event: Event) -> Event {
    let code = match &event {
        Event::Key(key) if !key.modifiers.any() => match key.code {
            KeyCode::Up => Some(KeyCode::BackTab),
            KeyCode::Down => Some(KeyCode::Tab),
            _ => None,
        },
        _ => None,
    };
    code.map_or(event, |code| Event::Key(KeyEvent::new(code)))
}

impl demo_shared::Demo for App {
    fn handle_event(&mut self, event: Event) -> bool {
        match self.ratcn.handle_event(as_traversal(event), &self.state) {
            EventResult::Emit(msg) => {
                self.update(msg);
                true
            }
            EventResult::Consumed => true,
            EventResult::Ignored => false,
        }
    }

    fn draw(&mut self, frame: &mut Frame, theme: &Theme) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(theme.background));

        let state = &self.state;
        self.ratcn.render(frame, state, theme, |ctx| {
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

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
