//! The ratcn website as a terminal application.
//!
//! Two views under one header: the landing page, and a browser for every demo
//! in the repository. Both embed a demo the way the website embeds a preview —
//! the demo paints the region it is handed and takes the input while the user
//! is inside it — which is the same contract `demo_shared::Demo` already
//! describes, so nothing here is special-cased per demo.

mod catalog;
mod chrome;
mod demos;
mod page;

use std::{io, time::Duration};

use ratatui::{
    Frame, Terminal,
    backend::TestBackend,
    layout::{Position, Rect},
    style::Style,
};
use ratcn::{
    Theme, Toast, ToasterState, ToasterWidget,
    runtime::{
        Event, EventResult, FocusState, KeyCode, MouseButton, MouseEvent, MouseKind, Ratcn, TabWrap,
    },
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
    /// The landing page's first visible content row.
    page_scroll: u16,
    /// The embedded demo has the input.
    entered: bool,
    /// The chrome's own focus, stashed while [`entered`](Self::entered).
    parked: Option<FocusState>,
    toasts: ToasterState<'static>,
}

enum Msg {
    FocusChanged(FocusState),
    Navigate(View),
    DemoFocused(usize, usize),
    DemoScrolled(usize),
    PageScrolled(u16),
    Open(&'static str),
}

/// Where an embedded demo is on screen, and what its own top-left maps to
/// there.
#[derive(Clone, Copy)]
struct Pane {
    rect: Rect,
    /// The demo-space cell showing at `rect`'s top-left.
    origin: Position,
}

impl Pane {
    /// A demo that owns its screen rect outright, so there is nothing to
    /// translate.
    const fn identity(rect: Rect) -> Self {
        Self {
            rect,
            origin: Position::new(rect.x, rect.y),
        }
    }

    /// `mouse` in the demo's own coordinate space.
    ///
    /// The shift is a constant rather than a clip to the rect, which is what
    /// lets a drag that strays outside the region keep reaching the cell it
    /// means. The hits that buys outside the region cost nothing: the landing
    /// canvas begins with the demo's own top padding, and rows past its end
    /// are never blitted.
    fn translate(self, mouse: MouseEvent) -> MouseEvent {
        MouseEvent {
            column: shift(mouse.column, self.rect.x, self.origin.x),
            row: shift(mouse.row, self.rect.y, self.origin.y),
            ..mouse
        }
    }
}

/// What the landing page measured to last frame: the rows its viewport shows,
/// and the rows it has. Retained because the page keys are the app's on that
/// view, and the app cannot say how far a page is without both.
#[derive(Clone, Copy, Default)]
struct PageView {
    viewport: u16,
    content: u16,
}

/// `value` moved by the constant `origin - anchor`, saturating at the ends.
const fn shift(value: u16, anchor: u16, origin: u16) -> u16 {
    if origin >= anchor {
        value.saturating_add(origin - anchor)
    } else {
        value.saturating_sub(anchor - origin)
    }
}

struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
    /// The landing page's own instance, which is not the catalog's `landing`
    /// entry: browsing to that one leaves this one as the user left it.
    landing: Box<dyn Embedded>,
    /// The offscreen canvas the landing demo paints, sized to itself. A demo
    /// owns its own runtime over its own state, so it cannot be declared
    /// inside the page's viewport; the rows the page shows are copied out of
    /// this instead.
    canvas: Terminal<TestBackend>,
    /// Catalog demos, built on first use and kept afterwards, so a demo
    /// returned to still has its state. Nothing is built up front: `effects`
    /// makes a network request the moment it is constructed.
    opened: Vec<Option<Box<dyn Embedded>>>,
    /// Where the embedded demo painted last frame, and how to reach its own
    /// cells from there — the only thing routing needs to know about the
    /// layout. [`None`] when no demo is on screen.
    pane: Option<Pane>,
    /// What the landing page measured to last frame.
    page_view: PageView,
}

impl App {
    fn new() -> Self {
        Self {
            state: AppState::default(),
            ratcn: Ratcn::new()
                .focus(|state: &AppState| &state.focus, Msg::FocusChanged)
                .tab_wrap(TabWrap::Wrap),
            landing: Box::new(landing::App::new()),
            canvas: Terminal::new(TestBackend::new(1, 1)).expect("an offscreen canvas opens"),
            opened: std::iter::repeat_with(|| None)
                .take(catalog::ENTRIES.len())
                .collect(),
            pane: None,
            page_view: PageView::default(),
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => self.state.focus = focus,
            Msg::Navigate(view) => {
                self.state.view = view;
                // The nav list is what the Demos view is for, so focusing it
                // is what makes the arrows work the moment the user arrives.
                // The landing page keeps the focus the user pressed to get
                // here: a control focused inside it would be revealed, and a
                // page that opens scrolled to its own middle reads as broken.
                if view == View::Demos {
                    self.state.focus = FocusState::intent([demos::LIST_ID]);
                }
            }
            Msg::DemoFocused(index, offset) => {
                self.state.selected = index;
                self.state.scroll = offset;
            }
            Msg::DemoScrolled(offset) => self.state.scroll = offset,
            Msg::PageScrolled(offset) => self.state.page_scroll = offset,
            Msg::Open(url) => {
                // A browser that opens behind the terminal, and an opener that
                // is not installed, look the same from here: nothing happens.
                // So say what happened either way.
                let toast = match open_url(url) {
                    Ok(()) => Toast::success("Opened in your browser").with_description(url),
                    Err(error) => Toast::error("Could not open a browser").with_description(error),
                };
                self.state.toasts.push(toast, demo_shared::monotonic_time());
            }
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

    /// Route one event to the embedded demo, in the demo's own coordinates.
    fn route_to_demo(&mut self, event: Event) -> bool {
        let event = match (event, self.pane) {
            (Event::Mouse(mouse), Some(pane)) => Event::Mouse(pane.translate(mouse)),
            (event, _) => event,
        };
        self.shown_mut().handle_event(event)
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

    /// Whether `position` is inside the embedded demo.
    fn over_demo(&self, position: Position) -> bool {
        self.pane.is_some_and(|pane| pane.rect.contains(position))
    }

    /// Enter the demo region on a key the chrome passed on. Only the Demos view
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

    /// Scroll the landing page on a page key the chrome passed on.
    ///
    /// The page is the whole view, so its scroll keys belong to the app — the
    /// same place the `landing` demo keeps its own alt-chords. Whenever focus
    /// is inside the page the scroll area answers these keys itself and this
    /// never runs; the Demos view has no page to scroll and never reaches it.
    fn page_key(&mut self, event: &Event) -> bool {
        let Event::Key(key) = event else {
            return false;
        };
        if self.state.view != View::Landing || key.modifiers.any() {
            return false;
        }
        let Some(offset) = page::scrolled(
            key.code,
            self.state.page_scroll,
            self.page_view.viewport,
            self.page_view.content,
        ) else {
            return false;
        };
        self.update(Msg::PageScrolled(offset));
        true
    }

    /// The Demos view: the nav column, the rule, and the selected demo.
    fn draw_demos(&mut self, frame: &mut Frame, frames: &chrome::Frames, theme: &Theme) {
        let columns = chrome::columns(frames.body);
        self.pane = Some(Pane::identity(columns.pane));
        chrome::separators(
            frame.buffer_mut(),
            frames,
            Some(columns.rule.x),
            theme,
            self.state.entered,
        );

        let state = &self.state;
        self.ratcn.render(frame, state, theme, |ctx| {
            chrome::declare(ctx, state, frames.header);
            demos::declare(ctx, columns.nav, state.entered);
        });

        // Outside the render closure: that closure borrows the state, and a
        // demo needs `&mut` to paint.
        let pane = columns.pane;
        let demo = self.shown_mut();
        let demo_theme = demo.theme(theme);
        demo.draw(frame, pane, &demo_theme);
    }

    /// The landing view: the site's front page, scrolling, with the demo blitted
    /// into its preview window.
    fn draw_landing(&mut self, frame: &mut Frame, frames: &chrome::Frames, theme: &Theme) {
        // The scroll area always keeps a gutter column for its scrollbar.
        let layout = page::page(frames.body.width.saturating_sub(1));
        // The area clamps the offset it lays out from to what the content can
        // actually scroll; the blit has to clamp it the same way or a resize
        // would leave the two reading different rows.
        let offset = self
            .state
            .page_scroll
            .min(layout.height.saturating_sub(frames.body.height));
        let interior = page::interior(layout.window);
        let column = Rect::new(
            frames.body.x + interior.x,
            frames.body.y,
            interior.width,
            frames.body.height,
        );
        self.page_view = PageView {
            viewport: frames.body.height,
            content: layout.height,
        };
        let placed = page::placement(column, interior.y, interior.height, offset);
        self.pane = placed.map(|(rect, source)| Pane {
            rect,
            origin: Position::new(0, source),
        });

        chrome::separators(frame.buffer_mut(), frames, None, theme, self.state.entered);

        let state = &self.state;
        self.ratcn.render(frame, state, theme, |ctx| {
            chrome::declare(ctx, state, frames.header);
            page::declare(ctx, frames.body, layout, state.entered);
        });

        let Some((rect, source)) = placed else {
            return;
        };
        let Self {
            canvas, landing, ..
        } = self;
        let demo_theme = landing.theme(theme);
        canvas.backend_mut().resize(interior.width, interior.height);
        let painted = canvas
            .draw(|canvas| landing.draw(canvas, canvas.area(), &demo_theme))
            .expect("an offscreen canvas cannot fail to draw")
            .buffer;
        page::blit(frame.buffer_mut(), painted, rect, source);
    }
}

/// Open `url` in the user's browser.
///
/// # Errors
///
/// A message naming what went wrong, ready to put in a toast.
#[cfg(not(target_arch = "wasm32"))]
fn open_url(url: &str) -> Result<(), String> {
    use std::process::{Command, Stdio};

    let (program, leading): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("open", &[])
    } else if cfg!(target_os = "windows") {
        // `start` takes a window title first, and an empty one keeps a URL
        // with spaces from being read as one.
        ("cmd", &["/c", "start", ""])
    } else {
        ("xdg-open", &[])
    };
    Command::new(program)
        .args(leading)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|error| format!("{program}: {error}"))
}

/// The browser has no opener to spawn, so the toast carries the address
/// instead.
#[cfg(target_arch = "wasm32")]
fn open_url(url: &str) -> Result<(), String> {
    Err(url.to_owned())
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
                    if self.route_to_demo(event) {
                        return true;
                    }
                    self.leave();
                    return true;
                }
                // A press outside the region means the user is done with the
                // demo, and the event that says so is the chrome's. Nothing
                // else out there is: a move, a wheel, or a drag that strays
                // past the edge still belongs to the demo, which may have
                // captured the pointer and be waiting for the release.
                Event::Mouse(mouse)
                    if mouse.kind == MouseKind::Down(MouseButton::Left)
                        && !self.over_demo(Position::new(mouse.column, mouse.row)) =>
                {
                    self.leave();
                }
                _ => {
                    if self.route_to_demo(event.clone()) {
                        return true;
                    }
                    // A key the demo ignored stops here; a mouse event falls
                    // through, which is what keeps the wheel scrolling the
                    // page under an embedded demo that does not scroll.
                    if !matches!(event, Event::Mouse(_)) {
                        return false;
                    }
                }
            }
        } else if let Event::Mouse(mouse) = &event
            && mouse.kind == MouseKind::Down(MouseButton::Left)
            && self.over_demo(Position::new(mouse.column, mouse.row))
        {
            // The press both enters and reaches the demo, so a control under
            // the pointer takes one click rather than two.
            self.enter();
            self.route_to_demo(event);
            return true;
        }

        match self.ratcn.handle_event(event.clone(), &self.state) {
            EventResult::Emit(msg) => {
                self.update(msg);
                true
            }
            EventResult::Consumed => true,
            // The two view-level fallbacks, each inert on the other's view.
            EventResult::Ignored => self.enter_key(&event) || self.page_key(&event),
        }
    }

    /// Whatever the demo on screen asks of the clock, and whatever the toasts
    /// need to expire on time.
    fn wake(&self) -> Option<Duration> {
        let demo = self.shown().and_then(Embedded::wake);
        let expiry = self
            .state
            .toasts
            .time_until_next_expiry(demo_shared::monotonic_time());
        match (demo, expiry) {
            (Some(demo), Some(expiry)) => Some(demo.min(expiry)),
            (demo, expiry) => demo.or(expiry),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let now = demo_shared::monotonic_time();
        let _ = self.state.toasts.prune_expired(now);
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(theme.background));

        let Some(frames) = chrome::layout(area) else {
            self.pane = None;
            chrome::too_small(frame, area, theme);
            return;
        };

        match self.state.view {
            View::Demos => self.draw_demos(frame, &frames, theme),
            View::Landing => self.draw_landing(frame, &frames, theme),
        }

        // Last, over the blit: a toast is the app talking, and nothing the
        // page or a demo paints belongs on top of it.
        frame.render_widget(
            ToasterWidget::new(&self.state.toasts, now).themed(theme),
            area,
        );
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}
