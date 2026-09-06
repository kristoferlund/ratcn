//! The ratcn website as a terminal application.
//!
//! Two views under one header: the landing page, and a browser for every demo
//! in the repository. The browser embeds a demo the way the website embeds a
//! preview — the demo paints the pane it is handed and takes the input while
//! the user is inside it — which is the same contract `demo_shared::Demo`
//! already describes, so nothing here is special-cased per demo.

mod catalog;
mod chrome;
mod demos;

use std::{io, time::Duration};

use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::Style,
};
use ratcn::{
    Theme,
    runtime::{Event, EventResult, FocusState, KeyCode, MouseButton, MouseKind, Ratcn, TabWrap},
};

use catalog::Embedded;

/// Which page is showing.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum View {
    #[default]
    Landing,
    Demos,
}

/// Where the chrome's focus is parked while the embedded demo has the input.
///
/// The chrome declares no such child, and that is the point: an engine whose
/// focus names nothing resolves to its first focusable leaf, so leaving the
/// real path in place — or clearing it — would paint a chrome focus ring beside
/// the demo's own. A path naming no declared component is left alone.
const PARKED_FOCUS: &str = "demo-has-the-input";

#[derive(Default)]
struct AppState {
    focus: FocusState,
    view: View,
    /// The row the nav cursor is on, as an index into [`catalog::ENTRIES`].
    /// There is no separate selection: the cursor is the choice.
    selected: usize,
    /// The nav list's top item.
    scroll: usize,
    /// The embedded demo has the input.
    entered: bool,
    /// The chrome's own focus, stashed while [`entered`](Self::entered).
    parked: Option<FocusState>,
}

enum Msg {
    FocusChanged(FocusState),
    Navigate(View),
    DemoFocused(usize, usize),
    DemoScrolled(usize),
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
    /// The landing page's own instance, which is not the catalog's `landing`
    /// entry: browsing to that one leaves this one as the user left it.
    landing: Box<dyn Embedded>,
    /// Catalog demos, built on first use and kept afterwards, so a demo
    /// returned to still has its state. Nothing is built up front: `effects`
    /// makes a network request the moment it is constructed.
    opened: Vec<Option<Box<dyn Embedded>>>,
    /// Where the embedded demo painted last frame — what a click is tested
    /// against, and the only thing routing needs to know about the layout.
    pane: Rect,
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new()
                .focus(|state: &AppState| &state.focus, Msg::FocusChanged)
                .tab_wrap(TabWrap::Wrap),
            landing: Box::new(landing::App::new()),
            opened: std::iter::repeat_with(|| None)
                .take(catalog::ENTRIES.len())
                .collect(),
            pane: Rect::ZERO,
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => self.state.focus = focus,
            Msg::Navigate(view) => self.state.view = view,
            Msg::DemoFocused(index, offset) => {
                self.state.selected = index;
                self.state.scroll = offset;
            }
            Msg::DemoScrolled(offset) => self.state.scroll = offset,
        }
    }

    /// The demo the current view shows, built if this is its first appearance.
    fn shown_mut(&mut self) -> &mut dyn Embedded {
        match self.state.view {
            View::Landing => self.landing.as_mut(),
            View::Demos => {
                let index = self.state.selected;
                self.opened[index]
                    .get_or_insert_with(|| catalog::ENTRIES[index].open())
                    .as_mut()
            }
        }
    }

    /// The demo on screen, if it has been built — never building one, so asking
    /// what the clock owes cannot start a network request.
    fn shown(&self) -> Option<&dyn Embedded> {
        match self.state.view {
            View::Landing => Some(self.landing.as_ref()),
            View::Demos => self.opened[self.state.selected].as_deref(),
        }
    }

    /// Hand the input to the embedded demo, parking the chrome's focus.
    fn enter(&mut self) {
        let chrome_focus =
            std::mem::replace(&mut self.state.focus, FocusState::intent([PARKED_FOCUS]));
        self.state.parked = Some(chrome_focus);
        self.state.entered = true;
    }

    /// Take it back, putting the chrome's focus where it was.
    fn leave(&mut self) {
        if let Some(focus) = self.state.parked.take() {
            self.state.focus = focus;
        }
        self.state.entered = false;
    }

    /// Enter the demo pane on a key the chrome passed on. Only the Demos view
    /// has a key for it; the landing page is entered by clicking into it.
    fn enter_key(&mut self, event: &Event) -> bool {
        let Event::Key(key) = event else {
            return false;
        };
        if self.state.view != View::Demos || !matches!(key.code, KeyCode::Enter | KeyCode::Right) {
            return false;
        }
        self.enter();
        true
    }
}

impl demo_shared::Demo for App {
    /// So a paste reaches an embedded demo that wants one.
    const PASTE: bool = true;

    /// Paint with the terminal's own colors, falling back to `THEME`. Each
    /// embedded demo then resolves its own theme from that.
    const ADAPTIVE: bool = true;

    fn handle_event(&mut self, event: Event) -> bool {
        if self.state.entered {
            match &event {
                // Esc is the demo's first: a dialog inside it dismisses on the
                // key, and only an Esc the demo ignored gives the input back.
                Event::Key(key) if key.code == KeyCode::Esc => {
                    if self.shown_mut().handle_event(event) {
                        return true;
                    }
                    self.leave();
                    return true;
                }
                // A press outside the pane means the user is done with the
                // demo, and the event that says so is the chrome's. Nothing
                // else out there is: a move, a wheel, or a drag that strays
                // past the edge still belongs to the demo, which may have
                // captured the pointer and be waiting for the release.
                Event::Mouse(mouse)
                    if mouse.kind == MouseKind::Down(MouseButton::Left)
                        && !self.pane.contains(Position::new(mouse.column, mouse.row)) =>
                {
                    self.leave();
                }
                _ => return self.shown_mut().handle_event(event),
            }
        } else if let Event::Mouse(mouse) = &event
            && mouse.kind == MouseKind::Down(MouseButton::Left)
            && self.pane.contains(Position::new(mouse.column, mouse.row))
        {
            // The press both enters and reaches the demo, so a control under
            // the pointer takes one click rather than two.
            self.enter();
            self.shown_mut().handle_event(event);
            return true;
        }

        match self.ratcn.handle_event(event.clone(), &self.state) {
            EventResult::Emit(msg) => {
                self.update(msg);
                true
            }
            EventResult::Consumed => true,
            EventResult::Ignored => self.enter_key(&event),
        }
    }

    /// Whatever the demo on screen asks of the clock.
    fn wake(&self) -> Option<Duration> {
        self.shown().and_then(Embedded::wake)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(theme.background));

        let Some(frames) = chrome::layout(area) else {
            self.pane = Rect::ZERO;
            chrome::too_small(frame, area, theme);
            return;
        };

        let columns = match self.state.view {
            // The landing page fills the body; the browser splits it.
            View::Landing => None,
            View::Demos => Some(chrome::columns(frames.body)),
        };
        self.pane = columns.as_ref().map_or(frames.body, |columns| columns.pane);
        chrome::separators(
            frame.buffer_mut(),
            &frames,
            columns.as_ref().map(|columns| columns.rule.x),
            theme,
            self.state.entered,
        );

        let state = &self.state;
        self.ratcn.render(frame, state, theme, |ctx| {
            chrome::declare(ctx, state, frames.header);
            if let Some(columns) = &columns {
                demos::declare(ctx, columns.nav, state.entered);
            }
        });

        // Outside the render closure: that closure borrows the state, and a
        // demo needs `&mut` to paint.
        let pane = self.pane;
        let demo = self.shown_mut();
        let demo_theme = demo.theme(theme);
        demo.draw(frame, pane, &demo_theme);
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
