//! The film's transport: the bottom row of the terminal, where the film is
//! paused, scrubbed and replayed.
//!
//! Two custom components and some paint. Like every other ratcn component they
//! change nothing themselves: a click on the play button or a drag along the
//! scrubber is a message, and `update` is what moves the film.

use ratatui::layout::Rect;
use ratcn::runtime::{
    Component, DeclareCtx, Event, EventCtx, EventResult, FocusState, KeyCode, MouseButton,
    MouseKind, PaintCtx, PointerShape, ScopeOptions,
};

use crate::{
    Msg,
    hud::{DIM, GREY, WHITE, fg},
    scenes, score,
};

/// How far an arrow key moves the film.
const SKIP: f64 = 5.0;

mod ids {
    pub const PLAY: &str = "play";
    pub const SCRUBBER: &str = "scrubber";
}

/// What the transport is drawn from.
pub struct Transport {
    /// Where the film is, in seconds.
    pub position: f64,
    pub running: bool,
    pub focus: FocusState,
    /// The soundtrack is audible, for the key hints.
    pub sound: bool,
}

impl Transport {
    pub fn new() -> Self {
        Self {
            position: 0.0,
            running: true,
            // The scrubber holds the keyboard: Space and the arrows are its keys.
            focus: FocusState::intent([ids::SCRUBBER]),
            sound: false,
        }
    }
}

/// Declare the transport along `row`, which is one cell tall.
pub fn declare(ctx: &mut DeclareCtx<'_, Transport, Msg>, row: Rect) {
    let state = ctx.state();
    let shown = state.position.min(score::END);
    let times = format!(
        " {:02}:{:02} / {:02}:{:02} ",
        shown as u32 / 60,
        shown as u32 % 60,
        score::END as u32 / 60,
        score::END as u32 % 60
    );
    let keys = format!(
        " SPACE pause  ←→ scrub  R replay  M sound {}  ^C quit ",
        if state.sound { "on" } else { "off" }
    );
    // The keys are shown while they are news, and whenever the film is not
    // running; the rest of the time the scrubber has the room.
    let idle = !state.running || !(8.0..score::END).contains(&state.position);
    let keys_width = if idle && row.width >= 110 {
        keys.chars().count() as u16
    } else {
        0
    };
    let chapter = scenes::CHAPTERS
        .iter()
        .rev()
        .find(|(start, _)| state.position >= *start)
        .map_or("", |(_, name)| name);
    let chapter_width = 24.min(row.width / 5);
    let play = Rect::new(row.x, row.y, 2, 1);
    let first = play.right() + times.chars().count() as u16;
    let width = row
        .width
        .saturating_sub(first - row.x + keys_width + chapter_width + 2);
    let bar = Rect::new(first, row.y, width, 1);

    ctx.paint(move |ctx| {
        ctx.with_buffer(row, |row, buffer| {
            buffer.set_string(play.right(), row.y, &times, fg(WHITE));
            if keys_width > 0 {
                buffer.set_string(row.right() - keys_width, row.y, &keys, fg(DIM));
            }
            let chapter: String = chapter.chars().take(usize::from(chapter_width)).collect();
            buffer.set_string(bar.right() + 1, row.y, chapter, fg(GREY));
        });
    });
    ctx.component(ids::PLAY, PlayButton, play);
    ctx.component(ids::SCRUBBER, Scrubber, bar);
}

/// The play and pause button: one glyph, which is what a click toggles.
struct PlayButton;

impl Component<Transport, Msg> for PlayButton {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, Transport, Msg>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, Transport>) {
        let icon = if ctx.state().running { " ▶" } else { " ‖" };
        let area = ctx.area();
        ctx.widget(ratatui::text::Span::styled(icon, fg(WHITE)), area);
        ctx.set_pointer_shape(PointerShape::Pointer);
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &Transport,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<Msg> {
        match event {
            Event::Mouse(mouse) if mouse.kind == MouseKind::Click(MouseButton::Left) => {
                EventResult::Emit(Msg::Toggle)
            }
            // The press is this button's, so focus stays on the scrubber.
            Event::Mouse(mouse) if mouse.kind == MouseKind::Down(MouseButton::Left) => {
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

/// The scrubber: the film as a line, with the playhead where the film is.
///
/// A press anywhere along it moves the film there, and holding the button
/// drags it. Focused, it takes the transport's keys: Space, the arrows, Home.
struct Scrubber;

impl Scrubber {
    /// The moment in the film a column of `area` stands for.
    fn moment(area: Rect, column: u16) -> f64 {
        let last = area.width.saturating_sub(1).max(1);
        let along = column.saturating_sub(area.x).min(last);
        f64::from(along) / f64::from(last) * score::END
    }
}

impl Component<Transport, Msg> for Scrubber {
    fn scope_options(&self, _state: &Transport) -> ScopeOptions {
        ScopeOptions::default().focusable(true)
    }

    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, Transport, Msg>) {}

    fn paint(&mut self, ctx: &mut PaintCtx<'_, Transport>) {
        let area = ctx.area();
        let shown = ctx.state().position.min(score::END);
        let head = (shown / score::END * f64::from(area.width.saturating_sub(1))) as u16;
        // A chapter is marked on the cell its first moment falls in.
        let cell = score::END / f64::from(area.width.max(1));
        ctx.with_buffer(area, |area, buffer| {
            for column in 0..area.width {
                let at = Self::moment(area, area.x + column);
                let starts_chapter = scenes::CHAPTERS
                    .iter()
                    .any(|(start, _)| *start > 0.0 && (at - start).abs() < cell / 2.0);
                let (glyph, color) = match column {
                    column if column == head => ("●", WHITE),
                    column if column < head => ("━", GREY),
                    _ if starts_chapter => ("┿", GREY),
                    _ => ("─", DIM),
                };
                buffer.set_string(area.x + column, area.y, glyph, fg(color));
            }
        });
        ctx.set_pointer_shape(if ctx.pointer_captured() {
            PointerShape::Grabbing
        } else {
            PointerShape::Pointer
        });
    }

    fn handle_event(
        &mut self,
        event: &Event,
        state: &Transport,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<Msg> {
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(MouseButton::Left) => {
                    // Held past the ends of the bar, the drag is still this one's.
                    ctx.capture_pointer(MouseButton::Left);
                    EventResult::Emit(Msg::Seek(Self::moment(ctx.area(), mouse.column)))
                }
                MouseKind::Drag(MouseButton::Left) => {
                    EventResult::Emit(Msg::Seek(Self::moment(ctx.area(), mouse.column)))
                }
                _ => EventResult::Ignored,
            },
            Event::Key(key) => {
                // Read now, not when the frame was declared: two arrow presses
                // between frames are two skips.
                let position = state.position.min(score::END);
                match key.code {
                    KeyCode::Char(' ') => EventResult::Emit(Msg::Toggle),
                    KeyCode::Left => EventResult::Emit(Msg::Seek((position - SKIP).max(0.0))),
                    KeyCode::Right => {
                        EventResult::Emit(Msg::Seek((position + SKIP).min(score::END)))
                    }
                    KeyCode::Home => EventResult::Emit(Msg::Replay),
                    _ => EventResult::Ignored,
                }
            }
            _ => EventResult::Ignored,
        }
    }
}
