//! The ratcn website as a terminal application.
//!
//! Two views under one header: the landing page, and a browser for every demo
//! in the repository. Both embed a demo the way the website embeds a preview —
//! it takes the input while the user is inside it — and both do it the same
//! way, through [`App::blit_demo`].

mod catalog;
mod chrome;
// Temporary scaffolding: the module ships ahead of the Getting started page
// that renders its snippets. Delete this allow the moment the page calls it —
// it is not a standing exemption.
#[allow(
    dead_code,
    reason = "temporary: nothing calls this until the Getting started page lands in a later slice"
)]
mod code;
mod demos;
mod page;

use std::{io, time::Duration};

use ratatui::{
    Frame, Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::{Position, Rect, Size},
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
    nav_scroll: usize,
    /// The landing page's first visible content row.
    page_scroll: u16,
    /// The chrome's own focus, stashed while the demo has the input.
    parked: Option<FocusState>,
    toasts: ToasterState<'static>,
}

impl AppState {
    /// Whether the embedded demo has the input.
    ///
    /// Derived rather than stored: it is true exactly while the chrome's focus
    /// is parked, and two fields set and cleared together are one fact with
    /// nothing to keep them honest.
    const fn entered(&self) -> bool {
        self.parked.is_some()
    }
}

enum Msg {
    FocusChanged(FocusState),
    Navigate(View),
    NavFocused(usize, usize),
    NavScrolled(usize),
    PageScrolled(u16),
    OpenUrl(&'static str),
}

/// Where the embedded demo is on screen, and which of its own rows that is.
#[derive(Clone, Copy)]
struct Embed {
    /// The demo's visible rows, on screen.
    rect: Rect,
    /// The demo's own row showing at `rect`'s top. Its left column is always
    /// the demo's column 0: nothing here scrolls horizontally, so a row is the
    /// whole of what the region can be offset by.
    source_row: u16,
}

impl Embed {
    /// `mouse` in the demo's own coordinate space.
    ///
    /// The shift is a constant rather than a clip to the rect, which is what
    /// lets a drag that strays outside the region keep reaching the cell it
    /// means — the demo may have captured the pointer and be waiting for the
    /// release. But a constant shift also maps cells outside the region onto
    /// cells inside it, and for the kinds that only drive hover, landing on a
    /// control the pointer is nowhere near is simply a lie. Those get a cell
    /// no canvas contains.
    fn translate(self, mouse: MouseEvent) -> MouseEvent {
        let hover_only = matches!(mouse.kind, MouseKind::Moved | MouseKind::Scroll(_));
        if hover_only && !self.rect.contains(Position::new(mouse.column, mouse.row)) {
            return MouseEvent {
                column: u16::MAX,
                row: u16::MAX,
                ..mouse
            };
        }
        MouseEvent {
            column: shift(mouse.column, self.rect.x, 0),
            row: shift(mouse.row, self.rect.y, self.source_row),
            ..mouse
        }
    }
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
    /// Catalog demos, built on first use and kept afterwards, so a demo
    /// returned to still has its state.
    ///
    /// Nothing is built up front, and that is not an optimisation:
    /// `effects::App::new` starts a network request, so constructing all
    /// twenty-six would fetch on launch.
    opened: Vec<Option<Box<dyn Embedded>>>,
    /// The offscreen surface every embedded demo paints, resized to whatever
    /// the demo is given. See [`App::blit_demo`].
    canvas: Terminal<TestBackend>,
    /// Where the embedded demo painted last frame, and how to reach its own
    /// cells from there — the only thing routing needs to know about the
    /// layout. [`None`] when no demo is on screen.
    embed: Option<Embed>,
    /// The landing page as it was last measured, which is what its page keys
    /// and its focus reveal are answered from. [`None`] until it has been
    /// drawn once.
    page: Option<page::PageLayout>,
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
            canvas: Terminal::new(TestBackend::new(1, 1)).expect("an offscreen canvas opens"),
            embed: None,
            page: None,
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => {
                if let Some(offset) = self.page.and_then(|page| page.reveal(&focus)) {
                    self.state.page_scroll = offset;
                }
                self.state.focus = focus;
            }
            Msg::Navigate(view) => {
                self.state.view = view;
                // The nav list is what the Demos view is for, so focusing it
                // is what makes the arrows work the moment the user arrives.
                // The landing page is left alone; see `page::ID`.
                if view == View::Demos {
                    self.state.focus = FocusState::intent([demos::NAV_ID]);
                }
            }
            Msg::NavFocused(index, offset) => {
                self.state.selected = index;
                self.state.nav_scroll = offset;
            }
            Msg::NavScrolled(offset) => self.state.nav_scroll = offset,
            Msg::PageScrolled(offset) => self.state.page_scroll = offset,
            Msg::OpenUrl(url) => {
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
        let Self {
            state,
            landing,
            opened,
            ..
        } = self;
        shown_in(state, landing, opened)
    }

    /// The demo on screen, if it has been built. Never builds one — see
    /// [`App::opened`] for why that matters.
    fn shown(&self) -> Option<&dyn Embedded> {
        match self.state.view {
            View::Landing => Some(self.landing.as_ref()),
            View::Demos => self.opened[self.state.selected].as_deref(),
        }
    }

    /// Route one event to the embedded demo, in the demo's own coordinates.
    ///
    /// A demo with no rows on screen is not reachable by the pointer at all —
    /// the page has scrolled its preview out of the viewport, or the window has
    /// shrunk below the chrome's minimum — and it can still hold the input in
    /// both states. There is no cell to name for it, so the event is dropped
    /// rather than invented; keys still reach it, which is what lets Esc give
    /// the input back.
    fn route_to_demo(&mut self, event: Event) -> bool {
        let event = match (event, self.embed) {
            (Event::Mouse(mouse), Some(embed)) => Event::Mouse(embed.translate(mouse)),
            (Event::Mouse(_), None) => return false,
            (event, _) => event,
        };
        self.shown_mut().handle_event(event)
    }

    /// Hand the input to the embedded demo, parking the chrome's focus.
    fn enter(&mut self) {
        let chrome_focus =
            std::mem::replace(&mut self.state.focus, FocusState::intent([PARKED_FOCUS]));
        self.state.parked = Some(chrome_focus);
    }

    /// Take it back, putting the chrome's focus where it was.
    fn leave(&mut self) {
        if let Some(focus) = self.state.parked.take() {
            self.state.focus = focus;
        }
    }

    /// Whether `position` is inside the embedded demo.
    fn over_demo(&self, position: Position) -> bool {
        self.embed
            .is_some_and(|embed| embed.rect.contains(position))
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

    /// Scroll the landing page on a page key the chrome passed on. See
    /// [`page::PageLayout::scrolled`].
    fn page_key(&mut self, event: &Event) -> bool {
        let (Event::Key(key), Some(page)) = (event, self.page) else {
            return false;
        };
        if self.state.view != View::Landing || key.modifiers.any() {
            return false;
        }
        let Some(offset) = page.scrolled(key.code) else {
            return false;
        };
        self.update(Msg::PageScrolled(offset));
        true
    }

    /// Paint the demo the current view shows onto the offscreen canvas, and
    /// copy the rows `embed` asks for onto the screen.
    ///
    /// A demo cannot be declared inside this app's own render pass: it owns a
    /// runtime over its own state, and `Ratcn::render` builds a surface from
    /// `frame.area()`, so a `Select` panel or a `Tooltip` bubble would be
    /// placed against the whole terminal and paint straight over the chrome.
    /// Giving the demo a surface of exactly its own size makes `frame.area()`
    /// *be* the demo's area, so the escape stops being possible rather than
    /// merely unlikely.
    ///
    /// `Terminal<TestBackend>` is that surface because `Frame` has no public
    /// constructor: a terminal over some backend is the only way ratatui
    /// offers to get one. The cost is the cursor position, which a canvas
    /// cannot carry back — no component sets one today, and a demo that wanted
    /// a text caret would need this reconsidered.
    fn blit_demo(&mut self, frame: &mut Frame, canvas: Size, embed: Embed, theme: &Theme) {
        let Self {
            state,
            landing,
            opened,
            canvas: surface,
            ..
        } = self;
        let demo = shown_in(state, landing, opened);
        let demo_theme = demo.theme(theme);
        surface.backend_mut().resize(canvas.width, canvas.height);
        let painted = surface
            .draw(|offscreen| demo.draw(offscreen, offscreen.area(), &demo_theme))
            .expect("an offscreen canvas cannot fail to draw")
            .buffer;
        blit(frame.buffer_mut(), painted, embed.rect, embed.source_row);
    }

    /// The Demos view: the nav column, the rules, and the selected demo.
    fn draw_demos(&mut self, frame: &mut Frame, bands: &chrome::Bands, theme: &Theme) {
        let columns = demos::columns(bands.body);
        let embed = Embed {
            rect: columns.pane,
            source_row: 0,
        };
        self.embed = Some(embed);
        self.page = None;

        chrome::header_rule(frame.buffer_mut(), bands, theme);
        demos::separators(
            frame.buffer_mut(),
            bands,
            &columns,
            theme,
            self.state.entered(),
        );

        let state = &self.state;
        self.ratcn.render(frame, state, theme, |ctx| {
            chrome::declare(ctx, state, bands.header);
            demos::declare(ctx, columns.nav);
        });

        self.blit_demo(frame, columns.pane.as_size(), embed, theme);
    }

    /// The landing view: the site's front page, scrolling, with the demo
    /// blitted into its preview window.
    fn draw_landing(&mut self, frame: &mut Frame, bands: &chrome::Bands, theme: &Theme) {
        let page = page::layout(bands.body, self.state.page_scroll);
        self.page = Some(page);
        self.embed = page
            .embed
            .map(|(rect, source_row)| Embed { rect, source_row });

        chrome::header_rule(frame.buffer_mut(), bands, theme);

        let state = &self.state;
        self.ratcn.render(frame, state, theme, |ctx| {
            chrome::declare(ctx, state, bands.header);
            page::declare(ctx, bands.body, page, state.entered());
        });

        if let Some(embed) = self.embed {
            self.blit_demo(frame, page.canvas, embed, theme);
        }
    }
}

/// The demo `state`'s view shows, out of the two places demos are kept, built
/// if this is its first appearance.
///
/// Free rather than a method so a caller can hold the canvas at the same time.
fn shown_in<'a>(
    state: &AppState,
    landing: &'a mut Box<dyn Embedded>,
    opened: &'a mut [Option<Box<dyn Embedded>>],
) -> &'a mut dyn Embedded {
    match state.view {
        View::Landing => landing.as_mut(),
        View::Demos => opened[state.selected]
            .get_or_insert_with(|| catalog::ENTRIES[state.selected].open())
            .as_mut(),
    }
}

/// Copy `target.height` rows of `source`, starting at its row `source_y`, into
/// `target`.
fn blit(destination: &mut Buffer, source: &Buffer, target: Rect, source_y: u16) {
    for row in 0..target.height {
        for column in 0..target.width {
            let Some(cell) = source.cell(Position::new(column, source_y + row)) else {
                continue;
            };
            destination[(target.x + column, target.y + row)] = cell.clone();
        }
    }
}

/// Open `url` in the user's browser.
///
/// # Errors
///
/// A message naming what went wrong, ready to put in a toast.
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

impl demo_shared::Demo for App {
    /// So a paste reaches an embedded demo that wants one.
    const PASTE: bool = true;

    /// Paint with the terminal's own colors, falling back to `THEME`. Each
    /// embedded demo then resolves its own theme from that.
    const ADAPTIVE: bool = true;

    fn handle_event(&mut self, event: Event) -> bool {
        if self.state.entered() {
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
                // else out there is: a drag that strays past the edge still
                // belongs to the demo, which may have captured the pointer and
                // be waiting for the release.
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
                    // Only the wheel falls through, and only so the page keeps
                    // scrolling under a demo that does not scroll. Any other
                    // event reaching the chrome would take focus off the parked
                    // path on the press, which is the whole reason for parking.
                    if !matches!(&event, Event::Mouse(mouse) if matches!(mouse.kind, MouseKind::Scroll(_)))
                    {
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

        let Some(bands) = chrome::layout(area) else {
            self.embed = None;
            self.page = None;
            chrome::too_small(frame, area, theme);
            return;
        };

        match self.state.view {
            View::Demos => self.draw_demos(frame, &bands, theme),
            View::Landing => self.draw_landing(frame, &bands, theme),
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

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use ratcn::runtime::{KeyEvent, ScrollDirection};

    use super::*;

    /// A demo that records what it was handed and answers what a test needs.
    #[derive(Default)]
    struct Probe {
        seen: Rc<Cell<Option<Event>>>,
        handled: Rc<Cell<bool>>,
        /// Paint every cell of the frame rather than staying inside `area` —
        /// what a component's floating layers do near an edge.
        greedy: Rc<Cell<bool>>,
    }

    /// What a greedy probe paints, in every cell it can reach.
    const GREED: &str = "#";

    impl demo_shared::Demo for Probe {
        fn draw(&mut self, frame: &mut Frame, _area: Rect, _theme: &Theme) {
            if !self.greedy.get() {
                return;
            }
            let all = frame.area();
            for row in all.top()..all.bottom() {
                for column in all.left()..all.right() {
                    frame.buffer_mut()[(column, row)].set_symbol(GREED);
                }
            }
        }

        fn handle_event(&mut self, event: Event) -> bool {
            self.seen.set(Some(event));
            self.handled.get()
        }
    }

    /// An app with a probe in place of the demo it shows.
    struct Probed {
        app: App,
        /// The last event the demo was handed.
        seen: Rc<Cell<Option<Event>>>,
        /// What the demo reports for the next event it gets.
        handled: Rc<Cell<bool>>,
        /// Whether the demo paints its whole frame.
        greedy: Rc<Cell<bool>>,
    }

    /// An app showing the landing view with a probe in place of the demo, and
    /// one frame already drawn so the chrome has a surface to route against.
    fn probed() -> Probed {
        let seen = Rc::<Cell<Option<Event>>>::default();
        let handled = Rc::<Cell<bool>>::default();
        let greedy = Rc::<Cell<bool>>::default();
        let mut app = App::new();
        app.landing = Box::new(Probe {
            seen: Rc::clone(&seen),
            handled: Rc::clone(&handled),
            greedy: Rc::clone(&greedy),
        });
        draw_at(&mut app, 100, 40);
        Probed {
            app,
            seen,
            handled,
            greedy,
        }
    }

    /// Draw one frame at `width` × `height`, and hand back what was painted.
    fn draw_at(app: &mut App, width: u16, height: u16) -> Buffer {
        let mut terminal =
            Terminal::new(TestBackend::new(width, height)).expect("a test backend opens");
        terminal
            .draw(|frame| demo_shared::Demo::draw(app, frame, frame.area(), &Theme::default_dark()))
            .expect("the test backend draws")
            .buffer
            .clone()
    }

    fn text_of(buffer: &Buffer) -> String {
        (0..buffer.area.height)
            .flat_map(|row| (0..buffer.area.width).map(move |column| (column, row)))
            .map(|cell| buffer[cell].symbol().to_owned())
            .collect()
    }

    /// The app's own event routing, named explicitly: `App` implements both
    /// `Demo` and, through the blanket impl, `Embedded`.
    fn route(app: &mut App, event: Event) -> bool {
        demo_shared::Demo::handle_event(app, event)
    }

    fn mouse(kind: MouseKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: ratcn::runtime::Modifiers::default(),
        })
    }

    /// The one place in this crate where a mistake is a panic rather than a
    /// wrong pixel: base-layer paint is not clipped, so a chrome laid out in an
    /// area too small for it writes outside the buffer.
    #[test]
    fn the_layout_guard_is_what_keeps_a_small_window_from_painting_outside_it() {
        let min = chrome::min_size();
        let mut app = App::new();

        let laid_out = text_of(&draw_at(&mut app, min.width, min.height));
        assert!(
            laid_out.contains("ratcn"),
            "at exactly the minimum the chrome lays out"
        );

        for (width, height) in [
            (min.width - 1, min.height),
            (min.width, min.height - 1),
            (10, 3),
            (1, 1),
        ] {
            let painted = text_of(&draw_at(&mut app, width, height));
            assert!(
                !painted.contains("ratcn"),
                "at {width}×{height} the chrome laid out in an area too small for it"
            );
        }
    }

    /// A demo is handed events in its own coordinates, and the shift is
    /// constant so a drag that leaves the region keeps meaning what it says.
    #[test]
    fn a_mouse_event_reaches_the_demo_in_the_demos_own_cells() {
        // A region at screen (10, 5) showing the demo from its own row 7.
        let embed = Embed {
            rect: Rect::new(10, 5, 20, 8),
            source_row: 7,
        };

        let inside = embed.translate(match mouse(MouseKind::Moved, 12, 6) {
            Event::Mouse(mouse) => mouse,
            _ => unreachable!(),
        });
        assert_eq!((inside.column, inside.row), (2, 8));

        let dragged = embed.translate(match mouse(MouseKind::Drag(MouseButton::Left), 4, 20) {
            Event::Mouse(mouse) => mouse,
            _ => unreachable!(),
        });
        assert_eq!(
            (dragged.column, dragged.row),
            (0, 22),
            "a drag outside the region keeps the constant shift, so a gesture \
             that strays out and back lands where the user means"
        );

        let hovered = embed.translate(match mouse(MouseKind::Moved, 4, 1) {
            Event::Mouse(mouse) => mouse,
            _ => unreachable!(),
        });
        assert_eq!(
            (hovered.column, hovered.row),
            (u16::MAX, u16::MAX),
            "but motion outside it names no cell at all: the same shift would \
             put the pointer on a control it is nowhere near"
        );
    }

    #[test]
    fn shift_is_a_constant_offset_that_saturates_rather_than_wrapping() {
        assert_eq!(shift(12, 10, 0), 2, "toward the origin");
        assert_eq!(shift(12, 0, 10), 22, "and away from it");
        assert_eq!(shift(4, 10, 0), 0, "clamped at zero rather than wrapping");
        assert_eq!(shift(u16::MAX, 0, 10), u16::MAX, "and at the top");
    }

    /// The offscreen canvas, pinned by its consequence rather than its shape.
    ///
    /// A demo cannot keep to the `area` it is handed: its components place
    /// their floating layers against `frame.area()`, so a select panel or a
    /// tooltip bubble near an edge reaches outside it. Giving the demo a frame
    /// of exactly its own size is what makes that harmless. Painted straight
    /// into the app's frame, the demo below would take the header with it.
    #[test]
    fn a_demo_that_paints_its_whole_frame_cannot_reach_the_chrome() {
        let Probed {
            mut app, greedy, ..
        } = probed();
        greedy.set(true);
        let painted = draw_at(&mut app, 100, 40);
        let rect = app.embed.expect("the demo is on screen").rect;

        assert_eq!(
            painted[(rect.x, rect.y)].symbol(),
            GREED,
            "the demo did paint, so the rest of this test means something"
        );
        assert!(
            text_of(&painted).contains("ratcn"),
            "the header survived a demo painting every cell of its frame"
        );
        for column in 0..painted.area.width {
            assert_ne!(
                painted[(column, 0)].symbol(),
                GREED,
                "the demo reached the header row at column {column}"
            );
        }
    }

    /// The reveal has to run from `update`, not just exist: the scroll area's
    /// own reveal takes a hold it never reports, so a focus that lands on a
    /// clipped button without the app moving its offset would draw the page
    /// and the blitted demo at two different rows.
    #[test]
    fn tabbing_to_a_clipped_hero_button_scrolls_the_page_the_app_holds() {
        let Probed { mut app, .. } = probed();
        // Short enough that the hero buttons are below the viewport.
        draw_at(&mut app, 100, 20);
        assert_eq!(app.state.page_scroll, 0, "the page opens at the top");

        route(&mut app, Event::Key(KeyEvent::new(KeyCode::Tab)));
        route(&mut app, Event::Key(KeyEvent::new(KeyCode::Tab)));

        assert!(
            app.state.focus.contains_path(["page"]),
            "two tabs reach the page"
        );
        assert!(
            app.state.page_scroll > 0,
            "and the app scrolled its own offset to show the button focus landed on"
        );
    }

    /// The routing rules while the demo has the input. Each of these is a
    /// decision the app makes and nothing else enforces.
    #[test]
    fn a_press_outside_the_region_leaves_and_a_drag_outside_does_not() {
        let Probed { mut app, seen, .. } = probed();
        let outside = app.embed.expect("the demo is on screen").rect.y - 1;

        app.enter();
        route(
            &mut app,
            mouse(MouseKind::Drag(MouseButton::Left), 1, outside),
        );
        assert!(
            app.state.entered(),
            "a drag outside the region is still the demo's gesture"
        );
        assert!(seen.take().is_some(), "and it reached the demo");

        route(
            &mut app,
            mouse(MouseKind::Down(MouseButton::Left), 1, outside),
        );
        assert!(!app.state.entered(), "a press outside gives the input back");
        assert!(
            seen.take().is_none(),
            "and it is the chrome's, not the demo's"
        );
    }

    #[test]
    fn esc_reaches_the_demo_first_and_leaves_only_when_the_demo_ignored_it() {
        let Probed {
            mut app,
            seen,
            handled,
            ..
        } = probed();
        let escape = Event::Key(KeyEvent::new(KeyCode::Esc));

        app.enter();
        handled.set(true);
        route(&mut app, escape.clone());
        assert_eq!(seen.take(), Some(escape.clone()), "the demo saw it");
        assert!(
            app.state.entered(),
            "and having handled it, keeps the input"
        );

        handled.set(false);
        route(&mut app, escape.clone());
        assert_eq!(seen.take(), Some(escape), "the demo saw it again");
        assert!(!app.state.entered(), "and ignoring it gave the input back");
    }

    /// A demo with no rows on screen has no cell for the pointer to land on,
    /// so pointer events stop at the host rather than being given an invented
    /// one — but keys still reach it, which is what lets Esc give the input
    /// back from there.
    #[test]
    fn a_demo_with_nothing_on_screen_is_not_reachable_by_the_pointer() {
        let Probed { mut app, seen, .. } = probed();
        app.enter();
        // A window too small for the chrome is one of the two states that
        // leaves the demo with nothing on screen while it still holds the
        // input; the page scrolling its preview out of view is the other.
        draw_at(&mut app, 10, 3);
        assert!(app.embed.is_none(), "nothing of the demo is showing");
        assert!(app.state.entered(), "and it still has the input");

        assert!(
            !route(&mut app, mouse(MouseKind::Moved, 5, 1)),
            "the pointer finds nothing to reach"
        );
        assert!(seen.take().is_none(), "so the demo is never handed a cell");

        let escape = Event::Key(KeyEvent::new(KeyCode::Esc));
        route(&mut app, escape.clone());
        assert_eq!(seen.take(), Some(escape), "but a key still reaches it");
        assert!(!app.state.entered(), "and Esc still gives the input back");
    }

    #[test]
    fn only_the_wheel_falls_through_to_the_chrome() {
        let Probed { mut app, .. } = probed();
        let inside = app.embed.expect("the demo is on screen").rect;
        let (column, row) = (inside.x + 1, inside.y + 1);
        let parked = FocusState::intent([PARKED_FOCUS]);

        app.enter();
        route(
            &mut app,
            mouse(MouseKind::Scroll(ScrollDirection::Down), column, row),
        );
        assert!(
            app.state.page_scroll > 0,
            "a wheel the demo ignored still scrolls the page under it"
        );
        assert_eq!(app.state.focus, parked, "without disturbing the parking");

        let scrolled = app.state.page_scroll;
        route(
            &mut app,
            mouse(MouseKind::Down(MouseButton::Left), column, row),
        );
        assert_eq!(
            app.state.focus, parked,
            "a press the demo ignored must not reach the chrome: it would take \
             focus off the parked path, which is what parking is for"
        );
        assert_eq!(app.state.page_scroll, scrolled, "and moves nothing");
    }
}
