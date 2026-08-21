//! A vertical viewport for arbitrary interactive ratcn descendants.
//!
//! Descendants are declared against their full logical content allocations.
//! The runtime translates and clips ordinary paint and pointer input without
//! changing those allocations, while keeping offscreen descendants in focus
//! traversal. Popup, hint, modal, and deferred paint escape the ordinary clip.

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::Theme;
use crate::runtime::{
    Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, MouseKind, ScopeOptions,
    ScrollDirection,
};
use crate::theme::resolve_style;

const WHEEL_ROWS: u16 = 3;

/// Every color a [`ScrollArea`] scrollbar paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollAreaStyle {
    /// Scrollbar thumb color.
    pub thumb: Color,
    /// Scrollbar track color.
    pub track: Color,
}

impl ScrollAreaStyle {
    /// A neutral style using plain ANSI colors.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            thumb: Color::White,
            track: Color::DarkGray,
        }
    }

    /// Derive the thumb and track from `theme`.
    #[must_use]
    pub const fn from_theme(theme: &Theme) -> Self {
        Self {
            thumb: theme.primary,
            track: theme.border,
        }
    }
}

/// A scroll position [`ScrollArea`] asks a bound app to store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAreaChange {
    /// The requested first visible content row.
    ScrollTo(u16),
}

/// Where the wheel, a key, or a reveal left the view.
///
/// Event handling writes it and the next declaration reads it, which is what
/// lets an unbound area scroll at all and what carries a reveal into the frame
/// that follows a focus change.
///
/// `base` is the bound offset the hold was taken against, and `held` is what
/// makes releasing permanent: the declaration drops the hold for good the
/// moment a bound offset moves away from `base`, so an app that scrolls its own
/// area keeps it, and returning to the offset the hold was taken at cannot
/// revive it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ScrollPark {
    offset: u16,
    held: bool,
    base: Option<u16>,
}

type ReadOffsetFn<S> = Box<dyn Fn(&S) -> u16>;
type OnChangeFn<M> = Box<dyn Fn(ScrollAreaChange) -> M>;
type ContentFn<S, M> = Box<dyn FnOnce(&mut DeclareCtx<'_, S, M>)>;
type StyleFn = Box<dyn Fn(&Theme) -> ScrollAreaStyle>;

/// A vertical viewport that hosts arbitrary interactive ratcn descendants.
///
/// One column is reserved at the right for the scrollbar. The
/// [`content`](Self::content) closure receives the remaining width and exactly
/// `content_height` logical rows, however many of them are visible. The
/// scrollbar is an indicator of where the view sits.
///
/// The offset is the area's own until [`offset`](Self::offset) binds it. Wheel,
/// Page Up, Page Down, Home, and End reach descendants first; what none of them
/// handles scrolls the area. An event that leaves the offset where it is — every
/// one of these keys at an edge, and a horizontal wheel — bubbles on to the app,
/// which keeps app hotkeys on those keys alive. The area is a fallback focus
/// stop while it holds no focusable descendant, so keyboard scrolling works for
/// paint-only content.
///
/// Focus moving to a descendant the viewport clips scrolls that descendant
/// into view.
///
/// # Panics
///
/// A `ScrollArea` inside another `ScrollArea` panics, as does content larger
/// than 262,144 cells.
///
/// ```
/// # use ratatui::layout::Rect;
/// # use ratcn::runtime::DeclareCtx;
/// # use ratcn::{Button, ScrollArea};
/// # struct State;
/// # enum Msg { Saved }
/// # fn declare(ctx: &mut DeclareCtx<'_, State, Msg>) {
/// ctx.component(
///     "settings",
///     ScrollArea::new(40).content(|ctx| {
///         ctx.component(
///             "save",
///             Button::new("Save").on_press(|| Msg::Saved),
///             Rect::new(0, 0, 10, 3),
///         );
///     }),
///     Rect::new(0, 0, 30, 10),
/// );
/// # }
/// ```
pub struct ScrollArea<S, M> {
    content_height: u16,
    read_offset: Option<ReadOffsetFn<S>>,
    on_change: Option<OnChangeFn<M>>,
    content: Option<ContentFn<S, M>>,
    style: Option<StyleFn>,
    hover_focus: bool,
}

impl<S, M> std::fmt::Debug for ScrollArea<S, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollArea")
            .field("content_height", &self.content_height)
            .field("content", &self.content.is_some())
            .field("hover_focus", &self.hover_focus)
            .finish_non_exhaustive()
    }
}

impl<S, M> ScrollArea<S, M> {
    /// A viewport over `content_height` logical rows.
    #[must_use]
    pub fn new(content_height: u16) -> Self {
        Self {
            content_height,
            read_offset: None,
            on_change: None,
            content: None,
            style: None,
            hover_focus: false,
        }
    }

    /// Bind the first visible content row to app state.
    ///
    /// `read` is consulted for every event, so repeated wheel or page events
    /// compose before a redraw, and `on_change` carries each new offset to the
    /// app's update. Left unbound, the area keeps the offset itself and emits
    /// nothing.
    ///
    /// A reveal moves the view on its own, without a message; the next offset
    /// the area emits starts from where the reveal left it.
    #[must_use]
    pub fn offset(
        mut self,
        read: impl Fn(&S) -> u16 + 'static,
        on_change: impl Fn(ScrollAreaChange) -> M + 'static,
    ) -> Self {
        self.read_offset = Some(Box::new(read));
        self.on_change = Some(Box::new(on_change));
        self
    }

    /// Declare the viewport's descendants in their full logical content area.
    #[must_use]
    pub fn content(mut self, content: impl FnOnce(&mut DeclareCtx<'_, S, M>) + 'static) -> Self {
        self.content = Some(Box::new(content));
        self
    }

    /// Replace the theme-derived scrollbar style.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> ScrollAreaStyle + 'static) -> Self {
        self.style = Some(Box::new(style));
        self
    }

    /// Make pointer motion choose between the content's direct child scopes.
    ///
    /// This is opt-in so ordinary scrollable forms leave keyboard focus alone
    /// when the pointer drifts. It suits a scrollable pane or tile grid whose
    /// direct children are the regions focus should follow.
    #[must_use]
    pub const fn hover_focus(mut self) -> Self {
        self.hover_focus = true;
        self
    }

    /// The visible content rectangle inside `area`: everything but the
    /// scrollbar gutter.
    fn viewport(area: Rect) -> Rect {
        Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height)
    }

    fn max_offset(&self, area: Rect) -> u16 {
        self.content_height
            .saturating_sub(Self::viewport(area).height)
    }

    fn bound_offset(&self, state: &S) -> Option<u16> {
        self.read_offset.as_ref().map(|read| read(state))
    }

    /// The offset in force: a standing hold, or the bound value.
    fn resolve(&self, area: Rect, bound: Option<u16>, park: ScrollPark) -> u16 {
        let offset = if park.held && park.base == bound {
            park.offset
        } else {
            bound.unwrap_or(0)
        };
        offset.min(self.max_offset(area))
    }

    /// Release a hold the app has scrolled out from under, and answer with the
    /// offset this declaration lays out from.
    ///
    /// Releasing happens here, once per frame, because the declaration is
    /// where a bound offset is read; and it is permanent, so an app that
    /// returns to the offset a hold was taken at does not revive it.
    fn settle(&self, ctx: &mut DeclareCtx<'_, S, M>, area: Rect, bound: Option<u16>) -> u16 {
        let mut unheld = ScrollPark::default();
        let park = ctx.transient_mut::<ScrollPark>().unwrap_or(&mut unheld);
        if park.held && park.base != bound {
            *park = ScrollPark::default();
        }
        self.resolve(area, bound, *park)
    }

    /// The offset in force at event time.
    fn current(&self, state: &S, ctx: &mut EventCtx<'_>) -> u16 {
        let park = *ctx.transient::<ScrollPark>();
        self.resolve(ctx.area(), self.bound_offset(state), park)
    }

    /// Hold the view at `offset` for the coming declaration, and report the
    /// offset taken. `None` when the view is already there.
    fn park(&self, offset: u16, state: &S, ctx: &mut EventCtx<'_>) -> Option<u16> {
        let area = ctx.area();
        let bound = self.bound_offset(state);
        let current = self.current(state, ctx);
        let offset = offset.min(self.max_offset(area));
        if offset == current {
            return None;
        }
        *ctx.transient::<ScrollPark>() = ScrollPark {
            offset,
            held: true,
            base: bound,
        };
        Some(offset)
    }

    /// Scroll to `offset`.
    ///
    /// [`EventResult::Ignored`] when the view is already there, which leaves an
    /// app hotkey on Home, End, or a page key working while focus rests in an
    /// area with nothing to scroll.
    fn scroll_to(&self, offset: u16, state: &S, ctx: &mut EventCtx<'_>) -> EventResult<M> {
        let Some(offset) = self.park(offset, state, ctx) else {
            return EventResult::Ignored;
        };
        match &self.on_change {
            Some(on_change) => EventResult::Emit(on_change(ScrollAreaChange::ScrollTo(offset))),
            None => EventResult::Consumed,
        }
    }

    /// The smallest offset change that puts `target` on screen.
    fn reveal_offset(&self, target: Rect, area: Rect, current: u16) -> u16 {
        let viewport = Self::viewport(area);
        let visible_top = viewport.y.saturating_add(current);
        let visible_bottom = visible_top.saturating_add(viewport.height);
        let requested = if target.height > viewport.height || target.y < visible_top {
            target.y.saturating_sub(viewport.y)
        } else if target.bottom() > visible_bottom {
            target
                .bottom()
                .saturating_sub(viewport.height)
                .saturating_sub(viewport.y)
        } else {
            current
        };
        requested.min(self.max_offset(area))
    }
}

impl<S: 'static, M: 'static> Component<S, M> for ScrollArea<S, M> {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, S, M>) {
        let area = ctx.area();
        let viewport = Self::viewport(area);
        let bound = self.bound_offset(ctx.state());
        let offset = self.settle(ctx, area, bound);
        let content = self.content.take();
        ctx.viewport(viewport, self.content_height, offset, |ctx| {
            if let Some(content) = content {
                content(ctx);
            }
        });

        if self.content_height <= viewport.height || area.width == 0 || area.height == 0 {
            return;
        }
        let gutter = Rect::new(area.right().saturating_sub(1), area.y, 1, area.height);
        let style = resolve_style(
            self.style.as_deref(),
            ctx.theme,
            ScrollAreaStyle::from_theme,
        );
        let viewport_height = viewport.height;
        // Ratatui 0.3.2 defines `content_length - 1` as the maximum
        // scrollbar position. ScrollArea positions are row offsets, so the
        // count is the inclusive offset range, not the total row count.
        let position_count = self
            .content_height
            .saturating_sub(viewport_height)
            .saturating_add(1);
        ctx.paint(move |ctx| {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(Style::new().fg(style.thumb))
                .track_style(Style::new().fg(style.track));
            let mut state = ScrollbarState::new(usize::from(position_count))
                .position(usize::from(offset))
                .viewport_content_length(usize::from(viewport_height));
            ctx.stateful_widget(scrollbar, gutter, &mut state);
        });
    }

    fn handle_event(&mut self, event: &Event, state: &S, ctx: &mut EventCtx<'_>) -> EventResult<M> {
        let area = ctx.area();
        let current = self.current(state, ctx);
        let page = Self::viewport(area).height;
        let target = match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseKind::Scroll(ScrollDirection::Up) => current.saturating_sub(WHEEL_ROWS),
                MouseKind::Scroll(ScrollDirection::Down) => current.saturating_add(WHEEL_ROWS),
                _ => return EventResult::Ignored,
            },
            Event::Key(key) if !key.modifiers.any() => match key.code {
                KeyCode::PageUp => current.saturating_sub(page),
                KeyCode::PageDown => current.saturating_add(page),
                KeyCode::Home => 0,
                KeyCode::End => self.max_offset(area),
                _ => return EventResult::Ignored,
            },
            _ => return EventResult::Ignored,
        };
        self.scroll_to(target, state, ctx)
    }

    fn reveal_in_viewport(&mut self, target: Rect, state: &S, ctx: &mut EventCtx<'_>) {
        let area = ctx.area();
        let current = self.current(state, ctx);
        self.park(self.reveal_offset(target, area, current), state, ctx);
    }

    fn scope_options(&self) -> ScopeOptions {
        let options = ScopeOptions::default().focusable();
        if self.hover_focus {
            options.hover_focus()
        } else {
            options
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        rc::Rc,
    };

    use ratatui::{
        Terminal,
        backend::TestBackend,
        text::Text,
        widgets::{Block, Paragraph},
    };

    use super::*;
    use crate::runtime::{
        ChildId, FocusState, KeyChord, KeyEvent, Modifiers, MouseButton, MouseEvent, PopupOptions,
        Ratcn,
    };
    use crate::{Button, ListItem, Select, Tooltip};

    #[derive(Default)]
    struct State {
        focus: FocusState,
        offset: u16,
        select_open: bool,
        select_cursor: Option<&'static str>,
        selected: Option<&'static str>,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Area(ScrollAreaChange),
        Focus(FocusState),
        Pressed(&'static str),
        ChildHandled,
        SelectOpen(bool),
        SelectFocused(&'static str),
        Selected(&'static str),
        Escape,
        ModalClosed,
    }

    struct Driver {
        terminal: Terminal<TestBackend>,
        ratcn: Ratcn<State, Msg>,
    }

    impl Driver {
        fn new(width: u16, height: u16) -> Self {
            Self {
                terminal: Terminal::new(TestBackend::new(width, height)).expect("terminal"),
                ratcn: Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
            }
        }

        fn render(&mut self, state: &State, declare: impl FnOnce(&mut DeclareCtx<'_, State, Msg>)) {
            let theme = Theme::default_dark();
            self.terminal
                .draw(|frame| self.ratcn.render(frame, state, &theme, declare))
                .expect("draw");
        }

        fn event(&mut self, event: Event, state: &State) -> EventResult<Msg> {
            self.ratcn.handle_event(event, state)
        }

        fn row(&self, row: u16) -> String {
            let buffer = self.terminal.backend().buffer();
            (0..buffer.area.width)
                .map(|column| buffer.cell((column, row)).expect("cell").symbol())
                .collect()
        }
    }

    fn scroll_area(
        content_height: u16,
        content: impl FnOnce(&mut DeclareCtx<'_, State, Msg>) + 'static,
    ) -> ScrollArea<State, Msg> {
        ScrollArea::new(content_height)
            .offset(|state: &State| state.offset, Msg::Area)
            .content(content)
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code))
    }

    fn mouse(kind: MouseKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: Modifiers::NONE,
        })
    }

    #[derive(Debug)]
    struct Probe {
        name: &'static str,
        focusable: bool,
        handle_page_down: bool,
    }

    impl Probe {
        const fn focusable(name: &'static str) -> Self {
            Self {
                name,
                focusable: true,
                handle_page_down: false,
            }
        }

        const fn page_sink(name: &'static str) -> Self {
            Self {
                name,
                focusable: true,
                handle_page_down: true,
            }
        }
    }

    impl Component<State, Msg> for Probe {
        fn declare(&mut self, _ctx: &mut DeclareCtx<'_, State, Msg>) {}

        fn paint(&mut self, ctx: &mut crate::runtime::PaintCtx<'_, '_, State>) {
            ctx.widget(Paragraph::new(self.name), ctx.area());
        }

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &State,
            _ctx: &mut EventCtx<'_>,
        ) -> EventResult<Msg> {
            match event {
                Event::Key(key)
                    if self.handle_page_down
                        && key.code == KeyCode::PageDown
                        && !key.modifiers.any() =>
                {
                    EventResult::Emit(Msg::ChildHandled)
                }
                Event::Mouse(mouse) if mouse.kind == MouseKind::Click(MouseButton::Left) => {
                    EventResult::Emit(Msg::Pressed(self.name))
                }
                _ => EventResult::Ignored,
            }
        }

        fn is_focusable(&self, _state: &State) -> bool {
            self.focusable
        }
    }

    #[test]
    fn ordinary_paint_uses_full_logical_allocation_then_translates_and_clips() {
        let mut driver = Driver::new(8, 5);
        let state = State {
            offset: 2,
            ..State::default()
        };
        driver.render(&state, |ctx| {
            ctx.component(
                "scroll",
                scroll_area(6, |ctx| {
                    let area = ctx.area();
                    ctx.paint_widget(
                        Paragraph::new(Text::from("00000\n11111\n22222\n33333\n44444\n55555")),
                        area,
                    );
                }),
                Rect::new(1, 1, 6, 3),
            );
        });

        assert_eq!(&driver.row(1)[1..6], "22222");
        assert_eq!(&driver.row(2)[1..6], "33333");
        assert_eq!(&driver.row(3)[1..6], "44444");
        for (column, row) in [(0, 1), (7, 1), (1, 0), (1, 4)] {
            assert_eq!(
                driver
                    .terminal
                    .backend()
                    .buffer()
                    .cell((column, row))
                    .expect("outside cell")
                    .symbol(),
                " ",
                "viewport paint escaped at ({column}, {row})"
            );
        }
    }

    #[test]
    fn empty_and_sparse_viewports_preserve_earlier_frame_cells() {
        for sparse in [false, true] {
            let mut driver = Driver::new(6, 2);
            let state = State::default();
            driver.render(&state, move |ctx| {
                ctx.paint_widget(
                    Paragraph::new("ABCDE\nFGHIJ").style(Style::new().bg(Color::Red)),
                    Rect::new(0, 0, 5, 2),
                );
                ctx.component(
                    "scroll",
                    scroll_area(2, move |ctx| {
                        if sparse {
                            ctx.paint_widget(Paragraph::new("X"), Rect::new(2, 0, 1, 1));
                        }
                    }),
                    Rect::new(0, 0, 6, 2),
                );
            });

            assert_eq!(&driver.row(0)[..5], if sparse { "ABXDE" } else { "ABCDE" });
            assert_eq!(&driver.row(1)[..5], "FGHIJ");
            let buffer = driver.terminal.backend().buffer();
            for position in [(0, 0), (4, 0), (0, 1), (4, 1)] {
                assert_eq!(
                    buffer.cell(position).expect("preserved cell").bg,
                    Color::Red,
                    "untouched styles survive viewport composition"
                );
            }
        }
    }

    #[test]
    fn partially_visible_fixed_height_control_keeps_its_real_allocation() {
        let mut driver = Driver::new(8, 4);
        let state = State {
            offset: 2,
            ..State::default()
        };
        driver.render(&state, |ctx| {
            ctx.component(
                "scroll",
                scroll_area(6, |ctx| {
                    let area = ctx.area();
                    ctx.component(
                        "button",
                        Button::new("OK")
                            .outline()
                            .size(crate::ButtonSize::Large)
                            .on_press(|| Msg::Pressed("button")),
                        Rect::new(area.x, area.y + 1, area.width, 3),
                    );
                }),
                Rect::new(1, 0, 6, 3),
            );
        });

        assert!(driver.row(0).contains("OK"), "{}", driver.row(0));
        assert!(driver.row(1).contains('└'), "{}", driver.row(1));
        assert_eq!(&driver.row(2)[1..6], "     ");
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 2, 0), &state),
            EventResult::Emit(Msg::Pressed("button")),
            "the visible middle row remains interactive"
        );
    }

    #[test]
    fn pointer_routing_inverse_translates_visible_hits_and_clips_offscreen_hits() {
        let mut driver = Driver::new(8, 5);
        let state = State {
            offset: 2,
            ..State::default()
        };
        driver.render(&state, |ctx| {
            ctx.component(
                "scroll",
                scroll_area(8, |ctx| {
                    let area = ctx.area();
                    ctx.component(
                        "visible",
                        Probe::focusable("visible"),
                        Rect::new(area.x, area.y + 3, area.width, 1),
                    );
                    ctx.component(
                        "offscreen",
                        Probe::focusable("offscreen"),
                        Rect::new(area.x, area.y + 7, area.width, 1),
                    );
                }),
                Rect::new(1, 1, 6, 3),
            );
        });

        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 2, 2), &state),
            EventResult::Emit(Msg::Pressed("visible"))
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 2, 8), &state),
            EventResult::Ignored,
            "an offscreen logical allocation is not a screen hit target"
        );
    }

    /// A focus change inside a `ScrollArea` reaches the app through the same
    /// `Ratcn::focus` binding every other focus change uses, and the reveal
    /// arrives with it.
    #[test]
    fn tab_into_an_offscreen_descendant_emits_the_focus_message_and_reveals_it() {
        let mut driver = Driver::new(8, 3);
        let mut state = State {
            focus: FocusState::intent(["scroll", "first"]),
            ..State::default()
        };
        let render = |driver: &mut Driver, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(9, |ctx| {
                        assert_eq!(ctx.area().height, 9, "children see full content height");
                        ctx.component("first", Probe::focusable("first"), Rect::new(0, 0, 7, 1));
                        ctx.component("last", Probe::focusable("last"), Rect::new(0, 6, 7, 1));
                    }),
                    Rect::new(0, 0, 8, 3),
                );
            });
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(key(KeyCode::Tab), &state),
            EventResult::Emit(Msg::Focus(FocusState::intent(["scroll", "last"]))),
            "the app's focus binding fires for a focus change inside a ScrollArea"
        );
        state.focus = FocusState::intent(["scroll", "last"]);
        render(&mut driver, &state);
        assert_eq!(
            &driver.row(2)[..4],
            "last",
            "the reveal reached the same frame the focus change did"
        );
    }

    /// A reveal holds the view until the app scrolls its own bound offset, and
    /// releasing it is permanent: coming back to the offset the hold was taken
    /// at must not scroll the reveal back in.
    #[test]
    fn a_bound_offset_takes_the_area_back_for_good() {
        let mut driver = Driver::new(8, 3);
        let mut state = State {
            focus: FocusState::intent(["scroll", "first"]),
            ..State::default()
        };
        let render = |driver: &mut Driver, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(9, |ctx| {
                        ctx.component("first", Probe::focusable("first"), Rect::new(0, 0, 7, 1));
                        ctx.component("last", Probe::focusable("last"), Rect::new(0, 6, 7, 1));
                    }),
                    Rect::new(0, 0, 8, 3),
                );
            });
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(key(KeyCode::Tab), &state),
            EventResult::Emit(Msg::Focus(FocusState::intent(["scroll", "last"])))
        );
        state.focus = FocusState::intent(["scroll", "last"]);
        render(&mut driver, &state);
        assert_eq!(&driver.row(2)[..4], "last", "the reveal held the view");

        state.offset = 2;
        render(&mut driver, &state);
        assert_eq!(
            &driver.row(0)[..5],
            "     ",
            "the app scrolled its own offset, so the hold is released"
        );

        state.offset = 0;
        render(&mut driver, &state);
        assert_eq!(
            &driver.row(0)[..5],
            "first",
            "returning to the offset the hold was taken at must not revive it"
        );
    }

    /// An unbound area owns the reveal outright: the app stores focus and
    /// nothing else.
    #[test]
    fn an_unbound_area_reveals_without_asking_the_app_for_an_offset() {
        let mut driver = Driver::new(8, 3);
        let mut state = State {
            focus: FocusState::intent(["scroll", "first"]),
            ..State::default()
        };
        let render = |driver: &mut Driver, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    "scroll",
                    ScrollArea::new(9).content(|ctx| {
                        ctx.component("first", Probe::focusable("first"), Rect::new(0, 0, 7, 1));
                        ctx.component("last", Probe::focusable("last"), Rect::new(0, 6, 7, 1));
                    }),
                    Rect::new(0, 0, 8, 3),
                );
            });
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(key(KeyCode::Tab), &state),
            EventResult::Emit(Msg::Focus(FocusState::intent(["scroll", "last"])))
        );
        state.focus = FocusState::intent(["scroll", "last"]);
        render(&mut driver, &state);
        assert_eq!(&driver.row(2)[..4], "last");
    }

    #[test]
    fn backtab_and_focus_keys_minimally_reveal_their_destination() {
        let mut driver = Driver::new(8, 3);
        driver.ratcn = std::mem::take(&mut driver.ratcn)
            .focus_key(KeyChord::from('l').alt(), ["scroll", "last"]);
        let mut state = State {
            focus: FocusState::intent(["scroll", "last"]),
            offset: 4,
            ..State::default()
        };
        let render = |driver: &mut Driver, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(9, |ctx| {
                        ctx.component("first", Probe::focusable("first"), Rect::new(0, 0, 7, 1));
                        ctx.component("last", Probe::focusable("last"), Rect::new(0, 6, 7, 1));
                    }),
                    Rect::new(0, 0, 8, 3),
                );
            });
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(key(KeyCode::BackTab), &state),
            EventResult::Emit(Msg::Focus(FocusState::intent(["scroll", "first"])))
        );
        state.focus = FocusState::intent(["scroll", "first"]);
        render(&mut driver, &state);
        assert_eq!(
            &driver.row(0)[..5],
            "first",
            "the top edge is the minimal reveal for a target above the view"
        );

        let jump = Event::Key(KeyEvent {
            code: KeyCode::Char('l'),
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        });
        assert_eq!(
            driver.event(jump, &state),
            EventResult::Emit(Msg::Focus(FocusState::intent(["scroll", "last"])))
        );
        state.focus = FocusState::intent(["scroll", "last"]);
        render(&mut driver, &state);
        assert_eq!(
            &driver.row(2)[..4],
            "last",
            "the bottom edge is the minimal reveal for a target below the view"
        );
    }

    #[test]
    fn focus_key_reveals_an_already_focused_offscreen_descendant() {
        let mut driver = Driver::new(8, 3);
        driver.ratcn = std::mem::take(&mut driver.ratcn)
            .focus_key(KeyChord::from('l').alt(), ["scroll", "last"]);
        let state = State {
            focus: FocusState::intent(["scroll", "last"]),
            ..State::default()
        };
        let render = |driver: &mut Driver, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(9, |ctx| {
                        ctx.component("last", Probe::focusable("last"), Rect::new(0, 6, 7, 1));
                    }),
                    Rect::new(0, 0, 8, 3),
                );
            });
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(
                Event::Key(KeyEvent {
                    code: KeyCode::Char('l'),
                    modifiers: Modifiers {
                        alt: true,
                        ..Modifiers::NONE
                    },
                }),
                &state,
            ),
            EventResult::Consumed,
            "focus did not change, so there is no focus message to send"
        );
        render(&mut driver, &state);
        assert_eq!(&driver.row(2)[..4], "last");
    }

    #[test]
    fn wrapped_traversal_reveals_its_same_offscreen_target() {
        let mut driver = Driver::new(8, 3);
        let state = State {
            focus: FocusState::intent(["scroll", "wrap", "only"]),
            ..State::default()
        };
        let render = |driver: &mut Driver, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(9, |ctx| {
                        ctx.scope(
                            "wrap",
                            ctx.area(),
                            ScopeOptions::default().tab_wrap(crate::runtime::TabWrap::Wrap),
                            |ctx| {
                                ctx.component(
                                    "only",
                                    Probe::focusable("only"),
                                    Rect::new(0, 6, 7, 1),
                                );
                            },
                        );
                    }),
                    Rect::new(0, 0, 8, 3),
                );
            });
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(key(KeyCode::Tab), &state),
            EventResult::Consumed,
            "the wrap landed back where it started, so focus did not change"
        );
        render(&mut driver, &state);
        assert_eq!(&driver.row(2)[..4], "only");
    }

    #[test]
    fn repeated_controlled_events_read_current_app_offset_before_redraw() {
        let mut driver = Driver::new(8, 3);
        let mut state = State::default();
        driver.render(&state, |ctx| {
            ctx.component("scroll", scroll_area(12, |_| {}), Rect::new(0, 0, 8, 3));
        });

        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 1, 1),
                &state
            ),
            EventResult::Emit(Msg::Area(ScrollAreaChange::ScrollTo(3)))
        );
        state.offset = 3;
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 1, 1),
                &state
            ),
            EventResult::Emit(Msg::Area(ScrollAreaChange::ScrollTo(6)))
        );
        state.offset = 6;
        assert_eq!(
            driver.event(key(KeyCode::PageDown), &state),
            EventResult::Emit(Msg::Area(ScrollAreaChange::ScrollTo(9)))
        );
    }

    /// An unbound area owns its offset the same way an unbound `List` owns
    /// its scroll: the wheel moves the view and the offset survives the
    /// redraw, with nothing emitted.
    #[test]
    fn an_unbound_area_scrolls_itself_on_the_wheel() {
        let mut driver = Driver::new(8, 3);
        let state = State::default();
        let render = |driver: &mut Driver, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    "scroll",
                    ScrollArea::new(9).content(|ctx| {
                        let area = ctx.area();
                        ctx.paint_widget(
                            Paragraph::new(Text::from("a\nb\nc\nd\ne\nf\ng\nh\ni")),
                            area,
                        );
                    }),
                    Rect::new(0, 0, 8, 3),
                );
            });
        };
        render(&mut driver, &state);
        assert_eq!(&driver.row(0)[..1], "a");

        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 1, 1),
                &state
            ),
            EventResult::Consumed,
            "there is no offset binding, so nothing is emitted"
        );
        render(&mut driver, &state);
        assert_eq!(
            &driver.row(0)[..1],
            "d",
            "the wheel moved the view by itself"
        );
    }

    /// A key the view cannot act on has to reach the app, or an app hotkey on
    /// Home, End, or a page key dies whenever focus rests in a scroll area.
    #[test]
    fn scroll_keys_and_wheel_clamp_and_bubble_at_the_edges() {
        let mut driver = Driver::new(8, 3);
        let mut state = State {
            offset: 5,
            ..State::default()
        };
        driver.render(&state, |ctx| {
            ctx.component("scroll", scroll_area(8, |_| {}), Rect::new(0, 0, 8, 3));
        });

        assert_eq!(
            driver.event(key(KeyCode::End), &state),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 1, 1),
                &state
            ),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(key(KeyCode::Home), &state),
            EventResult::Emit(Msg::Area(ScrollAreaChange::ScrollTo(0)))
        );
        state.offset = 0;
        assert_eq!(
            driver.event(key(KeyCode::PageUp), &state),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Left), 1, 1),
                &state
            ),
            EventResult::Ignored
        );
        assert_eq!(
            driver.event(
                Event::Key(KeyEvent {
                    code: KeyCode::End,
                    modifiers: Modifiers {
                        ctrl: true,
                        ..Modifiers::NONE
                    },
                }),
                &state
            ),
            EventResult::Ignored
        );
    }

    /// The same keys in an area that has nothing to scroll.
    #[test]
    fn an_area_that_cannot_scroll_leaves_its_keys_to_the_app() {
        let mut driver = Driver::new(8, 5);
        let state = State::default();
        driver.render(&state, |ctx| {
            ctx.component("scroll", scroll_area(2, |_| {}), Rect::new(0, 0, 8, 5));
        });

        for code in [
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
        ] {
            assert_eq!(
                driver.event(key(code), &state),
                EventResult::Ignored,
                "{code:?} has nothing to scroll and belongs to the app"
            );
        }
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 1, 1),
                &state
            ),
            EventResult::Ignored
        );
    }

    #[test]
    fn focused_descendant_gets_scroll_keys_before_the_area() {
        let mut driver = Driver::new(8, 3);
        let state = State {
            focus: FocusState::intent(["scroll", "sink"]),
            ..State::default()
        };
        driver.render(&state, |ctx| {
            ctx.component(
                "scroll",
                scroll_area(8, |ctx| {
                    ctx.component("sink", Probe::page_sink("sink"), Rect::new(0, 0, 7, 1));
                }),
                Rect::new(0, 0, 8, 3),
            );
        });

        assert_eq!(
            driver.event(key(KeyCode::PageDown), &state),
            EventResult::Emit(Msg::ChildHandled)
        );
    }

    #[test]
    fn reserved_gutter_uses_the_configured_ratatui_scrollbar_style() {
        let mut driver = Driver::new(6, 4);
        let state = State::default();
        driver.render(&state, |ctx| {
            ctx.component(
                "scroll",
                scroll_area(12, |ctx| {
                    ctx.paint_widget(Block::new().style(Style::new().bg(Color::Red)), ctx.area());
                })
                .style(|_| ScrollAreaStyle {
                    thumb: Color::Yellow,
                    track: Color::Blue,
                }),
                Rect::new(0, 0, 6, 4),
            );
        });

        let buffer = driver.terminal.backend().buffer();
        assert_eq!(buffer.cell((4, 0)).expect("content cell").bg, Color::Red);
        let gutter = (0..4)
            .map(|row| buffer.cell((5, row)).expect("gutter cell"))
            .collect::<Vec<_>>();
        assert!(gutter.iter().any(|cell| cell.fg == Color::Yellow));
        assert!(gutter.iter().any(|cell| cell.fg == Color::Blue));
        assert!(gutter.iter().all(|cell| cell.bg != Color::Red));
    }

    #[test]
    fn scrollbar_thumb_reaches_both_offset_endpoints() {
        let mut driver = Driver::new(6, 4);
        let mut state = State::default();
        let render = |driver: &mut Driver, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(12, |_| {}).style(|_| ScrollAreaStyle {
                        thumb: Color::Yellow,
                        track: Color::Blue,
                    }),
                    Rect::new(0, 0, 6, 4),
                );
            });
        };
        let thumb_rows = |driver: &Driver| {
            (0..4)
                .filter(|&row| {
                    driver
                        .terminal
                        .backend()
                        .buffer()
                        .cell((5, row))
                        .is_some_and(|cell| cell.fg == Color::Yellow)
                })
                .collect::<Vec<_>>()
        };

        render(&mut driver, &state);
        assert_eq!(thumb_rows(&driver).first(), Some(&0));

        state.offset = 8;
        render(&mut driver, &state);
        assert_eq!(thumb_rows(&driver).last(), Some(&3));
    }

    /// One row of overflow is two offsets, not one, so the thumb has to reach
    /// both ends of the gutter.
    #[test]
    fn a_single_row_of_overflow_still_reaches_both_scrollbar_endpoints() {
        let mut driver = Driver::new(6, 4);
        let mut state = State::default();
        let render = |driver: &mut Driver, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(5, |_| {}).style(|_| ScrollAreaStyle {
                        thumb: Color::Yellow,
                        track: Color::Blue,
                    }),
                    Rect::new(0, 0, 6, 4),
                );
            });
        };
        let thumb_rows = |driver: &Driver| {
            (0..4)
                .filter(|&row| {
                    driver
                        .terminal
                        .backend()
                        .buffer()
                        .cell((5, row))
                        .is_some_and(|cell| cell.fg == Color::Yellow)
                })
                .collect::<Vec<_>>()
        };

        render(&mut driver, &state);
        let top = thumb_rows(&driver);
        assert_eq!(top.first(), Some(&0));
        assert_ne!(top.last(), Some(&3), "one row is still below the view");

        state.offset = 1;
        render(&mut driver, &state);
        let bottom = thumb_rows(&driver);
        assert_eq!(bottom.last(), Some(&3));
        assert_ne!(bottom.first(), Some(&0), "one row is now above the view");
    }

    #[test]
    fn zero_sized_viewports_are_inert_without_dropping_declarations() {
        for area in [Rect::new(0, 0, 0, 3), Rect::new(0, 0, 4, 0)] {
            let mut driver = Driver::new(4, 3);
            let state = State::default();
            driver.render(&state, move |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(5, |ctx| {
                        ctx.component("child", Probe::focusable("child"), ctx.area());
                    }),
                    area,
                );
            });

            assert!(
                driver
                    .ratcn
                    .focus_path(&[ChildId::from("scroll"), ChildId::from("child")])
                    .is_none()
            );
            assert_eq!(
                driver.event(key(KeyCode::Tab), &state),
                EventResult::Ignored
            );
        }
    }

    #[derive(Debug)]
    struct LayerHost {
        layer: LayerExample,
    }

    #[derive(Debug)]
    struct HoverPopupHost;

    impl Component<State, Msg> for HoverPopupHost {
        fn declare(&mut self, ctx: &mut DeclareCtx<'_, State, Msg>) {
            let anchor = ctx.area();
            ctx.paint_widget(Paragraph::new("OWNER"), anchor);
            if ctx.pointer_within() {
                let popup = Rect::new(anchor.x, anchor.y + 2, 5, 1);
                ctx.popup("popup", PopupOptions::default(), popup, move |ctx| {
                    ctx.paint_widget(Paragraph::new("POPUP"), popup);
                    ctx.component("item", Probe::focusable("popup-item"), popup);
                });
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum LayerExample {
        Hint,
        Popup,
        Modal,
        Deferred,
    }

    impl Component<State, Msg> for LayerHost {
        fn declare(&mut self, ctx: &mut DeclareCtx<'_, State, Msg>) {
            let anchor = ctx.area();
            let escaped = Rect::new(anchor.x, anchor.y + 2, 5, 1);
            match self.layer {
                LayerExample::Hint => {
                    ctx.hint("layer", ScopeOptions::default(), escaped, move |ctx| {
                        ctx.paint_widget(Paragraph::new("HINT"), escaped);
                    });
                }
                LayerExample::Popup => {
                    ctx.popup("layer", PopupOptions::default(), escaped, move |ctx| {
                        ctx.paint_widget(Paragraph::new("POPUP"), escaped);
                        ctx.component("item", Probe::focusable("item"), escaped);
                    });
                }
                LayerExample::Modal => {
                    ctx.modal("layer", ModalProbe, escaped);
                }
                LayerExample::Deferred => {
                    ctx.defer_paint(move |painter, _| {
                        painter.with_buffer(|buffer| {
                            buffer.set_string(escaped.x, escaped.y, "DEFER", Style::default());
                        });
                    });
                }
            }
        }

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &State,
            _ctx: &mut EventCtx<'_>,
        ) -> EventResult<Msg> {
            match event {
                Event::Key(key) if key.code == KeyCode::Esc && !key.modifiers.any() => {
                    EventResult::Emit(Msg::Escape)
                }
                _ => EventResult::Ignored,
            }
        }
    }

    #[derive(Debug)]
    struct ModalProbe;

    impl Component<State, Msg> for ModalProbe {
        fn declare(&mut self, _ctx: &mut DeclareCtx<'_, State, Msg>) {}

        fn paint(&mut self, ctx: &mut crate::runtime::PaintCtx<'_, '_, State>) {
            ctx.widget(Paragraph::new("MODAL"), ctx.area());
        }

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &State,
            _ctx: &mut EventCtx<'_>,
        ) -> EventResult<Msg> {
            match event {
                Event::Key(key) if key.code == KeyCode::Esc && !key.modifiers.any() => {
                    EventResult::Emit(Msg::ModalClosed)
                }
                _ => EventResult::Ignored,
            }
        }

        fn is_focusable(&self, _state: &State) -> bool {
            true
        }
    }

    fn render_layer_example(driver: &mut Driver, state: &State, layer: LayerExample) {
        driver.render(state, move |ctx| {
            ctx.component(
                "scroll",
                scroll_area(8, move |ctx| {
                    let area = ctx.area();
                    ctx.component(
                        "host",
                        LayerHost { layer },
                        Rect::new(area.x, area.y + 3, 5, 1),
                    );
                }),
                Rect::new(0, 0, 6, 3),
            );
            ctx.paint_widget(Paragraph::new("XXXXX"), Rect::new(0, 3, 5, 1));
        });
    }

    fn declare_scroll_in_outer_layer(ctx: &mut DeclareCtx<'_, State, Msg>) {
        ctx.paint_widget(Paragraph::new("KEEP"), Rect::new(1, 4, 4, 1));
        ctx.component(
            "scroll",
            scroll_area(6, |ctx| {
                let area = ctx.area();
                ctx.paint_widget(Paragraph::new("zero\none\ntwo\nthree\nfour\nfive"), area);
                ctx.component(
                    "child",
                    Probe::focusable("layer-child"),
                    Rect::new(area.x, area.y + 2, area.width, 1),
                );
            }),
            Rect::new(1, 1, 7, 3),
        );
    }

    #[test]
    fn viewport_inside_popup_and_modal_uses_true_nesting_for_paint_clip_and_hits() {
        let state = State {
            offset: 2,
            ..State::default()
        };
        for modal in [false, true] {
            let mut driver = Driver::new(10, 6);
            driver.render(&state, move |ctx| {
                if modal {
                    ctx.modal_scope(
                        "outer",
                        Rect::new(0, 0, 10, 6),
                        ScopeOptions::default(),
                        declare_scroll_in_outer_layer,
                    );
                } else {
                    ctx.popup(
                        "outer",
                        PopupOptions::default(),
                        Rect::new(0, 0, 10, 6),
                        declare_scroll_in_outer_layer,
                    );
                }
            });

            assert!(driver.row(1).contains("layer"), "{}", driver.row(1));
            assert!(
                driver.row(4).contains("KEEP"),
                "viewport paint escaped its clip in the outer layer: {}",
                driver.row(4)
            );
            assert_eq!(
                driver.event(mouse(MouseKind::Click(MouseButton::Left), 2, 1), &state),
                EventResult::Emit(Msg::Pressed("layer-child")),
                "the visible logical child is hit inside the outer layer"
            );
        }
    }

    #[test]
    fn viewport_priming_preserves_earlier_enclosing_layer_cells() {
        for sparse in [false, true] {
            let mut driver = Driver::new(6, 4);
            let state = State::default();
            driver.render(&state, move |ctx| {
                ctx.popup(
                    "outer",
                    PopupOptions::default(),
                    Rect::new(0, 0, 6, 4),
                    move |ctx| {
                        ctx.paint_widget(
                            Paragraph::new("ABCDE\nFGHIJ").style(Style::new().bg(Color::Magenta)),
                            Rect::new(0, 1, 5, 2),
                        );
                        ctx.component(
                            "scroll",
                            scroll_area(2, move |ctx| {
                                if sparse {
                                    ctx.paint_widget(Paragraph::new("X"), Rect::new(2, 1, 1, 1));
                                }
                            }),
                            Rect::new(0, 1, 6, 2),
                        );
                    },
                );
            });

            assert_eq!(&driver.row(1)[..5], if sparse { "ABXDE" } else { "ABCDE" });
            assert_eq!(&driver.row(2)[..5], "FGHIJ");
            assert_eq!(
                driver
                    .terminal
                    .backend()
                    .buffer()
                    .cell((4, 2))
                    .expect("preserved layer cell")
                    .bg,
                Color::Magenta
            );
        }
    }

    #[test]
    fn sparse_viewport_in_a_layer_does_not_cover_lower_layer_cells() {
        for sparse in [false, true] {
            let mut driver = Driver::new(6, 4);
            let state = State::default();
            driver.render(&state, move |ctx| {
                ctx.paint_widget(
                    Paragraph::new("ABCDE\nFGHIJ").style(Style::new().bg(Color::Blue)),
                    Rect::new(0, 1, 5, 2),
                );
                ctx.popup(
                    "outer",
                    PopupOptions::default(),
                    Rect::new(0, 0, 6, 4),
                    move |ctx| {
                        ctx.component(
                            "scroll",
                            scroll_area(2, move |ctx| {
                                if sparse {
                                    ctx.paint_widget(Paragraph::new("X"), Rect::new(2, 1, 1, 1));
                                }
                            }),
                            Rect::new(0, 1, 6, 2),
                        );
                    },
                );
            });

            assert_eq!(&driver.row(1)[..5], if sparse { "ABXDE" } else { "ABCDE" });
            assert_eq!(&driver.row(2)[..5], "FGHIJ");
            assert_eq!(
                driver
                    .terminal
                    .backend()
                    .buffer()
                    .cell((4, 2))
                    .expect("preserved base cell")
                    .bg,
                Color::Blue
            );
        }
    }

    #[test]
    fn hints_popups_modals_and_deferred_paint_escape_and_project_once() {
        let state = State {
            offset: 2,
            ..State::default()
        };
        for (layer, expected) in [
            (LayerExample::Hint, "HINT"),
            (LayerExample::Popup, "item"),
            (LayerExample::Modal, "MODAL"),
            (LayerExample::Deferred, "DEFER"),
        ] {
            let mut driver = Driver::new(8, 6);
            render_layer_example(&mut driver, &state, layer);
            assert!(
                driver.row(3).contains(expected),
                "{layer:?} was clipped or translated more than once: {}",
                driver.row(3)
            );
        }
    }

    #[test]
    fn popup_and_modal_keep_existing_escape_routing() {
        let mut popup = Driver::new(8, 6);
        let popup_state = State {
            focus: FocusState::intent(["scroll", "host"]),
            offset: 2,
            ..State::default()
        };
        render_layer_example(&mut popup, &popup_state, LayerExample::Popup);
        assert_eq!(
            popup.event(key(KeyCode::Esc), &popup_state),
            EventResult::Emit(Msg::Escape),
            "Esc crosses a popup root to its declaring component"
        );

        let mut modal = Driver::new(8, 6);
        let modal_state = State {
            offset: 2,
            ..State::default()
        };
        render_layer_example(&mut modal, &modal_state, LayerExample::Modal);
        assert_eq!(
            modal.event(key(KeyCode::Esc), &modal_state),
            EventResult::Emit(Msg::ModalClosed),
            "the modal remains the key-routing floor"
        );
    }

    #[test]
    fn pointer_within_keeps_an_owner_open_while_the_pointer_is_in_its_escaped_popup() {
        let mut driver = Driver::new(10, 8);
        let state = State::default();
        let render = |driver: &mut Driver| {
            driver.render(&state, |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(8, |ctx| {
                        ctx.component("owner", HoverPopupHost, Rect::new(0, 3, 5, 1));
                    }),
                    Rect::new(0, 0, 7, 5),
                );
            });
        };

        render(&mut driver);
        driver.event(mouse(MouseKind::Moved, 1, 3), &state);
        render(&mut driver);
        assert!(driver.row(5).contains("popup"), "{}", driver.row(5));

        driver.event(mouse(MouseKind::Moved, 1, 5), &state);
        render(&mut driver);
        assert!(
            driver.row(5).contains("popup"),
            "the escaped popup remains in its owner's hovered subtree: {}",
            driver.row(5)
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 1, 5), &state),
            EventResult::Emit(Msg::Pressed("popup-item"))
        );
    }

    #[test]
    fn select_popup_inside_scrolled_content_inverse_projects_option_events() {
        let mut driver = Driver::new(16, 8);
        let state = State {
            offset: 3,
            select_open: true,
            select_cursor: Some("one"),
            ..State::default()
        };
        driver.render(&state, |ctx| {
            ctx.component(
                "scroll",
                scroll_area(8, |ctx| {
                    let area = ctx.area();
                    ctx.component(
                        "select",
                        Select::new([ListItem::new("one", "One"), ListItem::new("two", "Two")])
                            .open(|state: &State| state.select_open, Msg::SelectOpen)
                            .item_focus(|state: &State| state.select_cursor, Msg::SelectFocused)
                            .selection(|state: &State| state.selected, Msg::Selected),
                        Rect::new(area.x, area.y + 3, area.width, 1),
                    );
                }),
                Rect::new(2, 2, 11, 3),
            );
        });

        assert!(
            driver.row(3).contains("Two"),
            "the escaped panel paints in screen space: {}",
            driver.row(3)
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 4, 3), &state),
            EventResult::Emit(Msg::SelectFocused("two")),
            "option hover uses the popup's declaration-space row"
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 4, 3), &state),
            EventResult::Emit(Msg::Selected("two")),
            "option click uses the same inverse projection"
        );
    }

    /// A popup or hint whose anchor has scrolled out of sight is dropped even
    /// though its own rows would land on visible screen rows.
    #[test]
    fn popup_and_hint_anchors_are_dropped_when_the_anchor_is_offscreen() {
        let state = State::default();
        for layer in [LayerExample::Hint, LayerExample::Popup] {
            let escaped = Rect::new(0, 1, 5, 1);
            let mut visible = Driver::new(8, 6);
            visible.render(&state, move |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(8, move |ctx| {
                        ctx.component(
                            "host",
                            FixedLayerHost { layer, escaped },
                            Rect::new(0, 0, 5, 1),
                        );
                    }),
                    Rect::new(0, 0, 6, 3),
                );
            });
            assert!(
                (0..6).any(|row| visible.row(row).contains("LAYER")),
                "a visible {layer:?} anchor must keep its layer"
            );

            let mut offscreen = Driver::new(8, 6);
            offscreen.render(&state, move |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(8, move |ctx| {
                        ctx.component(
                            "host",
                            FixedLayerHost { layer, escaped },
                            Rect::new(0, 6, 5, 1),
                        );
                    }),
                    Rect::new(0, 0, 6, 3),
                );
            });
            assert!(
                (0..6).all(|row| !offscreen.row(row).contains("LAYER")),
                "offscreen {layer:?} anchor left an active layer on visible rows"
            );
        }
    }

    /// A layer host whose escaped rectangle does not follow its anchor, so the
    /// anchor can be offscreen while the layer would not be.
    #[derive(Debug)]
    struct FixedLayerHost {
        layer: LayerExample,
        escaped: Rect,
    }

    impl Component<State, Msg> for FixedLayerHost {
        fn declare(&mut self, ctx: &mut DeclareCtx<'_, State, Msg>) {
            let escaped = self.escaped;
            match self.layer {
                LayerExample::Hint => {
                    ctx.hint("layer", ScopeOptions::default(), escaped, move |ctx| {
                        ctx.paint_widget(Paragraph::new("LAYER"), escaped);
                    });
                }
                LayerExample::Popup => {
                    ctx.popup("layer", PopupOptions::default(), escaped, move |ctx| {
                        ctx.paint_widget(Paragraph::new("LAYER"), escaped);
                    });
                }
                LayerExample::Modal | LayerExample::Deferred => {}
            }
        }
    }

    #[test]
    fn nested_scroll_areas_are_rejected() {
        let mut driver = Driver::new(8, 4);
        let state = State::default();
        let result = catch_unwind(AssertUnwindSafe(|| {
            driver.render(&state, |ctx| {
                ctx.component(
                    "outer",
                    scroll_area(6, |ctx| {
                        ctx.component("inner", scroll_area(5, |_| {}), Rect::new(0, 0, 6, 3));
                    }),
                    Rect::new(0, 0, 8, 4),
                );
            });
        }));

        let panic = result.expect_err("nested ScrollAreas must panic");
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or_default();
        assert!(
            message.contains("cannot be declared inside another viewport"),
            "{message}"
        );
    }

    #[derive(Debug)]
    struct HoverCaptureProbe {
        hovered: Rc<Cell<bool>>,
    }

    impl Component<State, Msg> for HoverCaptureProbe {
        fn declare(&mut self, _ctx: &mut DeclareCtx<'_, State, Msg>) {}

        fn paint(&mut self, ctx: &mut crate::runtime::PaintCtx<'_, '_, State>) {
            self.hovered.set(ctx.hovered);
        }

        fn handle_event(
            &mut self,
            event: &Event,
            _state: &State,
            ctx: &mut EventCtx<'_>,
        ) -> EventResult<Msg> {
            match event {
                Event::Mouse(mouse) if mouse.kind == MouseKind::Down(MouseButton::Left) => {
                    ctx.capture_pointer(MouseButton::Left);
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            }
        }
    }

    #[test]
    fn scrolling_a_captured_descendant_offscreen_clears_its_hover() {
        let mut driver = Driver::new(8, 4);
        let mut state = State::default();
        let hovered = Rc::new(Cell::new(false));
        let render = |driver: &mut Driver, state: &State, hovered: &Rc<Cell<bool>>| {
            let hovered = Rc::clone(hovered);
            driver.render(state, move |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(8, move |ctx| {
                        ctx.component(
                            "capture",
                            HoverCaptureProbe { hovered },
                            Rect::new(0, 1, 7, 1),
                        );
                    }),
                    Rect::new(0, 0, 8, 3),
                );
            });
        };

        render(&mut driver, &state, &hovered);
        driver.event(mouse(MouseKind::Moved, 1, 1), &state);
        render(&mut driver, &state, &hovered);
        assert!(hovered.get());
        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state);

        state.offset = 4;
        render(&mut driver, &state, &hovered);
        assert!(
            !hovered.get(),
            "capture preserves routing, but not hover for an offscreen node"
        );
    }

    #[test]
    fn wheel_scrolling_a_tooltip_trigger_away_closes_it_on_the_resulting_redraw() {
        let mut driver = Driver::new(12, 8);
        let mut state = State::default();
        let render = |driver: &mut Driver, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(9, |ctx| {
                        ctx.component(
                            "tip",
                            Tooltip::new("TIP").trigger(|ctx| {
                                ctx.paint_widget(Paragraph::new("TRIG"), ctx.area());
                            }),
                            Rect::new(0, 3, 5, 1),
                        );
                    }),
                    Rect::new(0, 0, 8, 5),
                );
            });
        };

        render(&mut driver, &state);
        driver.event(mouse(MouseKind::Moved, 1, 3), &state);
        render(&mut driver, &state);
        assert!((0..8).any(|row| driver.row(row).contains("TIP")));

        let EventResult::Emit(Msg::Area(ScrollAreaChange::ScrollTo(offset))) = driver.event(
            mouse(MouseKind::Scroll(ScrollDirection::Down), 1, 3),
            &state,
        ) else {
            panic!("the wheel must bubble through the Tooltip to ScrollArea");
        };
        assert_eq!(offset, 3);
        state.offset = offset;
        render(&mut driver, &state);
        assert!(
            (0..8).all(|row| !driver.row(row).contains("TIP")),
            "the wheel redraw must use the trigger's current projected geometry"
        );
        assert!(
            driver.row(0).contains("TRIG"),
            "the trigger remains visible"
        );
    }
}
