//! Draggable block demo: a single block that can be dragged anywhere within
//! the frame.
//!
//! The integration has two parts:
//!
//! - `AppState::block_offset` controls where the block is rendered;
//! - `EventCtx::drag` retains the gesture anchor by component identity and
//!   captures the pointer through release.
//!
//! Ratcn normalizes held-button pointer movement into `MouseKind::Drag` before
//! routing it to the component.

use std::io;

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Style,
    widgets::{Block, Paragraph},
};
use ratcn::{
    Theme,
    color::darken,
    runtime::{
        CellOffset, Component, DeclareCtx, DragOptions, DragPhase, Event, EventCtx, EventResult,
        PaintCtx, Ratcn, clamp_offset, offset_rect,
    },
};

const BLOCK_WIDTH: u16 = 30;
const BLOCK_HEIGHT: u16 = 7;
const HOVER_DARKEN_PERCENT: u16 = 10;

/// Child ids, named once so declarations and retained identity cannot drift.
mod ids {
    pub const BLOCK: &str = "block";
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

#[derive(Default)]
struct AppState {
    block_offset: CellOffset,
}

#[derive(Clone)]
enum Msg {
    BlockMoved(CellOffset),
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new(),
        }
    }
}

impl demo_shared::Demo for App {
    fn handle_event(&mut self, event: Event) -> bool {
        match self.ratcn.handle_event(event, &self.state) {
            EventResult::Emit(Msg::BlockMoved(offset)) => {
                self.state.block_offset = offset;
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
        let draggable_block_area = offset_rect(
            area,
            area.centered(
                Constraint::Length(BLOCK_WIDTH),
                Constraint::Length(BLOCK_HEIGHT),
            ),
            self.state.block_offset,
        );
        self.ratcn.render(frame, &self.state, theme, |ctx| {
            ctx.component(
                ids::BLOCK,
                DraggableBlock {
                    area,
                    text: "Drag me!",
                },
                draggable_block_area,
            );
        });
    }
}

/// An app-defined component that renders and handles the draggable block.
struct DraggableBlock {
    /// The full frame area that constrains the block's movement.
    area: Rect,
    text: &'static str,
}

impl Component<AppState, Msg> for DraggableBlock {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, AppState, Msg>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, AppState>) {
        let area = ctx.area();
        let theme = ctx.theme;
        let background_color = if ctx.hovered {
            darken(theme.surface, HOVER_DARKEN_PERCENT)
        } else {
            theme.surface
        };
        ctx.widget(
            Block::default().style(Style::default().bg(background_color)),
            area,
        );
        let text_area = area.centered(
            Constraint::Length(self.text.len() as u16),
            Constraint::Length(1),
        );
        ctx.widget(
            Paragraph::new(self.text)
                .style(Style::default().fg(theme.foreground).bg(background_color)),
            text_area,
        );
    }

    fn handle_event(
        &mut self,
        event: &Event,
        state: &AppState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<Msg> {
        let Event::Mouse(mouse_event) = event else {
            return EventResult::Ignored;
        };
        match ctx.drag(mouse_event, DragOptions::new(state.block_offset)) {
            DragPhase::Down | DragPhase::Ended { .. } => EventResult::Consumed,
            DragPhase::Moved {
                offset: next_offset,
                ..
            } => {
                let clamped_offset = clamp_offset(
                    self.area,
                    self.area.centered(
                        Constraint::Length(BLOCK_WIDTH),
                        Constraint::Length(BLOCK_HEIGHT),
                    ),
                    next_offset,
                );
                EventResult::Emit(Msg::BlockMoved(clamped_offset))
            }
            DragPhase::Ignored => EventResult::Ignored,
        }
    }
}
