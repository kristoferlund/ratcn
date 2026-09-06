//! The ratcn website as a terminal application.
//!
//! Three views under one header: the landing page, the Getting started page,
//! and a browser for every demo in the repository. The two that embed a demo do
//! it the way the website embeds a preview — it takes the input while the user
//! is inside it — and both do it the same way, through [`App::blit_demo`].

mod catalog;
mod chrome;
mod code;
mod demos;
mod getting_started;
mod page;
mod page_geometry;
#[cfg(test)]
mod snapshot;

use std::{io, time::Duration};

use ratatui::{
    Frame, Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::{Position, Rect, Size},
    style::Style,
};
use ratcn::{
    Theme,
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
    GettingStarted,
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
    /// Moving it commits nothing.
    cursor: usize,
    /// The demo the Demos view shows, as an index into [`catalog::ENTRIES`].
    /// Only a click or Enter moves it.
    showing: usize,
    /// The nav list's top item.
    nav_scroll: usize,
    /// The landing page's first visible content row.
    landing_scroll: u16,
    /// The Getting started page's first visible content row. Each scrolling
    /// view keeps its own, so leaving one and coming back finds it where it
    /// was left.
    getting_started_scroll: u16,
    /// The chrome's own focus, stashed while the demo has the input.
    parked: Option<FocusState>,
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
    NavSelected(usize),
    NavScrolled(usize),
    LandingScrolled(u16),
    GettingStartedScrolled(u16),
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
    /// and its focus reveal are answered from. [`None`] whenever it is not the
    /// view on screen.
    landing_page: Option<page::Layout>,
    /// The Getting started page as it was last measured, for its page keys.
    /// [`None`] whenever it is not the view on screen.
    getting_started: Option<getting_started::Layout>,
    /// Rows the nav list had on screen, which is what a commit reveals into.
    /// [`None`] whenever the Demos view is not on screen.
    nav_rows: Option<u16>,
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
            landing_page: None,
            getting_started: None,
            nav_rows: None,
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => {
                if let Some(offset) = self.landing_page.and_then(|page| page.reveal(&focus)) {
                    self.state.landing_scroll = offset;
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
                self.state.cursor = index;
                self.state.nav_scroll = offset;
            }
            // A click commits without a preceding move, so the cursor follows
            // the row that was committed: whatever the user does next with the
            // arrows continues from where they clicked. And the row is scrolled
            // into view, because the wheel can have left it off screen — see
            // [`demos::revealed`].
            Msg::NavSelected(index) => {
                self.state.showing = index;
                self.state.cursor = index;
                if let Some(rows) = self.nav_rows {
                    self.state.nav_scroll =
                        demos::revealed(index, self.state.nav_scroll, usize::from(rows));
                }
            }
            Msg::NavScrolled(offset) => self.state.nav_scroll = offset,
            Msg::LandingScrolled(offset) => self.state.landing_scroll = offset,
            Msg::GettingStartedScrolled(offset) => self.state.getting_started_scroll = offset,
        }
    }

    /// The demo the current view shows, built if this is its first appearance.
    fn shown_mut(&mut self) -> Option<&mut dyn Embedded> {
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
            View::GettingStarted => None,
            View::Demos => self.opened[self.state.showing].as_deref(),
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
        self.shown_mut()
            .is_some_and(|demo| demo.handle_event(event))
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
    /// has a key for it, and only Right: the nav list's bound selection makes
    /// Enter the list's own. The landing page is entered by clicking into it.
    fn enter_key(&mut self, event: &Event) -> bool {
        let Event::Key(key) = event else {
            return false;
        };
        // Modifiers are gated the way [`App::page_key`] gates them: a chord is
        // the app's or the terminal's, and Ctrl+Right must not hand the demo
        // the keyboard behind the user's back.
        if self.state.view != View::Demos || key.modifiers.any() || key.code != KeyCode::Right {
            return false;
        }
        self.enter();
        true
    }

    /// Scroll whichever page is showing on a page key the chrome passed on.
    /// See [`page_geometry::Scroll::scrolled`] — both scrolling views answer from the
    /// same arithmetic, over their own offset.
    fn page_key(&mut self, event: &Event) -> bool {
        let Event::Key(key) = event else {
            return false;
        };
        if key.modifiers.any() {
            return false;
        }
        let scrolled = match self.state.view {
            View::Landing => self
                .landing_page
                .and_then(|landing| landing.scroll.scrolled(key.code))
                .map(Msg::LandingScrolled),
            View::GettingStarted => self
                .getting_started
                .and_then(|guide| guide.scroll.scrolled(key.code))
                .map(Msg::GettingStartedScrolled),
            View::Demos => None,
        };
        let Some(msg) = scrolled else {
            return false;
        };
        self.update(msg);
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
        let Some(demo) = shown_in(state, landing, opened) else {
            return;
        };
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
        self.nav_rows = Some(demos::visible_rows(columns.nav));

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
        let page = page::layout(bands.body, self.state.landing_scroll);
        self.landing_page = Some(page);
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

    /// The Getting started view: prose and code, scrolling, and nothing else.
    fn draw_getting_started(&mut self, frame: &mut Frame, bands: &chrome::Bands, theme: &Theme) {
        let page = getting_started::layout(bands.body, self.state.getting_started_scroll);
        self.getting_started = Some(page);

        chrome::header_rule(frame.buffer_mut(), bands, theme);

        let state = &self.state;
        self.ratcn.render(frame, state, theme, |ctx| {
            chrome::declare(ctx, state, bands.header);
            getting_started::declare(ctx, bands.body, page);
        });
    }
}

/// The demo `state`'s view shows, out of the two places demos are kept, built
/// if this is its first appearance. [`None`] on a view that embeds nothing.
///
/// Free rather than a method so a caller can hold the canvas at the same time.
fn shown_in<'a>(
    state: &AppState,
    landing: &'a mut Box<dyn Embedded>,
    opened: &'a mut [Option<Box<dyn Embedded>>],
) -> Option<&'a mut dyn Embedded> {
    match state.view {
        View::Landing => Some(landing.as_mut()),
        // The Getting started page is prose and code; it embeds nothing.
        View::GettingStarted => None,
        View::Demos => Some(
            opened[state.showing]
                .get_or_insert_with(|| catalog::ENTRIES[state.showing].open())
                .as_mut(),
        ),
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

    /// Whatever the demo on screen asks of the clock.
    fn wake(&self) -> Option<Duration> {
        self.shown().and_then(Embedded::wake)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(theme.background));

        // Everything derived from the last frame's layout, cleared before the
        // frame that replaces it: each view then sets only what it owns, and a
        // view that owns none of it cannot inherit another's.
        self.embed = None;
        self.landing_page = None;
        self.getting_started = None;
        self.nav_rows = None;

        let Some(bands) = chrome::layout(area) else {
            chrome::too_small(frame, area, theme);
            return;
        };

        match self.state.view {
            View::Demos => self.draw_demos(frame, &bands, theme),
            View::Landing => self.draw_landing(frame, &bands, theme),
            View::GettingStarted => self.draw_getting_started(frame, &bands, theme),
        }
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(App::new())
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use ratcn::{
        Button,
        runtime::{KeyEvent, Modifiers, ScrollDirection},
    };

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
        // Pinned, because it is a promise to whoever runs this over SSH rather
        // than an incidental number: the nav column's widest demo name sets the
        // width, and the hint lines set the height. A fifth hint costs a row of
        // everyone's terminal, and should have to be argued for here.
        assert_eq!((min.width, min.height), (42, 8));
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
        assert_eq!(app.state.landing_scroll, 0, "the page opens at the top");

        for _ in 0..HEADER_LINKS {
            route(&mut app, Event::Key(KeyEvent::new(KeyCode::Tab)));
        }

        assert!(
            app.state.focus.contains_path(["page"]),
            "a tab past each header link reaches the page"
        );
        assert!(
            app.state.landing_scroll > 0,
            "and the app scrolled its own offset to show the button focus landed on"
        );
    }

    /// Header links, in declaration order — which is also Tab order, and the
    /// number of tabs it takes to leave the header. Focus starts on the first
    /// of them, so a tab each reaches the first thing below.
    const HEADER_LINKS: usize = 3;

    /// Where each header link sits on row 0: the labels laid out end to end,
    /// each two cells of padding either side of its text.
    fn header_link(label: &str) -> u16 {
        let width = |label: &str| Button::<Msg>::new(label).width();
        ["ratcn", "Getting started", "Demos"]
            .iter()
            .take_while(|candidate| **candidate != label)
            .map(|candidate| width(candidate))
            .sum::<u16>()
            + width(label) / 2
    }

    /// Every link reaches every view, and the two hero buttons are the same
    /// two navigations under the same two names — so no destination is
    /// reachable by only one route.
    #[test]
    fn all_three_views_are_reachable_from_every_link_and_from_the_hero_buttons() {
        let mut app = App::new();
        let click = |app: &mut App, label: &str| {
            draw_at(app, 100, 40);
            route(
                app,
                mouse(MouseKind::Click(MouseButton::Left), header_link(label), 0),
            );
        };

        for from in [View::Landing, View::GettingStarted, View::Demos] {
            for (label, to) in [
                ("ratcn", View::Landing),
                ("Getting started", View::GettingStarted),
                ("Demos", View::Demos),
            ] {
                app.state.view = from;
                click(&mut app, label);
                assert!(
                    app.state.view == to,
                    "{label:?} did not reach its view from the one before it"
                );
            }
        }

        // The hero buttons, reached by tabbing past the header rather than by
        // arithmetic: they are inside a scroll area, and where they sit
        // depends on where the page is scrolled to.
        for (tabs, expected) in [
            (HEADER_LINKS, View::GettingStarted),
            (HEADER_LINKS + 1, View::Demos),
        ] {
            app.state.view = View::Landing;
            app.state.landing_scroll = 0;
            app.state.focus = FocusState::default();
            draw_at(&mut app, 100, 40);
            for _ in 0..tabs {
                route(&mut app, Event::Key(KeyEvent::new(KeyCode::Tab)));
                draw_at(&mut app, 100, 40);
            }
            route(&mut app, Event::Key(KeyEvent::new(KeyCode::Enter)));
            assert!(
                app.state.view == expected,
                "the hero button {tabs} tabs in did not navigate"
            );
        }
    }

    /// Browsing and choosing are two different things in the nav list: the
    /// cursor moves freely, and only a click or Enter changes the demo the
    /// pane is showing.
    #[test]
    fn the_demo_list_shows_a_demo_on_a_click_or_enter_and_never_on_a_hover() {
        let mut app = App::new();
        // Every demo pre-built as a probe: `effects::App::new` starts a
        // network request, and this test is about the list, not the demos.
        for slot in &mut app.opened {
            *slot = Some(Box::new(Probe::default()));
        }
        app.update(Msg::Navigate(View::Demos));
        draw_at(&mut app, 100, 40);
        // The nav list starts at the first body row, one item to a row.
        let row = |index: u16| 2 + index;

        route(&mut app, mouse(MouseKind::Moved, 2, row(3)));
        assert_eq!(app.state.cursor, 3, "a hover moves the cursor");
        assert_eq!(
            app.state.showing, 0,
            "and moves nothing else: hovering a name must not swap the demo"
        );

        route(
            &mut app,
            mouse(MouseKind::Click(MouseButton::Left), 2, row(3)),
        );
        assert_eq!(app.state.showing, 3, "a click is the choice");

        route(&mut app, mouse(MouseKind::Moved, 2, row(5)));
        assert_eq!(app.state.cursor, 5, "the cursor browses on");
        assert_eq!(app.state.showing, 3, "with the demo where it was left");

        route(&mut app, Event::Key(KeyEvent::new(KeyCode::Enter)));
        assert_eq!(
            app.state.showing, 5,
            "and Enter commits the row the cursor reached"
        );
        assert!(
            !app.state.entered(),
            "the list consumed Enter, so it never reached the app's own fallback"
        );

        route(&mut app, Event::Key(KeyEvent::new(KeyCode::Right)));
        assert!(app.state.entered(), "Right is what enters the demo pane");
    }

    /// The wheel is the one gesture that separates the cursor from the
    /// viewport, so it is the one that can commit a row that is nowhere on
    /// screen. A key labelled "show" has to show something.
    #[test]
    fn a_row_committed_after_the_wheel_is_scrolled_into_view() {
        let mut app = App::new();
        for slot in &mut app.opened {
            *slot = Some(Box::new(Probe::default()));
        }
        app.update(Msg::Navigate(View::Demos));
        // Short enough that the nav list holds far fewer rows than the catalog.
        draw_at(&mut app, 100, 16);
        let rows = usize::from(app.nav_rows.expect("the nav list is on screen"));
        assert!(
            rows < catalog::ENTRIES.len(),
            "the list has to overflow its window for this to mean anything"
        );

        route(&mut app, mouse(MouseKind::Click(MouseButton::Left), 2, 2));
        assert_eq!(app.state.showing, 0, "a row at the top is committed");

        // Wheel the view down past it, which leaves the cursor behind.
        for _ in 0..6 {
            route(
                &mut app,
                mouse(MouseKind::Scroll(ScrollDirection::Down), 2, 4),
            );
        }
        draw_at(&mut app, 100, 16);
        assert!(
            app.state.nav_scroll > 0,
            "the wheel scrolled the list out from under the cursor"
        );

        route(&mut app, Event::Key(KeyEvent::new(KeyCode::Enter)));
        let showing = app.state.showing;
        assert!(
            (app.state.nav_scroll..app.state.nav_scroll + rows).contains(&showing),
            "row {showing} was committed while the list shows \
             {}..{}",
            app.state.nav_scroll,
            app.state.nav_scroll + rows
        );
    }

    /// The two view-level key fallbacks have to agree about modifiers, or one
    /// of them answers chords the other leaves to the app.
    #[test]
    fn a_chord_is_not_one_of_the_view_level_keys() {
        let mut app = App::new();
        for slot in &mut app.opened {
            *slot = Some(Box::new(Probe::default()));
        }
        app.update(Msg::Navigate(View::Demos));
        draw_at(&mut app, 100, 40);

        let held = |ctrl, alt, shift| Modifiers { ctrl, alt, shift };
        for modifiers in [
            held(true, false, false),
            held(false, true, false),
            held(false, false, true),
        ] {
            let mut key = KeyEvent::new(KeyCode::Right);
            key.modifiers = modifiers;
            route(&mut app, Event::Key(key));
            assert!(
                !app.state.entered(),
                "{modifiers:?}+Right handed the demo the keyboard"
            );
        }

        route(&mut app, Event::Key(KeyEvent::new(KeyCode::Right)));
        assert!(app.state.entered(), "while plain Right still does");
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
            app.state.landing_scroll > 0,
            "a wheel the demo ignored still scrolls the page under it"
        );
        assert_eq!(app.state.focus, parked, "without disturbing the parking");

        let scrolled = app.state.landing_scroll;
        route(
            &mut app,
            mouse(MouseKind::Down(MouseButton::Left), column, row),
        );
        assert_eq!(
            app.state.focus, parked,
            "a press the demo ignored must not reach the chrome: it would take \
             focus off the parked path, which is what parking is for"
        );
        assert_eq!(app.state.landing_scroll, scrolled, "and moves nothing");
    }
}
