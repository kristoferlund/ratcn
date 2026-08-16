//! Kanban demo: cards retain their identity while moving between columns.
//!
//! This extends the draggable block pattern with a drop target:
//!
//! - pressing a card begins pointer capture, while the first movement starts
//!   the semantic drag;
//! - stable card identities preserve the gesture across declaration rebuilds
//!   and pointer movement outside the source card;
//! - while dragging, an empty bordered placeholder holds the card's slot, and
//!   the dragged card is painted above the board;
//! - releasing over a column moves the card there.
//!
//! The app owns card placement and the active semantic drag. Ratcn retains only
//! the transient gesture anchor and pointer capture.

use std::io;

#[cfg(target_arch = "wasm32")]
use std::{cell::RefCell, rc::Rc};

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Layout, Margin, Rect},
    style::Style,
    symbols::border,
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};
use ratcn::{
    Theme,
    runtime::{
        self, CellOffset, ChildId, Component, DragOptions, DragPhase, Event, EventCtx, EventResult,
        PaintCtx, Ratcn, RenderCtx, offset_rect,
    },
};

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;

#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

const THEME: Theme = Theme::default_dark();
const CARD_WIDTH: u16 = 13;
const CARD_HEIGHT: u16 = 3;
const CARD_VERTICAL_SPACING: u16 = 1;
const COLUMN_COUNT: usize = 3;
const BOARD_HORIZONTAL_PADDING: u16 = 4;
const BOARD_VERTICAL_PADDING: u16 = 2;
const CARD_COUNT: usize = 5;
const COLUMN_TITLES: [&str; COLUMN_COUNT] = ["Todo", "Doing", "Done"];

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

struct AppState {
    cards_by_column: [Vec<ChildId>; COLUMN_COUNT],
    active_drag: Option<ActiveDrag>,
}

struct ActiveDrag {
    card_id: ChildId,
    offset: CellOffset,
}

impl AppState {
    fn new() -> Self {
        let mut cards_by_column: [Vec<ChildId>; COLUMN_COUNT] = std::array::from_fn(|_| Vec::new());
        for number in 1..=CARD_COUNT {
            cards_by_column[(number - 1) % COLUMN_COUNT].push(number.to_string().into());
        }
        Self {
            cards_by_column,
            active_drag: None,
        }
    }
}

enum Msg {
    DragStarted(ActiveDrag),
    DragMoved(CellOffset),
    CardDropped { target_column_index: usize },
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
            state: AppState::new(),
            ratcn: Ratcn::new(),
        }
    }

    fn handle_event(&mut self, event: impl TryInto<runtime::Event>) {
        if let EventResult::Emit(msg) = self.ratcn.handle_event(event, &self.state) {
            update(&mut self.state, msg);
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(THEME.background));
        let board_layout = BoardLayout {
            area: area.inner(Margin::new(
                BOARD_HORIZONTAL_PADDING,
                BOARD_VERTICAL_PADDING,
            )),
        };
        let state = &self.state;
        self.ratcn.render(frame, state, &THEME, |ctx| {
            let column_areas = board_layout.column_areas();
            ctx.paint(move |ctx| {
                for (column_index, column_area) in column_areas.iter().enumerate() {
                    if column_index > 0 {
                        ctx.render_widget(
                            Block::new()
                                .borders(Borders::LEFT)
                                .border_set(border::ROUNDED)
                                .border_style(Style::default().fg(ctx.theme.border)),
                            *column_area,
                        );
                    }
                    ctx.render_widget(
                        Paragraph::new(COLUMN_TITLES[column_index])
                            .alignment(Alignment::Center)
                            .style(Style::default().fg(ctx.theme.muted_foreground)),
                        Rect {
                            height: 1,
                            ..*column_area
                        },
                    );
                }
            });

            for (column_index, cards_in_column) in state.cards_by_column.iter().enumerate() {
                for (card_index, card_id) in cards_in_column.iter().enumerate() {
                    ctx.render_component(
                        card_id,
                        KanbanCard {
                            card_id: card_id.clone(),
                            board_layout,
                        },
                        board_layout.card_area(column_index, card_index),
                    );
                }
            }
        });
    }
}

/// The only place app state changes; every emitted `Msg` lands here.
fn update(state: &mut AppState, msg: Msg) {
    match msg {
        Msg::DragStarted(active_drag) => state.active_drag = Some(active_drag),
        Msg::DragMoved(offset) => {
            if let Some(active_drag) = &mut state.active_drag {
                active_drag.offset = offset;
            }
        }
        // The release commits: the card leaves its old column and joins the
        // bottom of the one it landed on. Landing back on its own column is a
        // cancelled drag — the card keeps its position rather than moving to
        // the bottom.
        Msg::CardDropped {
            target_column_index,
        } => {
            if let Some(active_drag) = state.active_drag.take()
                && let Some(source_column_index) = state
                    .cards_by_column
                    .iter()
                    .position(|column| column.contains(&active_drag.card_id))
                && source_column_index != target_column_index
            {
                state.cards_by_column[source_column_index]
                    .retain(|card_id| card_id != &active_drag.card_id);
                state.cards_by_column[target_column_index].push(active_drag.card_id);
            }
        }
    }
}

/// Shared geometry for card placement, drag bounds, and drop hit-testing.
#[derive(Clone, Copy)]
struct BoardLayout {
    area: Rect,
}

impl BoardLayout {
    fn column_areas(&self) -> [Rect; COLUMN_COUNT] {
        Layout::horizontal([Constraint::Ratio(1, 3); COLUMN_COUNT]).areas(self.area)
    }

    /// The board column under a pointer position when the card is released.
    fn column_index_at(&self, pointer_column: u16) -> usize {
        self.column_areas()
            .iter()
            .position(|column_area| pointer_column < column_area.right())
            .unwrap_or(COLUMN_COUNT - 1)
    }

    /// The area a card occupies in its column's ordered list. A dragged card
    /// keeps its slot, so the remaining cards do not reflow mid-drag.
    fn card_area(&self, column_index: usize, card_index: usize) -> Rect {
        let column_area = self.column_areas()[column_index];
        let card_index = card_index as u16;
        let card_width = CARD_WIDTH.min(column_area.width.saturating_sub(4));
        Rect {
            x: column_area.x + column_area.width.saturating_sub(card_width) / 2,
            y: column_area
                .y
                .saturating_add(2)
                .saturating_add(card_index.saturating_mul(CARD_HEIGHT + CARD_VERTICAL_SPACING)),
            width: card_width,
            height: CARD_HEIGHT,
        }
    }
}

/// An app-defined draggable card with board-aware drop handling.
struct KanbanCard {
    card_id: ChildId,
    board_layout: BoardLayout,
}

impl Component<AppState, Msg> for KanbanCard {
    fn render(&mut self, ctx: &mut RenderCtx<'_, '_, AppState, Msg>) {
        // The ghost follows the pointer over every card declared after this
        // one, so it is deferred to the top of the frame rather than painted
        // in place.
        let Some(active_drag) = ctx
            .state()
            .active_drag
            .as_ref()
            .filter(|active_drag| active_drag.card_id == self.card_id)
        else {
            return;
        };
        let dragged_card_area = offset_rect(self.board_layout.area, ctx.area(), active_drag.offset);
        let dragged_card_id = self.card_id.clone();
        ctx.defer_paint(move |painter, _state| {
            let theme = painter.theme;
            painter.with_buffer(|buf| {
                paint_card(buf, dragged_card_area, &dragged_card_id, theme);
            });
        });
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, '_, AppState>) {
        let area = ctx.area();
        let theme = ctx.theme;
        let dragging = ctx
            .state()
            .active_drag
            .as_ref()
            .is_some_and(|active_drag| active_drag.card_id == self.card_id);
        if dragging {
            // The card left an empty slot behind: only its outline stays.
            ctx.render_widget(
                Block::bordered()
                    .border_set(border::ROUNDED)
                    .border_style(Style::default().fg(theme.border)),
                area,
            );
        } else {
            ctx.with_buffer(|buf| paint_card(buf, area, &self.card_id, theme));
        }
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
        match ctx.drag(
            mouse_event,
            DragOptions::default().start_if(state.active_drag.is_none()),
        ) {
            DragPhase::Down => EventResult::Consumed,
            DragPhase::Moved { offset, .. } => match &state.active_drag {
                Some(active_drag) if active_drag.card_id == self.card_id => {
                    EventResult::Emit(Msg::DragMoved(offset))
                }
                None => EventResult::Emit(Msg::DragStarted(ActiveDrag {
                    card_id: self.card_id.clone(),
                    offset,
                })),
                Some(_) => EventResult::Consumed,
            },
            DragPhase::Ended { position, moved } => {
                if moved
                    && state
                        .active_drag
                        .as_ref()
                        .is_some_and(|active_drag| active_drag.card_id == self.card_id)
                {
                    EventResult::Emit(Msg::CardDropped {
                        target_column_index: self.board_layout.column_index_at(position.x),
                    })
                } else {
                    EventResult::Consumed
                }
            }
            DragPhase::Ignored => EventResult::Ignored,
        }
    }
}

fn paint_card(buf: &mut Buffer, area: Rect, card_id: &ChildId, theme: &Theme) {
    // Cards are opaque; styling alone would leave underlying border glyphs visible.
    Clear.render(area, buf);
    buf.set_style(area, Style::default().bg(theme.accent));
    Paragraph::new(card_id.as_str())
        .style(Style::default().fg(theme.background).bg(theme.accent))
        .alignment(Alignment::Center)
        .render(
            area.centered(Constraint::Fill(1), Constraint::Length(1)),
            buf,
        );
}

// The payoff of a pure `update`: the drop commit is testable without a
// terminal or a pointer.
#[cfg(test)]
mod tests {
    use super::*;

    fn dragging(card_id: &ChildId) -> Msg {
        Msg::DragStarted(ActiveDrag {
            card_id: card_id.clone(),
            offset: CellOffset::default(),
        })
    }

    #[test]
    fn drop_moves_the_card_to_the_bottom_of_the_target_column() {
        let mut state = AppState::new();
        let moved = state.cards_by_column[0][0].clone();

        update(&mut state, dragging(&moved));
        update(
            &mut state,
            Msg::CardDropped {
                target_column_index: 2,
            },
        );

        assert!(!state.cards_by_column[0].contains(&moved));
        assert_eq!(state.cards_by_column[2].last(), Some(&moved));
        assert!(state.active_drag.is_none());
    }

    #[test]
    fn drop_on_the_source_column_keeps_the_card_position() {
        let mut state = AppState::new();
        let dragged = state.cards_by_column[0][0].clone();
        assert!(
            state.cards_by_column[0].len() > 1,
            "the cancelled drag must have a position to lose"
        );

        update(&mut state, dragging(&dragged));
        update(
            &mut state,
            Msg::CardDropped {
                target_column_index: 0,
            },
        );

        assert_eq!(state.cards_by_column[0].first(), Some(&dragged));
        assert!(state.active_drag.is_none());
    }
}
