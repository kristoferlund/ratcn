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

#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

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
        self, CellOffset, Component, DragOptions, DragPhase, Event, EventCtx, EventResult,
        PaintCtx, Ratcn, RenderCtx, clamp_offset, offset_rect,
    },
};

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;

#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

const THEME: Theme = Theme::default_dark();
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

    // Ratzilla reports browser pointer positions in terminal-cell coordinates.
    terminal
        .on_mouse_event({
            let app = Rc::clone(&app);
            move |mouse_event| {
                app.borrow_mut().handle_event(mouse_event);
            }
        })
        .map_err(|e| io::Error::other(e.to_string()))?;

    terminal.draw_web(move |frame| app.borrow_mut().draw(frame));
    Ok(())
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new(),
        }
    }

    fn handle_event(&mut self, event: impl TryInto<runtime::Event>) {
        if let EventResult::Emit(msg) = self.ratcn.handle_event(event, &self.state) {
            match msg {
                Msg::BlockMoved(offset) => self.state.block_offset = offset,
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(THEME.background));
        let draggable_block_area = offset_rect(
            area,
            area.centered(
                Constraint::Length(BLOCK_WIDTH),
                Constraint::Length(BLOCK_HEIGHT),
            ),
            self.state.block_offset,
        );
        self.ratcn.render(frame, &self.state, &THEME, |ctx| {
            ctx.render_component(
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
    fn render(&mut self, _ctx: &mut RenderCtx<'_, '_, AppState, Msg>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, AppState>) {
        let area = ctx.area();
        let theme = ctx.theme;
        let background_color = if ctx.hovered {
            darken(theme.surface, HOVER_DARKEN_PERCENT)
        } else {
            theme.surface
        };
        ctx.render_widget(
            Block::default().style(Style::default().bg(background_color)),
            area,
        );
        let text_area = area.centered(
            Constraint::Length(self.text.len() as u16),
            Constraint::Length(1),
        );
        ctx.render_widget(
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
