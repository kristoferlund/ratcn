//! The ratcn website as a terminal application.
//!
//! Three views under one header: the landing page, the Getting started page,
//! and a browser for every demo in the repository. The two that embed a demo do
//! it the way the website embeds a preview — it takes the input while the user
//! is inside it. Catalog demos draw directly into their pane; only the scrolling
//! landing preview uses an offscreen buffer and a visible-row copy.

mod catalog;
mod chrome;
mod demos;
mod getting_started;
mod page;
mod page_geometry;

use std::{io, time::Duration};

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::Style,
};
use ratcn::{
    Theme,
    linear_nav::cursor_visible_offset,
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
    /// A saved chrome focus means the demo owns input.
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
    /// The landing preview's row showing at `rect`'s top; its source column is
    /// always zero. Unused for catalog demos, which use screen coordinates.
    source_row: u16,
}

impl Embed {
    /// Preserve drag coordinates outside the preview, but never invent a hover.
    fn translate(self, mouse: MouseEvent, primary_down: bool) -> MouseEvent {
        let hover_only = matches!(mouse.kind, MouseKind::Scroll(_))
            || (mouse.kind == MouseKind::Moved && !primary_down);
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
    /// Lazy, persistent instances: constructing `effects` starts a network request.
    opened: Vec<Option<Box<dyn Embedded>>>,
    /// Only the scrolling landing preview needs offscreen rows.
    canvas: Buffer,
    /// Actual painted demo bounds, not a projection from a pending scroll.
    embed: Option<Embed>,
    /// Last valid body for this view. Event queries combine it with current state.
    body: Option<Rect>,
    /// Raw motion may become a drag only after reaching the child runtime.
    primary_down: bool,
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
            canvas: Buffer::empty(Rect::default()),
            embed: None,
            body: None,
            primary_down: false,
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focus) => {
                if self.state.view == View::Landing
                    && let Some(offset) = self.body.and_then(|body| {
                        page::layout(body, self.state.landing_scroll).reveal(&focus)
                    })
                {
                    self.state.landing_scroll = offset;
                }
                self.state.focus = focus;
            }
            Msg::Navigate(view) => {
                if self.state.view != view {
                    self.leave();
                    self.body = None;
                    self.embed = None;
                }
                self.state.view = view;
                // Arrows browse immediately on arrival in Demos.
                if view == View::Demos {
                    self.state.focus = FocusState::intent([demos::NAV_ID]);
                }
            }
            Msg::NavFocused(index, offset) => {
                self.state.cursor = index;
                self.state.nav_scroll = offset;
            }
            // A click commits without moving first; reveal even after wheel scrolling.
            Msg::NavSelected(index) => {
                if self.state.showing != index {
                    self.leave();
                    self.embed = None;
                }
                self.state.showing = index;
                self.state.cursor = index;
                if let Some(body) = self.body {
                    self.state.nav_scroll = cursor_visible_offset(
                        catalog::ENTRIES.len(),
                        usize::from(demos::visible_rows(demos::columns(body).nav)),
                        self.state.nav_scroll,
                        Some(index),
                    );
                }
            }
            Msg::NavScrolled(offset) => self.state.nav_scroll = offset,
            Msg::LandingScrolled(offset) => self.state.landing_scroll = offset,
            Msg::GettingStartedScrolled(offset) => self.state.getting_started_scroll = offset,
        }
    }

    /// The current instance, if already built. Only painting constructs demos.
    fn shown_mut(&mut self) -> Option<&mut dyn Embedded> {
        match self.state.view {
            View::Landing => Some(self.landing.as_mut()),
            View::GettingStarted => None,
            View::Demos => match self.opened[self.state.showing].as_mut() {
                Some(demo) => Some(demo.as_mut()),
                None => None,
            },
        }
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

    /// Only a painted demo receives input. The host can still leave on Esc.
    fn route_to_demo(&mut self, event: Event) -> bool {
        let Some(embed) = self.embed else {
            return false;
        };
        let event = match event {
            Event::Mouse(mouse) if self.state.view == View::Landing => {
                match mouse.kind {
                    MouseKind::Down(MouseButton::Left) => self.primary_down = true,
                    MouseKind::Up(MouseButton::Left) => self.primary_down = false,
                    _ => {}
                }
                Event::Mouse(embed.translate(mouse, self.primary_down))
            }
            event => event,
        };
        self.shown_mut()
            .is_some_and(|demo| demo.handle_event(event))
    }

    /// Hand the input to the embedded demo, parking the chrome's focus.
    fn enter(&mut self) {
        if self.embed.is_none() || self.state.entered() {
            return;
        }
        let chrome_focus = std::mem::replace(&mut self.state.focus, FocusState::none());
        self.state.parked = Some(chrome_focus);
    }

    /// Take it back, putting the chrome's focus where it was.
    fn leave(&mut self) {
        self.exit_demo();
        if let Some(focus) = self.state.parked.take() {
            self.update(Msg::FocusChanged(focus));
        }
    }

    /// End pointer interaction before hiding or relinquishing the child.
    /// Callers already owe a frame, so any cancelled app-owned drag is repainted.
    fn exit_demo(&mut self) {
        self.primary_down = false;
        if self.embed.is_some()
            && let Some(demo) = self.shown_mut()
        {
            demo.handle_event(Event::Mouse(MouseEvent {
                kind: MouseKind::Exited,
                column: 0,
                row: 0,
                modifiers: Default::default(),
            }));
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
        if self.state.view != View::Demos
            || self.embed.is_none()
            || key.modifiers.any()
            || key.code != KeyCode::Right
        {
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
        let Some(body) = self.body else {
            return false;
        };
        let scrolled = match self.state.view {
            View::Landing => page::layout(body, self.state.landing_scroll)
                .scroll
                .scrolled(key.code)
                .map(Msg::LandingScrolled),
            View::GettingStarted => {
                getting_started::layout(body, self.state.getting_started_scroll)
                    .scroll
                    .scrolled(key.code)
                    .map(Msg::GettingStartedScrolled)
            }
            View::Demos => None,
        };
        let Some(msg) = scrolled else {
            return false;
        };
        self.update(msg);
        true
    }

    /// The Demos view: the nav column, the rules, and the selected demo.
    fn draw_demos(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        bands: &chrome::Bands,
        theme: &Theme,
    ) {
        let columns = demos::columns(bands.body);
        let embed = Embed {
            rect: columns.pane,
            source_row: 0,
        };
        self.embed = Some(embed);

        chrome::header_rule(buffer, bands, theme);
        demos::separators(buffer, bands, &columns, theme, self.state.entered());

        let state = &self.state;
        self.ratcn.render_into(buffer, area, state, theme, |ctx| {
            chrome::declare(ctx, state, bands.header);
            demos::declare(ctx, columns.nav);
        });

        let demo = self.opened[self.state.showing]
            .get_or_insert_with(|| catalog::ENTRIES[self.state.showing].open());
        let demo_theme = demo.theme(theme);
        demo.draw(buffer, columns.pane, &demo_theme);
    }

    /// The landing view: the site's front page, scrolling, with the demo
    /// blitted into its preview window.
    fn draw_landing(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        bands: &chrome::Bands,
        theme: &Theme,
    ) {
        let page = page::layout(bands.body, self.state.landing_scroll);
        if page.embed.is_none() && self.embed.is_some() {
            self.exit_demo();
        }
        self.embed = page
            .embed
            .map(|(rect, source_row)| Embed { rect, source_row });

        chrome::header_rule(buffer, bands, theme);

        let state = &self.state;
        self.ratcn.render_into(buffer, area, state, theme, |ctx| {
            chrome::declare(ctx, state, bands.header);
            page::declare(ctx, bands.body, page, state.entered());
        });

        if let Some(embed) = self.embed {
            let canvas_area = Rect::new(0, 0, page.canvas.width, page.canvas.height);
            self.canvas.resize(canvas_area);
            self.canvas.reset();
            let demo_theme = self.landing.theme(theme);
            self.landing
                .draw(&mut self.canvas, canvas_area, &demo_theme);
            blit(buffer, &self.canvas, embed.rect, embed.source_row);
        }
    }

    /// The Getting started view: prose and code, scrolling, and nothing else.
    fn draw_getting_started(
        &mut self,
        buffer: &mut Buffer,
        area: Rect,
        bands: &chrome::Bands,
        theme: &Theme,
    ) {
        let page = getting_started::layout(bands.body, self.state.getting_started_scroll);
        self.embed = None;

        chrome::header_rule(buffer, bands, theme);

        let state = &self.state;
        self.ratcn.render_into(buffer, area, state, theme, |ctx| {
            chrome::declare(ctx, state, bands.header);
            getting_started::declare(ctx, bands.body, page);
        });
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
        if matches!(&event, Event::Mouse(mouse) if mouse.kind == MouseKind::Exited) {
            self.primary_down = false;
        }
        if self.body.is_none() {
            if self.state.entered() && matches!(&event, Event::Key(key) if key.code == KeyCode::Esc)
            {
                self.leave();
                return true;
            }
            return false;
        }
        let mut redraw = false;
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
                // Outside presses leave; an ongoing drag still belongs to the demo.
                Event::Mouse(mouse)
                    if mouse.kind == MouseKind::Down(MouseButton::Left)
                        && !self.over_demo(Position::new(mouse.column, mouse.row)) =>
                {
                    self.leave();
                    redraw = true;
                }
                _ => {
                    if self.route_to_demo(event.clone()) {
                        return true;
                    }
                    // Only the wheel falls through, and only so the page keeps
                    // scrolling under a demo that does not scroll. Any other
                    // event reaching the chrome could focus it again while the
                    // demo still owns the input.
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
            EventResult::Ignored => self.enter_key(&event) || self.page_key(&event) || redraw,
        }
    }

    /// Whatever the demo on screen asks of the clock.
    fn wake(&self) -> Option<Duration> {
        self.embed?;
        self.shown().and_then(Embedded::wake)
    }

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        buffer.set_style(area, Style::default().bg(theme.background));

        let Some(bands) = chrome::layout(area) else {
            if self.embed.is_some() {
                self.exit_demo();
            }
            self.embed = None;
            self.body = None;
            self.primary_down = false;
            chrome::too_small(buffer, area, theme);
            return;
        };
        let resized = self.body != Some(bands.body);
        if resized && self.embed.is_some() {
            self.exit_demo();
            self.embed = None;
        }
        self.body = Some(bands.body);
        if resized {
            self.update(Msg::FocusChanged(self.state.focus.clone()));
        }

        match self.state.view {
            View::Demos => self.draw_demos(buffer, area, &bands, theme),
            View::Landing => self.draw_landing(buffer, area, &bands, theme),
            View::GettingStarted => self.draw_getting_started(buffer, area, &bands, theme),
        }
        if self.embed.is_none() {
            self.primary_down = false;
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
    }

    impl demo_shared::Demo for Probe {
        fn draw(&mut self, _buffer: &mut Buffer, _area: Rect, _theme: &Theme) {}

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
    }

    /// An app showing the landing view with a probe in place of the demo, and
    /// one frame already drawn so the chrome has a surface to route against.
    fn probed() -> Probed {
        let seen = Rc::<Cell<Option<Event>>>::default();
        let handled = Rc::<Cell<bool>>::default();
        let mut app = App::new();
        app.landing = Box::new(Probe {
            seen: Rc::clone(&seen),
            handled: Rc::clone(&handled),
        });
        draw_at(&mut app, 100, 40);
        Probed { app, seen, handled }
    }

    /// Draw one frame at `width` × `height`, and hand back what was painted.
    fn draw_at(app: &mut App, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        demo_shared::Demo::draw(app, &mut buffer, area, &Theme::default_dark());
        buffer
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

    #[test]
    fn getting_started_markdown_wraps_and_scrolls_without_changing_the_header() {
        for width in [43, 60, 100, 140] {
            let mut app = App::new();
            app.update(Msg::Navigate(View::GettingStarted));
            let first = draw_at(&mut app, width, 20);
            let body = app.body.unwrap();
            let intro: String = body
                .positions()
                .map(|point| first[point].symbol())
                .collect();
            assert!(intro.contains("component library"));
            assert!(intro.contains("Preview release."));
            assert!(
                !intro.contains("**Preview"),
                "Markdown emphasis must be rendered, not shown as source markup"
            );
            assert!(route(&mut app, Event::Key(KeyEvent::new(KeyCode::End))));
            let last = draw_at(&mut app, width, 20);
            assert!(
                text_of(&last).contains("ratzilla."),
                "the final paragraph must remain reachable at width {width}"
            );
            for y in 0..body.y {
                for x in 0..width {
                    assert_eq!(
                        last[(x, y)],
                        first[(x, y)],
                        "scrolling overwrote the header"
                    );
                }
            }
            assert!(route(&mut app, Event::Key(KeyEvent::new(KeyCode::Home))));
            assert_eq!(draw_at(&mut app, width, 20), first);
        }
        assert!(
            getting_started::layout(Rect::new(0, 0, 43, 20), 0)
                .scroll
                .content
                > getting_started::layout(Rect::new(0, 0, 100, 20), 0)
                    .scroll
                    .content,
            "narrow columns must wrap the document instead of clipping it"
        );
    }

    #[test]
    fn consecutive_header_page_keys_use_the_current_scroll_offset() {
        for view in [View::Landing, View::GettingStarted] {
            let mut app = App::new();
            app.update(Msg::Navigate(view));
            let before = draw_at(&mut app, 60, 12);
            let offset = |app: &App| match view {
                View::Landing => app.state.landing_scroll,
                _ => app.state.getting_started_scroll,
            };
            assert!(route(
                &mut app,
                Event::Key(KeyEvent::new(KeyCode::PageDown))
            ));
            let first = offset(&app);
            assert!(route(
                &mut app,
                Event::Key(KeyEvent::new(KeyCode::PageDown))
            ));
            assert_eq!(
                offset(&app),
                first * 2,
                "both page keys must advance before any redraw"
            );
            assert_ne!(draw_at(&mut app, 60, 12), before);
        }
    }

    #[test]
    fn standard_list_marks_selection_separately_from_cursor_and_fits_every_label() {
        let mut app = App::new();
        app.opened[0] = Some(Box::new(Probe::default()));
        app.update(Msg::Navigate(View::Demos));
        let width = chrome::min_size().width;
        draw_at(&mut app, width, 40);
        route(&mut app, Event::Key(KeyEvent::new(KeyCode::Down)));
        let painted = draw_at(&mut app, width, 40);
        let nav = demos::columns(app.body.unwrap()).nav;
        let markers = ratcn::selection_indicator::MarkerGlyphs::radio();
        assert_eq!(app.state.cursor, 1);
        assert_eq!(app.state.showing, 0);
        for (index, entry) in catalog::ENTRIES.iter().enumerate() {
            let y = nav.y + index as u16;
            assert_eq!(painted[(nav.x + 1, y)].symbol(), markers.marker(index == 0));
            let row: String = (nav.x..nav.right())
                .map(|x| painted[(x, y)].symbol())
                .collect();
            assert!(
                row.ends_with(entry.name) || row.contains(&format!("{} ", entry.name)),
                "{} was truncated: {row:?}",
                entry.name
            );
        }
        assert_ne!(
            painted[(nav.x, nav.y + 1)].bg,
            painted[(nav.x, nav.y + 2)].bg,
            "cursor styling must distinguish browsing from other rows"
        );
        app.enter();
        let entered = draw_at(&mut app, width, 40);
        assert_eq!(
            entered[(nav.x + 1, nav.y)].symbol(),
            markers.selected,
            "selection remains visible when the demo owns input"
        );
    }

    #[test]
    fn a_blank_outside_press_requests_the_frame_that_restores_chrome_focus() {
        let Probed { mut app, .. } = probed();
        app.enter();
        let entered = draw_at(&mut app, 100, 40);
        assert!(route(
            &mut app,
            mouse(MouseKind::Down(MouseButton::Left), 99, 0)
        ));
        assert!(!app.state.entered());
        assert_ne!(draw_at(&mut app, 100, 40), entered);
    }

    #[test]
    fn restoring_a_hero_focus_synchronizes_the_page_and_preview() {
        let Probed { mut app, .. } = probed();
        for _ in 0..HEADER_LINKS {
            route(&mut app, Event::Key(KeyEvent::new(KeyCode::Tab)));
            draw_at(&mut app, 100, 40);
        }
        let focus = app.state.focus.clone();
        assert!(focus.contains_path(["page", "getting-started"]));
        app.enter();
        app.update(Msg::LandingScrolled(40));
        draw_at(&mut app, 100, 40);
        let body = chrome::layout(Rect::new(0, 0, 100, 40)).unwrap().body;
        let expected = page::layout(body, 40).reveal(&focus).unwrap();
        route(&mut app, Event::Key(KeyEvent::new(KeyCode::Esc)));
        assert_eq!(app.state.landing_scroll, expected);
        let painted = draw_at(&mut app, 100, 40);
        let embed = app.embed.unwrap();
        assert_eq!(embed.source_row, 0);
        let title: String = (0..100)
            .map(|x| painted[(x, embed.rect.y - 1)].symbol())
            .collect();
        assert!(
            title.contains("cargo run -p landing"),
            "the preview must align with its actual page border"
        );
    }

    #[test]
    fn missing_or_changed_geometry_cannot_enter_or_route_stale_controls() {
        let mut app = App::new();
        app.update(Msg::Navigate(View::Demos));
        assert!(!route(&mut app, Event::Key(KeyEvent::new(KeyCode::Right))));
        draw_at(&mut app, 100, 40);
        draw_at(&mut app, 10, 3);
        assert!(!route(&mut app, Event::Key(KeyEvent::new(KeyCode::Down))));
        assert_eq!(app.state.cursor, 0);
        assert!(!route(&mut app, Event::Key(KeyEvent::new(KeyCode::Right))));

        draw_at(&mut app, 100, 40);
        app.update(Msg::Navigate(View::Landing));
        assert!(!route(&mut app, Event::Key(KeyEvent::new(KeyCode::Enter))));
        route(
            &mut app,
            mouse(MouseKind::Click(MouseButton::Left), header_link("Demos"), 0),
        );
        assert!(
            app.state.view == View::Landing,
            "the previous header must not route before the new view paints"
        );
        assert!(app.opened[1].is_none());
        app.update(Msg::Navigate(View::Demos));
        draw_at(&mut app, 100, 40);
        let old_pane = app.embed.unwrap().rect;
        app.update(Msg::NavSelected(1));
        route(
            &mut app,
            mouse(
                MouseKind::Down(MouseButton::Left),
                old_pane.x + 1,
                old_pane.y + 1,
            ),
        );
        assert!(!route(&mut app, Event::Key(KeyEvent::new(KeyCode::Right))));
        assert!(!app.state.entered());
        assert!(
            app.opened[1].is_none(),
            "input must not construct a demo before its first paint"
        );
        draw_at(&mut app, 100, 40);
        assert!(route(&mut app, Event::Key(KeyEvent::new(KeyCode::Right))));
    }

    #[test]
    fn only_a_painted_preview_can_request_clock_wakeups() {
        struct Expired;
        impl demo_shared::Demo for Expired {
            fn draw(&mut self, _buffer: &mut Buffer, _area: Rect, _theme: &Theme) {}
            fn wake(&self) -> Option<Duration> {
                Some(Duration::ZERO)
            }
        }
        let mut app = App::new();
        app.landing = Box::new(Expired);
        assert_eq!(demo_shared::Demo::wake(&app), None);
        draw_at(&mut app, 100, 20);
        assert!(app.embed.is_none());
        assert_eq!(demo_shared::Demo::wake(&app), None);
        app.update(Msg::LandingScrolled(40));
        draw_at(&mut app, 100, 20);
        assert!(app.embed.is_some());
        assert_eq!(demo_shared::Demo::wake(&app), Some(Duration::ZERO));
        draw_at(&mut app, 10, 3);
        assert_eq!(demo_shared::Demo::wake(&app), None);
    }

    #[test]
    fn raw_pointer_motion_outside_preserves_a_real_demo_drag() {
        for view in [View::Landing, View::Demos] {
            let mut app = App::new();
            app.update(Msg::Navigate(view));
            app.landing = Box::new(drag::App::new());
            app.state.showing = catalog::ENTRIES
                .iter()
                .position(|entry| entry.name == "drag")
                .unwrap();
            draw_at(&mut app, 90, 40);
            if view == View::Landing {
                app.update(Msg::LandingScrolled(app.canvas.area.height / 2 + 24));
            }
            let before = draw_at(&mut app, 90, 30);
            let embed = app.embed.unwrap();
            let label = embed
                .rect
                .positions()
                .find(|&point| before[point].symbol() == "D")
                .unwrap();
            route(
                &mut app,
                mouse(MouseKind::Down(MouseButton::Left), label.x, label.y),
            );
            route(
                &mut app,
                mouse(MouseKind::Moved, embed.rect.right() + 3, label.y),
            );
            route(
                &mut app,
                mouse(
                    MouseKind::Up(MouseButton::Left),
                    embed.rect.right() + 3,
                    label.y,
                ),
            );
            let after = draw_at(&mut app, 90, 30);
            let moved = embed
                .rect
                .positions()
                .find(|&point| after[point].symbol() == "D")
                .expect("the dragged label must remain visible");
            assert_eq!(
                moved.y, label.y,
                "horizontal raw motion must not jump to the canvas bottom"
            );
            assert!(moved.x > label.x);
            route(&mut app, mouse(MouseKind::Moved, label.x, label.y));
            let released = draw_at(&mut app, 90, 30);
            let resting = embed
                .rect
                .positions()
                .find(|&point| released[point].symbol() == "D")
                .unwrap();
            assert_eq!(resting, moved, "motion after Up must not continue the drag");
        }
    }

    #[test]
    fn leaving_ends_child_capture_before_reentry() {
        let mut app = App::new();
        app.update(Msg::Navigate(View::Demos));
        app.state.showing = catalog::ENTRIES
            .iter()
            .position(|entry| entry.name == "drag")
            .unwrap();
        let before = draw_at(&mut app, 100, 40);
        let pane = app.embed.unwrap().rect;
        let label = pane
            .positions()
            .find(|&point| before[point].symbol() == "D")
            .unwrap();
        route(
            &mut app,
            mouse(MouseKind::Down(MouseButton::Left), label.x, label.y),
        );
        route(&mut app, Event::Key(KeyEvent::new(KeyCode::Esc)));
        assert!(!app.state.entered());
        route(
            &mut app,
            mouse(MouseKind::Up(MouseButton::Left), label.x, label.y),
        );
        route(&mut app, Event::Key(KeyEvent::new(KeyCode::Right)));
        assert!(app.state.entered());
        route(&mut app, mouse(MouseKind::Moved, label.x + 3, label.y));
        let after = draw_at(&mut app, 100, 40);
        let moved = pane
            .positions()
            .find(|&point| after[point].symbol() == "D")
            .unwrap();
        assert_eq!(
            moved, label,
            "a released pointer must not continue a retained child capture"
        );
    }

    #[test]
    fn pointer_exit_restores_preview_hover_suppression_without_an_up() {
        let Probed {
            mut app,
            seen,
            handled,
        } = probed();
        handled.set(true);
        let rect = app.embed.unwrap().rect;
        route(
            &mut app,
            mouse(MouseKind::Down(MouseButton::Left), rect.x + 1, rect.y + 1),
        );
        assert!(
            route(&mut app, mouse(MouseKind::Exited, 0, 0)),
            "exit handling must request a repaint"
        );
        assert!(
            !app.primary_down,
            "Exited ends tracking even when release happens outside the terminal"
        );
        // The backend does not deliver that release; the next event is hover on chrome.
        route(
            &mut app,
            mouse(MouseKind::Moved, rect.right() + 1, rect.y + 1),
        );
        assert_eq!(
            seen.take(),
            Some(mouse(MouseKind::Moved, u16::MAX, u16::MAX))
        );
    }

    #[test]
    fn preview_exit_precedes_resize_or_disappearance() {
        for change in ["resize", "too-small", "scroll"] {
            let Probed { mut app, seen, .. } = probed();
            if change == "scroll" {
                app.update(Msg::LandingScrolled(40));
                draw_at(&mut app, 100, 20);
            }
            let rect = app.embed.unwrap().rect;
            route(
                &mut app,
                mouse(MouseKind::Down(MouseButton::Left), rect.x + 1, rect.y + 1),
            );
            seen.take();
            match change {
                "resize" => {
                    draw_at(&mut app, 101, 40);
                }
                "too-small" => {
                    draw_at(&mut app, 10, 3);
                }
                "scroll" => {
                    app.update(Msg::LandingScrolled(0));
                    draw_at(&mut app, 100, 20);
                }
                _ => unreachable!(),
            }
            assert_eq!(
                seen.take(),
                Some(mouse(MouseKind::Exited, 0, 0)),
                "{change} must notify the old painted instance"
            );
            assert!(!app.primary_down);
            assert_eq!(app.embed.is_some(), change == "resize");
        }
    }

    #[test]
    fn switching_notifies_the_outgoing_demo_without_constructing_the_incoming_one() {
        for change_view in [false, true] {
            let mut app = App::new();
            let seen = Rc::default();
            app.opened[0] = Some(Box::new(Probe {
                seen: Rc::clone(&seen),
                handled: Rc::default(),
            }));
            app.update(Msg::Navigate(View::Demos));
            draw_at(&mut app, 100, 40);
            app.enter();
            if change_view {
                app.update(Msg::Navigate(View::GettingStarted));
            } else {
                let effects = catalog::ENTRIES
                    .iter()
                    .position(|entry| entry.name == "effects")
                    .unwrap();
                app.update(Msg::NavSelected(effects));
                assert!(
                    app.opened[effects].is_none(),
                    "cancellation must not start a network request"
                );
            }
            assert_eq!(seen.take(), Some(mouse(MouseKind::Exited, 0, 0)));
            assert!(!app.state.entered());
            assert!(app.embed.is_none());
            assert!(
                app.opened[0].is_some(),
                "the old app state remains available"
            );
        }
    }

    #[test]
    fn the_first_escape_leaves_the_mouse_driven_tooltip_demo() {
        let mut app = App::new();
        app.update(Msg::Navigate(View::Demos));
        app.state.showing = catalog::ENTRIES
            .iter()
            .position(|entry| entry.name == "tooltip")
            .unwrap();
        draw_at(&mut app, 100, 40);
        let pane = app.embed.unwrap().rect;
        route(
            &mut app,
            mouse(
                MouseKind::Down(MouseButton::Left),
                pane.x + pane.width / 2,
                pane.y,
            ),
        );
        route(
            &mut app,
            mouse(
                MouseKind::Up(MouseButton::Left),
                pane.x + pane.width / 2,
                pane.y,
            ),
        );
        assert!(app.state.entered());
        route(&mut app, Event::Key(KeyEvent::new(KeyCode::Esc)));
        assert!(
            !app.state.entered(),
            "an ignored Esc must not be claimed just to switch tooltip input mode"
        );
    }

    #[test]
    fn preview_pointer_tracking_ends_on_release_leave_hiding_and_view_change() {
        for reset in ["release", "leave", "hide", "navigate"] {
            let Probed { mut app, seen, .. } = probed();
            let rect = app.embed.unwrap().rect;
            route(
                &mut app,
                mouse(MouseKind::Down(MouseButton::Left), rect.x + 1, rect.y + 1),
            );
            assert!(app.primary_down);
            match reset {
                "release" => {
                    route(
                        &mut app,
                        mouse(
                            MouseKind::Up(MouseButton::Left),
                            rect.right() + 1,
                            rect.y + 1,
                        ),
                    );
                }
                "leave" => {
                    app.leave();
                    app.enter();
                }
                "hide" => {
                    draw_at(&mut app, 10, 3);
                    draw_at(&mut app, 100, 40);
                }
                "navigate" => {
                    app.update(Msg::Navigate(View::GettingStarted));
                    draw_at(&mut app, 100, 40);
                    app.update(Msg::Navigate(View::Landing));
                    draw_at(&mut app, 100, 40);
                    app.enter();
                }
                _ => unreachable!(),
            }
            assert!(
                !app.primary_down,
                "{reset} must end coordinate preservation"
            );
            seen.take();
            let rect = app.embed.unwrap().rect;
            route(
                &mut app,
                mouse(MouseKind::Moved, rect.right() + 1, rect.y + 1),
            );
            assert_eq!(
                seen.take(),
                Some(mouse(MouseKind::Moved, u16::MAX, u16::MAX)),
                "{reset} must restore outside hover suppression"
            );
        }
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
        assert_eq!((min.width, min.height), (43, 8));
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

        let inside = embed.translate(
            match mouse(MouseKind::Moved, 12, 6) {
                Event::Mouse(mouse) => mouse,
                _ => unreachable!(),
            },
            false,
        );
        assert_eq!((inside.column, inside.row), (2, 8));

        let dragged = embed.translate(
            match mouse(MouseKind::Drag(MouseButton::Left), 4, 20) {
                Event::Mouse(mouse) => mouse,
                _ => unreachable!(),
            },
            false,
        );
        assert_eq!(
            (dragged.column, dragged.row),
            (0, 22),
            "a drag outside the region keeps the constant shift, so a gesture \
             that strays out and back lands where the user means"
        );

        let hovered = embed.translate(
            match mouse(MouseKind::Moved, 4, 1) {
                Event::Mouse(mouse) => mouse,
                _ => unreachable!(),
            },
            false,
        );
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

    #[test]
    fn real_floating_and_modal_demos_respect_the_allocated_pane() {
        let theme = Theme::default_dark();
        let pane = Rect::new(40, 30, 60, 12);
        let local_area = Rect::new(0, 0, pane.width, pane.height);
        for (name, visible) in [
            ("select", "Mango"),
            ("tooltip", "This one"),
            ("dialog", "Sci-fi writers"),
        ] {
            let entry = catalog::ENTRIES
                .iter()
                .find(|entry| entry.name == name)
                .unwrap();
            let mut local_demo = entry.open();
            let mut hosted_demo = entry.open();
            let mut local = Buffer::empty(local_area);
            let mut hosted = Buffer::empty(Rect::new(0, 0, 160, 90));
            local_demo.draw(&mut local, local_area, &theme);
            hosted_demo.draw(&mut hosted, pane, &theme);
            let event = if name == "tooltip" {
                // The edge trigger must flip below the pane's top, not use
                // the ample space above it in the destination buffer.
                mouse(MouseKind::Moved, pane.width / 2, 0)
            } else {
                Event::Key(KeyEvent::new(KeyCode::Enter))
            };
            let hosted_event = match event.clone() {
                Event::Mouse(mut mouse) => {
                    mouse.column += pane.x;
                    mouse.row += pane.y;
                    Event::Mouse(mouse)
                }
                event => event,
            };
            local_demo.handle_event(event);
            hosted_demo.handle_event(hosted_event);
            local.reset();
            hosted.reset();
            let untouched = hosted.clone();
            local_demo.draw(&mut local, local_area, &theme);
            hosted_demo.draw(&mut hosted, pane, &theme);
            assert!(
                text_of(&local).contains(visible),
                "{name}'s overlay must be open"
            );
            for y in 0..hosted.area.height {
                for x in 0..hosted.area.width {
                    let expected = if pane.contains(Position::new(x, y)) {
                        &local[(x - pane.x, y - pane.y)]
                    } else {
                        &untouched[(x, y)]
                    };
                    assert_eq!(&hosted[(x, y)], expected, "{name} at ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn catalog_forwards_pane_bounds_to_oversized_modal_layers_and_dimming() {
        use ratatui::{style::Color, text::Line};
        use ratcn::runtime::ScopeOptions;

        struct ModalProbe(Ratcn<(), ()>);

        impl demo_shared::Demo for ModalProbe {
            fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
                let oversized = buffer.area;
                self.0.render_into(buffer, area, &(), theme, |ctx| {
                    ctx.modal_scope("modal", oversized, ScopeOptions::default(), |ctx| {
                        ctx.paint_widget(
                            Line::from("M".repeat(usize::from(oversized.width)))
                                .style(Color::Green),
                            Rect::new(oversized.x, area.y + area.height / 2, oversized.width, 1),
                        );
                    });
                });
            }
        }

        let mut app = App::new();
        app.update(Msg::Navigate(View::Demos));
        app.opened[0] = Some(Box::new(Probe::default()));
        let area = Rect::new(0, 0, 100, 20);
        let theme = Theme::default_dark();
        let mut before = Buffer::empty(area);
        for cell in &mut before.content {
            cell.set_symbol("#").set_fg(Color::Yellow);
        }
        demo_shared::Demo::draw(&mut app, &mut before, area, &theme);
        let pane = app.embed.unwrap().rect;
        app.opened[0] = Some(Box::new(ModalProbe(Ratcn::new())));
        let mut painted = before.clone();
        demo_shared::Demo::draw(&mut app, &mut painted, area, &theme);
        for position in area.positions() {
            if !pane.contains(position) {
                assert_eq!(
                    painted[position], before[position],
                    "modal changed chrome at {position:?}"
                );
            } else if position.y == pane.y + pane.height / 2 {
                assert_eq!(
                    painted[position].symbol(),
                    "M",
                    "the oversized layer must still copy into the pane"
                );
                assert_eq!(painted[position].fg, Color::Green);
            } else {
                assert_eq!(painted[position].symbol(), "#");
                assert_ne!(
                    painted[position].fg, before[position].fg,
                    "the pane's backdrop must dim"
                );
            }
        }
    }

    #[test]
    fn catalog_input_hits_absolute_pane_cells() {
        let mut app = App::new();
        app.update(Msg::Navigate(View::Demos));
        app.state.showing = catalog::ENTRIES
            .iter()
            .position(|entry| entry.name == "select")
            .unwrap();
        let closed = draw_at(&mut app, 100, 20);
        let pane = app.embed.unwrap().rect;
        let (x, y) = (pane.y..pane.bottom())
            .flat_map(|y| (pane.x..pane.right()).map(move |x| (x, y)))
            .find(|&cell| closed[cell].symbol() == "P")
            .expect("the Select placeholder is painted in the pane");
        route(&mut app, mouse(MouseKind::Down(MouseButton::Left), x, y));
        route(&mut app, mouse(MouseKind::Up(MouseButton::Left), x, y));
        let painted = draw_at(&mut app, 100, 20);
        assert!(app.state.entered());
        assert!(
            text_of(&painted).contains("Mango"),
            "the click must open the real Select at its absolute position"
        );
        assert_eq!(
            app.canvas.area,
            Rect::default(),
            "catalog drawing must not allocate an offscreen canvas"
        );
    }

    #[test]
    fn landing_reuses_a_cleared_buffer_without_stale_overlays() {
        let mut app = App::new();
        app.landing = Box::new(select::App::new());
        draw_at(&mut app, 100, 40);
        let original = app.canvas.clone();
        app.landing
            .handle_event(Event::Key(KeyEvent::new(KeyCode::Enter)));
        draw_at(&mut app, 100, 40);
        assert!(text_of(&app.canvas).contains("Mango"));
        app.landing
            .handle_event(Event::Key(KeyEvent::new(KeyCode::Esc)));
        draw_at(&mut app, 100, 40);
        assert_eq!(
            app.canvas, original,
            "a closed overlay must leave no stale cells in the reused buffer"
        );
    }

    #[test]
    fn landing_copies_the_scrolled_rows_without_changing_the_surrounding_page() {
        use ratatui::style::Color;

        struct Rows;

        impl demo_shared::Demo for Rows {
            fn draw(&mut self, buffer: &mut Buffer, _area: Rect, _theme: &Theme) {
                // Deliberately fill the whole offscreen buffer: only landing's
                // copy, not a catalog sandbox, bounds this paint on screen.
                for position in buffer.area.positions() {
                    buffer[position]
                        .set_symbol("R")
                        .set_bg(Color::Indexed(position.y as u8));
                }
            }
        }

        let Probed { mut app, .. } = probed();
        let original = app.canvas.area;
        app.state.landing_scroll = 40;
        let before = draw_at(&mut app, 90, 30);
        app.landing = Box::new(Rows);
        let painted = draw_at(&mut app, 90, 30);
        assert_ne!(
            app.canvas.area.width, original.width,
            "the preview must resize with the page"
        );
        let embed = app.embed.unwrap();
        assert!(
            embed.source_row > 0,
            "the preview must be cropped for this assertion"
        );
        assert_ne!(
            app.canvas[(0, 0)],
            app.canvas[(0, embed.source_row)],
            "source row zero must differ from the scrolled row"
        );
        for position in painted.area.positions() {
            if embed.rect.contains(position) {
                assert_eq!(
                    painted[position],
                    app.canvas[(
                        position.x - embed.rect.x,
                        embed.source_row + position.y - embed.rect.y
                    )],
                    "wrong source row copied at {position:?}"
                );
            } else {
                assert_eq!(
                    painted[position], before[position],
                    "preview copy changed the page at {position:?}"
                );
            }
        }
    }

    #[test]
    fn landing_alone_translates_pointer_coordinates() {
        let Probed { mut app, seen, .. } = probed();
        app.state.landing_scroll = 15;
        draw_at(&mut app, 100, 30);
        let embed = app.embed.unwrap();
        app.enter();
        route(
            &mut app,
            mouse(MouseKind::Moved, embed.rect.x + 2, embed.rect.y + 1),
        );
        assert_eq!(
            seen.take(),
            Some(mouse(MouseKind::Moved, 2, embed.source_row + 1))
        );

        app.leave();
        app.update(Msg::Navigate(View::Demos));
        app.opened[0] = Some(Box::new(Probe {
            seen: Rc::clone(&seen),
            handled: Rc::default(),
        }));
        draw_at(&mut app, 100, 30);
        let pane = app.embed.unwrap().rect;
        app.enter();
        let event = mouse(MouseKind::Moved, pane.x + 2, pane.y + 1);
        route(&mut app, event.clone());
        assert_eq!(seen.take(), Some(event));
    }

    #[test]
    fn entering_suppresses_chrome_focus_and_leaving_restores_the_real_path() {
        let Probed { mut app, .. } = probed();
        route(&mut app, Event::Key(KeyEvent::new(KeyCode::Tab)));
        let focus = app.state.focus.clone();
        assert!(focus.contains_path(["getting-started"]));
        app.enter();
        draw_at(&mut app, 100, 40);
        assert!(app.state.focus.is_none());
        assert_eq!(app.state.parked.as_ref(), Some(&focus));
        app.leave();
        assert_eq!(app.state.focus, focus);
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

        draw_at(&mut app, 100, 40);
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
        let rows = usize::from(demos::visible_rows(demos::columns(app.body.unwrap()).nav));
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
        assert_eq!(
            seen.take(),
            Some(mouse(MouseKind::Exited, 0, 0)),
            "the press belongs to chrome, but the outgoing demo must cancel its pointer"
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
        assert_eq!(
            seen.take(),
            Some(mouse(MouseKind::Exited, 0, 0)),
            "the ignored Esc is followed by cancellation, not a synthetic release"
        );
        assert!(!app.state.entered(), "and ignoring it gave the input back");
    }

    /// Hidden controls receive neither pointer nor keyboard input; Esc leaves at the host.
    #[test]
    fn a_demo_with_nothing_on_screen_is_not_reachable_by_the_pointer() {
        let Probed { mut app, seen, .. } = probed();
        app.enter();
        // A window too small for the chrome is one of the two states that
        // leaves the demo with nothing on screen while it still holds the
        // input; the page scrolling its preview out of view is the other.
        draw_at(&mut app, 10, 3);
        assert_eq!(
            seen.take(),
            Some(mouse(MouseKind::Exited, 0, 0)),
            "the visible instance is cancelled before its geometry is discarded"
        );
        assert!(app.embed.is_none(), "nothing of the demo is showing");
        assert!(app.state.entered(), "and it still has the input");

        assert!(
            !route(&mut app, mouse(MouseKind::Moved, 5, 1)),
            "the pointer finds nothing to reach"
        );
        assert!(seen.take().is_none(), "so the demo is never handed a cell");

        assert!(!route(&mut app, Event::Key(KeyEvent::new(KeyCode::Enter))));
        assert!(seen.take().is_none(), "hidden buttons must not activate");

        let escape = Event::Key(KeyEvent::new(KeyCode::Esc));
        route(&mut app, escape.clone());
        assert!(
            seen.take().is_none(),
            "Esc is handled by the host while hidden"
        );
        assert!(!app.state.entered(), "and Esc still gives the input back");
    }

    #[test]
    fn only_the_wheel_falls_through_to_the_chrome() {
        let Probed { mut app, .. } = probed();
        let inside = app.embed.expect("the demo is on screen").rect;
        let (column, row) = (inside.x + 1, inside.y + 1);
        let parked = FocusState::none();

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
             focus while the demo still owns the input"
        );
        assert_eq!(app.state.landing_scroll, scrolled, "and moves nothing");
    }
}
