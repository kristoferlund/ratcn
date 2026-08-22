//! Fixtures the crate's own tests share: a terminal-backed driver and the
//! event constructors that go with it.

use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::{Buffer, Cell},
    layout::Rect,
};

use crate::runtime::{
    DeclareCtx, Event, EventResult, KeyCode, KeyEvent, Modifiers, MouseEvent, MouseKind, Ratcn,
};
use crate::theme::Theme;

/// A [`Ratcn`] wired to a `TestBackend` terminal.
///
/// One frame is one [`render`](Driver::render); what it painted is read back
/// with [`row`](Driver::row), [`cell`](Driver::cell), or [`buffer`](Driver::buffer).
pub(crate) struct Driver<State, Msg> {
    pub(crate) terminal: Terminal<TestBackend>,
    pub(crate) ratcn: Ratcn<State, Msg>,
}

impl<State, Msg> Driver<State, Msg> {
    /// A driver over a `width` × `height` terminal.
    pub(crate) fn new(width: u16, height: u16) -> Self {
        Self::with(Ratcn::new(), width, height)
    }

    /// The same, for a runtime the test configured first.
    pub(crate) fn with(ratcn: Ratcn<State, Msg>, width: u16, height: u16) -> Self {
        Self {
            terminal: Terminal::new(TestBackend::new(width, height)).expect("terminal"),
            ratcn,
        }
    }

    /// The whole terminal, which is the area the root closure declares into.
    pub(crate) fn area(&self) -> Rect {
        self.buffer().area
    }

    /// Declare and paint one frame with the dark theme.
    pub(crate) fn render(
        &mut self,
        state: &State,
        declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>),
    ) {
        let theme = Theme::default_dark();
        let Self { terminal, ratcn } = self;
        terminal
            .draw(|frame| ratcn.render(frame, state, &theme, declare))
            .expect("draw");
    }

    /// Route one event through the retained surface.
    pub(crate) fn event(&mut self, event: Event, state: &State) -> EventResult<Msg> {
        self.ratcn.handle_event(event, state)
    }

    /// What the last frame painted.
    pub(crate) fn buffer(&self) -> &Buffer {
        self.terminal.backend().buffer()
    }

    /// The symbols on one painted row, left to right.
    pub(crate) fn row(&self, row: u16) -> String {
        let buffer = self.buffer();
        (0..buffer.area.width)
            .map(|column| buffer.cell((column, row)).expect("cell").symbol())
            .collect()
    }

    /// One painted cell, with the style it was painted in.
    pub(crate) fn cell(&self, column: u16, row: u16) -> &Cell {
        self.buffer().cell((column, row)).expect("cell")
    }
}

/// A mouse event at one cell, with no modifiers held.
pub(crate) fn mouse(kind: MouseKind, column: u16, row: u16) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: Modifiers::NONE,
    })
}

/// A key event with no modifiers held.
pub(crate) fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code))
}

/// A key event with `modifiers` held.
pub(crate) fn key_with(code: KeyCode, modifiers: Modifiers) -> Event {
    Event::Key(KeyEvent { code, modifiers })
}
