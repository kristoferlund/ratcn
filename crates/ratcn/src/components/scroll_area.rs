//! A vertical viewport for arbitrary interactive ratcn descendants.
//!
//! Descendants are declared against their full logical content allocations.
//! The runtime translates and clips ordinary paint and pointer input without
//! changing those allocations, while keeping offscreen descendants in focus
//! traversal. Popup, hint, modal, and deferred paint escape the ordinary clip.

use ratatui::{
    layout::{Position, Rect},
    style::{Color, Style},
    widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::Theme;
use crate::runtime::{
    CellOffset, Component, DeclareCtx, DragOptions, DragPhase, Event, EventCtx, EventResult,
    KeyCode, MouseButton, MouseEvent, MouseKind, ScopeOptions, ScrollDirection,
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
    /// Derive the thumb and track from `theme`.
    #[must_use]
    pub const fn from_theme(theme: &Theme) -> Self {
        Self {
            thumb: theme.primary,
            track: theme.border,
        }
    }
}

/// Where the wheel, a key, a gutter drag, or a reveal left the view.
///
/// Event handling writes it and the next declaration reads it, which is what
/// lets an unbound area scroll at all and what carries a reveal into the
/// declaration that follows it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum ScrollHold {
    /// Nothing is holding the view: the bound offset decides where it sits.
    #[default]
    Released,
    /// The view is held at `offset`. `base` is the bound offset the hold was
    /// taken against, and it is what makes releasing permanent: the
    /// declaration drops the hold for good the moment a bound offset moves
    /// away from `base`, so an app that scrolls its own area keeps it, and
    /// returning to the offset the hold was taken at cannot revive it.
    Held { offset: u16, base: Option<u16> },
}

type ReadOffsetFn<S> = Box<dyn Fn(&S) -> u16>;
type OnChangeFn<M> = Box<dyn Fn(u16) -> M>;
type ContentFn<S, M> = Box<dyn FnOnce(&mut DeclareCtx<'_, S, M>)>;
type StyleFn = Box<dyn Fn(&Theme) -> ScrollAreaStyle>;

/// A vertical viewport that hosts arbitrary interactive ratcn descendants.
///
/// One column is reserved at the right for the scrollbar. The
/// [`content`](Self::content) closure receives the remaining width and exactly
/// `content_height` logical rows, however many of them are visible. Press the
/// thumb to drag it from the grabbed point; press the track to jump the view.
///
/// The offset is the area's own until [`scroll`](Self::scroll) binds it. Wheel,
/// Page Up, Page Down, Home, End, and a press on the scrollbar gutter reach
/// descendants first; what none of them handles scrolls the area. An event that
/// leaves the offset where it is — every one of these keys at an edge, and a
/// horizontal wheel — bubbles on to the app, which keeps app hotkeys on those
/// keys alive. A gutter press that does not move the view still consumes, so
/// it cannot steal focus into a descendant. The area is a fallback focus stop
/// while it holds no focusable descendant, so keyboard scrolling works for
/// paint-only content.
///
/// Focus moving to a descendant the viewport clips scrolls that descendant
/// into view.
///
/// # Panics
///
/// A `ScrollArea` inside another `ScrollArea` panics, as does content larger
/// than 262,144 cells, as does a single paint inside one covering more than
/// that.
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
    pub fn scroll(
        mut self,
        read: impl Fn(&S) -> u16 + 'static,
        on_change: impl Fn(u16) -> M + 'static,
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
    fn resolve(&self, area: Rect, bound: Option<u16>, hold: ScrollHold) -> u16 {
        let offset = match hold {
            ScrollHold::Held { offset, base } if base == bound => offset,
            _ => bound.unwrap_or(0),
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
        let hold = ctx.transient_mut::<ScrollHold>();
        if matches!(*hold, ScrollHold::Held { base, .. } if base != bound) {
            *hold = ScrollHold::Released;
        }
        self.resolve(area, bound, *hold)
    }

    /// The offset in force at event time.
    fn current(&self, state: &S, ctx: &mut EventCtx<'_>) -> u16 {
        let hold = *ctx.transient::<ScrollHold>();
        self.resolve(ctx.area(), self.bound_offset(state), hold)
    }

    /// Hold the view at `offset` for the coming declaration, and report the
    /// offset taken. `None` when the view is already there.
    fn hold(&self, offset: u16, state: &S, ctx: &mut EventCtx<'_>) -> Option<u16> {
        let area = ctx.area();
        let current = self.current(state, ctx);
        let offset = offset.min(self.max_offset(area));
        if offset == current {
            return None;
        }
        *ctx.transient::<ScrollHold>() = ScrollHold::Held {
            offset,
            base: self.bound_offset(state),
        };
        Some(offset)
    }

    /// The right-hand gutter column, empty when the area has no width.
    fn gutter(area: Rect) -> Rect {
        if area.width == 0 {
            Rect::ZERO
        } else {
            Rect::new(area.right().saturating_sub(1), area.y, 1, area.height)
        }
    }

    /// Whether `position` is a cell of the scrollbar gutter.
    fn gutter_contains(area: Rect, position: Position) -> bool {
        Self::gutter(area).contains(position)
    }

    /// Ratatui's nearest-integer division: `(n + d/2) / d`.
    const fn rounding_divide(numerator: u32, denominator: u32) -> u32 {
        (numerator + denominator / 2) / denominator
    }

    /// Thumb start and length in rows from `area.y`, matching the painted
    /// Ratatui scrollbar so a press can tell the thumb from the track.
    fn thumb_span(&self, area: Rect, offset: u16) -> Option<(u16, u16)> {
        let track = area.height;
        let max_offset = self.max_offset(area);
        let viewport = Self::viewport(area).height;
        if track == 0 || max_offset == 0 {
            return None;
        }
        let max_viewport = u32::from(max_offset).saturating_add(u32::from(viewport));
        if max_viewport == 0 {
            return None;
        }
        let thumb_len = Self::rounding_divide(u32::from(viewport) * u32::from(track), max_viewport)
            .clamp(1, u32::from(track));
        let thumb_len = u16::try_from(thumb_len).unwrap_or(track);
        let start = Self::rounding_divide(u32::from(offset) * u32::from(track), max_viewport)
            .min(u32::from(track.saturating_sub(thumb_len)));
        let start = u16::try_from(start).unwrap_or(0);
        Some((start, thumb_len))
    }

    /// Whether `row` is a cell of the thumb painted at `offset`.
    fn thumb_contains(&self, area: Rect, offset: u16, row: u16) -> bool {
        let Some((start, len)) = self.thumb_span(area, offset) else {
            return false;
        };
        row.checked_sub(area.y)
            .is_some_and(|y| y >= start && y < start.saturating_add(len))
    }

    /// The first thumb row painted at `offset`, relative to the track.
    fn thumb_start(&self, area: Rect, offset: u16) -> Option<u16> {
        self.thumb_span(area, offset).map(|(start, _)| start)
    }

    /// The offset whose thumb is painted from `start` rows into the track:
    /// [`thumb_span`](Self::thumb_span)'s placement run backwards, so the thumb
    /// lands on exactly that row. Starts past either end clamp. `None` when
    /// the thumb fills the track and cannot move.
    fn offset_for_thumb_start(&self, area: Rect, start: i16) -> Option<u16> {
        let max_offset = self.max_offset(area);
        let (_, thumb_len) = self.thumb_span(area, 0)?;
        let movable = area.height.saturating_sub(thumb_len);
        if movable == 0 {
            return None;
        }
        let start = u16::try_from(start.max(0)).unwrap_or(0);
        if start >= movable {
            return Some(max_offset);
        }
        let max_viewport = u32::from(max_offset) + u32::from(Self::viewport(area).height);
        let mapped = Self::rounding_divide(u32::from(start) * max_viewport, u32::from(area.height));
        Some(u16::try_from(mapped).unwrap_or(max_offset).min(max_offset))
    }

    /// Treat an unchanged offset as handled: a gutter press that does not
    /// move the view still owns the gesture, so focus-on-press cannot steal it.
    fn consume(result: EventResult<M>) -> EventResult<M> {
        match result {
            EventResult::Ignored => EventResult::Consumed,
            other => other,
        }
    }

    /// Drag the thumb from the cell it was grabbed by; a press on the track
    /// first jumps the thumb's top to that row.
    ///
    /// [`EventCtx::drag`] owns the gesture, anchored at the thumb's top row,
    /// so a thumb press holds still until the pointer moves.
    fn handle_gutter_drag(
        &self,
        mouse: &MouseEvent,
        state: &S,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<M> {
        let area = ctx.area();
        let can_start = self.max_offset(area) > 0
            && Self::gutter_contains(area, Position::new(mouse.column, mouse.row));
        let mut jumped = EventResult::Ignored;
        let mut anchor = CellOffset::default();
        if can_start && mouse.kind == MouseKind::Down(MouseButton::Left) {
            let current = self.current(state, ctx);
            if !self.thumb_contains(area, current, mouse.row) {
                let row = i16::try_from(mouse.row.saturating_sub(area.y)).unwrap_or(i16::MAX);
                if let Some(target) = self.offset_for_thumb_start(area, row) {
                    jumped = self.scroll_to(target, state, ctx);
                }
            }
            let current = self.current(state, ctx);
            let start = self.thumb_start(area, current).unwrap_or(0);
            anchor.y = i16::try_from(start).unwrap_or(i16::MAX);
        }
        match ctx.drag(mouse, DragOptions::new(anchor).start_if(can_start)) {
            DragPhase::Down => Self::consume(jumped),
            DragPhase::Moved { offset, .. } => {
                // Only a new thumb row moves the view. Remapping the row the
                // thumb already sits on would snap an offset the wheel left
                // between rows.
                let painted = self.thumb_start(area, self.current(state, ctx));
                match self.offset_for_thumb_start(area, offset.y) {
                    Some(target) if self.thumb_start(area, target) != painted => {
                        Self::consume(self.scroll_to(target, state, ctx))
                    }
                    _ => EventResult::Consumed,
                }
            }
            DragPhase::Ended { .. } => EventResult::Consumed,
            DragPhase::Ignored => EventResult::Ignored,
        }
    }

    /// Scroll to `offset`.
    ///
    /// [`EventResult::Ignored`] when the view is already there, which leaves an
    /// app hotkey on Home, End, or a page key working while focus rests in an
    /// area with nothing to scroll.
    fn scroll_to(&self, offset: u16, state: &S, ctx: &mut EventCtx<'_>) -> EventResult<M> {
        let Some(offset) = self.hold(offset, state, ctx) else {
            return EventResult::Ignored;
        };
        match &self.on_change {
            Some(on_change) => EventResult::Emit(on_change(offset)),
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
        let gutter = Self::gutter(area);
        let style = resolve_style(
            self.style.as_deref(),
            ctx.theme,
            ScrollAreaStyle::from_theme,
        );
        let viewport_height = viewport.height;
        // Scrollbar positions are row offsets, so the count is the inclusive
        // offset range, not the total row count.
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
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseKind::Scroll(ScrollDirection::Up) => {
                    let current = self.current(state, ctx);
                    self.scroll_to(current.saturating_sub(WHEEL_ROWS), state, ctx)
                }
                MouseKind::Scroll(ScrollDirection::Down) => {
                    let current = self.current(state, ctx);
                    self.scroll_to(current.saturating_add(WHEEL_ROWS), state, ctx)
                }
                _ => self.handle_gutter_drag(mouse, state, ctx),
            },
            Event::Key(key) if !key.modifiers.any() => {
                let area = ctx.area();
                let current = self.current(state, ctx);
                let page = Self::viewport(area).height;
                let target = match key.code {
                    KeyCode::PageUp => current.saturating_sub(page),
                    KeyCode::PageDown => current.saturating_add(page),
                    KeyCode::Home => 0,
                    KeyCode::End => self.max_offset(area),
                    _ => return EventResult::Ignored,
                };
                self.scroll_to(target, state, ctx)
            }
            _ => EventResult::Ignored,
        }
    }

    fn reveal_in_viewport(&mut self, target: Rect, state: &S, ctx: &mut EventCtx<'_>) -> bool {
        let area = ctx.area();
        let current = self.current(state, ctx);
        self.hold(self.reveal_offset(target, area, current), state, ctx)
            .is_some()
    }

    fn scope_options(&self) -> ScopeOptions {
        let options = ScopeOptions::default().focusable(true);
        if self.hover_focus {
            options.hover_focus()
        } else {
            options
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use ratatui::{
        text::Text,
        widgets::{Block, Paragraph},
    };

    use super::*;
    use crate::runtime::{ChildId, FocusState, KeyChord, Modifiers, PopupOptions, Ratcn};
    use crate::test_support::{Driver, key, key_with, mouse};
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
        Area(u16),
        Focus(FocusState),
        Pressed(&'static str),
        ChildHandled,
        SelectOpen(bool),
        SelectFocused(&'static str),
        Selected(&'static str),
        Escape,
        ModalClosed,
    }

    /// A driver whose focus lives in the test state.
    fn driver(width: u16, height: u16) -> Driver<State, Msg> {
        Driver::with(
            Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
            width,
            height,
        )
    }

    fn scroll_area(
        content_height: u16,
        content: impl FnOnce(&mut DeclareCtx<'_, State, Msg>) + 'static,
    ) -> ScrollArea<State, Msg> {
        ScrollArea::new(content_height)
            .scroll(|state: &State| state.offset, Msg::Area)
            .content(content)
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

        fn paint(&mut self, ctx: &mut crate::runtime::PaintCtx<'_, State>) {
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

        fn scope_options(&self) -> ScopeOptions {
            ScopeOptions::default().focusable(self.focusable)
        }
    }

    #[test]
    fn ordinary_paint_uses_full_logical_allocation_then_translates_and_clips() {
        let mut driver = driver(8, 5);
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
            let mut driver = driver(6, 2);
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
        let mut driver = driver(8, 4);
        // No focus: a focused button would be revealed, moving the offset.
        let state = State {
            focus: FocusState::none(),
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
        let mut driver = driver(8, 5);
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
        let mut driver = driver(8, 3);
        let mut state = State {
            focus: FocusState::intent(["scroll", "first"]),
            ..State::default()
        };
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
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
        let mut driver = driver(8, 3);
        let mut state = State {
            focus: FocusState::intent(["scroll", "first"]),
            ..State::default()
        };
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
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
        let mut driver = driver(8, 3);
        let mut state = State {
            focus: FocusState::intent(["scroll", "first"]),
            ..State::default()
        };
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
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

    /// The render closure runs a second time only to rebuild the tree at the
    /// offset a reveal moved to — a reveal that leaves the offset where it was
    /// has nothing for a second run to change, and costs one run.
    #[test]
    fn the_frame_is_declared_again_only_when_the_reveal_scrolls() {
        let mut driver = driver(8, 3);
        let mut state = State {
            focus: FocusState::none(),
            ..State::default()
        };
        let runs = Cell::new(0);
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            runs.set(0);
            driver.render(state, |ctx| {
                runs.set(runs.get() + 1);
                ctx.component(
                    "scroll",
                    ScrollArea::new(9).content(|ctx| {
                        // Taller than the viewport and already at its top:
                        // clipped, yet as revealed as it can be.
                        ctx.component("tall", Probe::focusable("tall"), Rect::new(0, 0, 7, 5));
                        ctx.component("last", Probe::focusable("last"), Rect::new(0, 6, 7, 1));
                    }),
                    Rect::new(0, 0, 8, 3),
                );
            });
            runs.get()
        };
        render(&mut driver, &state);

        state.focus = FocusState::intent(["scroll", "tall"]);
        assert_eq!(render(&mut driver, &state), 1, "top-aligned: nothing moved");
        assert_eq!(&driver.row(0)[..4], "tall");

        state.focus = FocusState::intent(["scroll", "last"]);
        assert_eq!(render(&mut driver, &state), 2, "the reveal scrolled");
        assert_eq!(&driver.row(2)[..4], "last");
    }

    /// A row appended and focused by one update is on screen in the frame
    /// that first declares it: an on-demand host draws that one frame and
    /// then waits for input, so there is no later frame to finish the job.
    #[test]
    fn a_row_declared_and_focused_in_one_update_is_revealed_by_that_frame() {
        let mut driver = driver(8, 3);
        let mut state = State {
            focus: FocusState::intent(["scroll", "first"]),
            ..State::default()
        };
        let render = |driver: &mut Driver<State, Msg>, state: &State, appended: bool| {
            driver.render(state, |ctx| {
                ctx.component(
                    "scroll",
                    ScrollArea::new(9).content(move |ctx| {
                        ctx.component("first", Probe::focusable("first"), Rect::new(0, 0, 7, 1));
                        if appended {
                            ctx.component("new", Probe::focusable("new"), Rect::new(0, 6, 7, 1));
                        }
                    }),
                    Rect::new(0, 0, 8, 3),
                );
            });
        };
        render(&mut driver, &state, false);
        assert_eq!(&driver.row(0)[..5], "first");

        state.focus = FocusState::intent(["scroll", "new"]);
        render(&mut driver, &state, true);
        assert_eq!(&driver.row(2)[..3], "new", "revealed without another frame");
    }

    #[test]
    fn backtab_and_focus_keys_minimally_reveal_their_destination() {
        let mut driver = driver(8, 3);
        driver.ratcn = std::mem::take(&mut driver.ratcn)
            .focus_key(KeyChord::from('l').alt(), ["scroll", "last"]);
        let mut state = State {
            focus: FocusState::intent(["scroll", "last"]),
            offset: 4,
            ..State::default()
        };
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
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

        let jump = key_with(
            KeyCode::Char('l'),
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        );
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
        let mut driver = driver(8, 3);
        driver.ratcn = std::mem::take(&mut driver.ratcn)
            .focus_key(KeyChord::from('l').alt(), ["scroll", "last"]);
        let mut state = State {
            focus: FocusState::intent(["scroll", "last"]),
            ..State::default()
        };
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
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
        render(&mut driver, &state);
        assert_eq!(
            &driver.row(2)[..4],
            "last",
            "the stored focus reached its row"
        );

        // The app scrolls its own offset away from the focused row, which
        // releases the reveal's hold. Focus has not moved.
        state.offset = 1;
        render(&mut driver, &state);
        assert_ne!(&driver.row(2)[..4], "last");

        assert_eq!(
            driver.event(
                key_with(
                    KeyCode::Char('l'),
                    Modifiers {
                        alt: true,
                        ..Modifiers::NONE
                    },
                ),
                &state,
            ),
            EventResult::Consumed,
            "focus did not change, so there is no focus message to send"
        );
        render(&mut driver, &state);
        assert_eq!(
            &driver.row(2)[..4],
            "last",
            "asking for a control by name is a request to see it"
        );
    }

    #[test]
    fn wrapped_traversal_reveals_its_same_offscreen_target() {
        let mut driver = driver(8, 3);
        let mut state = State {
            focus: FocusState::intent(["scroll", "wrap", "only"]),
            ..State::default()
        };
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
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
        render(&mut driver, &state);
        assert_eq!(&driver.row(2)[..4], "only");

        state.offset = 1;
        render(&mut driver, &state);
        assert_ne!(&driver.row(2)[..4], "only");

        assert_eq!(
            driver.event(key(KeyCode::Tab), &state),
            EventResult::Consumed,
            "the wrap landed back where it started, so focus did not change"
        );
        render(&mut driver, &state);
        assert_eq!(
            &driver.row(2)[..4],
            "only",
            "the wrap still asked to see it"
        );
    }

    #[test]
    fn repeated_controlled_events_read_current_app_offset_before_redraw() {
        let mut driver = driver(8, 3);
        let mut state = State::default();
        driver.render(&state, |ctx| {
            ctx.component("scroll", scroll_area(12, |_| {}), Rect::new(0, 0, 8, 3));
        });

        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 1, 1),
                &state
            ),
            EventResult::Emit(Msg::Area(3))
        );
        state.offset = 3;
        assert_eq!(
            driver.event(
                mouse(MouseKind::Scroll(ScrollDirection::Down), 1, 1),
                &state
            ),
            EventResult::Emit(Msg::Area(6))
        );
        state.offset = 6;
        assert_eq!(
            driver.event(key(KeyCode::PageDown), &state),
            EventResult::Emit(Msg::Area(9))
        );
    }

    /// An unbound area owns its offset the same way an unbound `List` owns
    /// its scroll: the wheel moves the view and the offset survives the
    /// redraw, with nothing emitted.
    #[test]
    fn an_unbound_area_scrolls_itself_on_the_wheel() {
        let mut driver = driver(8, 3);
        let state = State::default();
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
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
        let mut driver = driver(8, 3);
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
            EventResult::Emit(Msg::Area(0))
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
                key_with(
                    KeyCode::End,
                    Modifiers {
                        ctrl: true,
                        ..Modifiers::NONE
                    },
                ),
                &state
            ),
            EventResult::Ignored
        );
    }

    /// The same keys in an area that has nothing to scroll.
    #[test]
    fn an_area_that_cannot_scroll_leaves_its_keys_to_the_app() {
        let mut driver = driver(8, 5);
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
        let mut driver = driver(8, 3);
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
        let mut driver = driver(6, 4);
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
        let mut driver = driver(6, 4);
        let mut state = State::default();
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
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
        let thumb_rows = |driver: &Driver<State, Msg>| {
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
        let mut driver = driver(6, 4);
        let mut state = State::default();
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
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
        let thumb_rows = |driver: &Driver<State, Msg>| {
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

    /// The gutter is a handle, not decoration: a press on the track jumps the
    /// view, and capture keeps the drag even after the pointer leaves the column.
    #[test]
    fn dragging_the_scrollbar_track_scrolls_the_area() {
        let mut driver = driver(6, 4);
        let mut state = State::default();
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component("scroll", scroll_area(12, |_| {}), Rect::new(0, 0, 6, 4));
            });
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 5, 3), &state),
            EventResult::Emit(Msg::Area(8)),
            "a press on the track jumps so the thumb follows the pointer row"
        );
        state.offset = 8;

        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 5, 0), &state),
            EventResult::Emit(Msg::Area(0)),
            "a captured gutter drag keeps routing after leaving the track"
        );
        state.offset = 0;

        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 0, 3), &state),
            EventResult::Emit(Msg::Area(8)),
            "capture, not hit-testing, owns the gutter drag"
        );

        assert_eq!(
            driver.event(mouse(MouseKind::Up(MouseButton::Left), 0, 3), &state),
            EventResult::Consumed
        );
    }

    /// Pressing the thumb must not map that row onto a new offset — the thumb
    /// would jump under the pointer. Movement then keeps the grabbed cell.
    #[test]
    fn pressing_the_thumb_anchors_until_the_pointer_moves() {
        let mut driver = driver(6, 4);
        let state = State::default();
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    "scroll",
                    scroll_area(6, |_| {}).style(|_| ScrollAreaStyle {
                        thumb: Color::Yellow,
                        track: Color::Blue,
                    }),
                    Rect::new(0, 0, 6, 4),
                );
            });
        };
        render(&mut driver, &state);
        let thumb: Vec<u16> = (0..4)
            .filter(|&row| {
                driver
                    .terminal
                    .backend()
                    .buffer()
                    .cell((5, row))
                    .is_some_and(|cell| cell.fg == Color::Yellow)
            })
            .collect();
        assert!(
            thumb.contains(&2),
            "the fixture needs a multi-row thumb covering row 2, got {thumb:?}"
        );
        assert!(
            !thumb.contains(&3),
            "row 3 must be track so a later press can still jump, got {thumb:?}"
        );

        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 5, 2), &state),
            EventResult::Consumed,
            "a press on the thumb must not jump the offset"
        );
        assert_eq!(state.offset, 0);

        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 5, 1), &state),
            EventResult::Consumed,
            "moving within the grabbed thumb must not jump as a track click would"
        );
        assert_eq!(state.offset, 0);

        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 5, 3), &state),
            EventResult::Emit(Msg::Area(2)),
            "dragging keeps the grabbed thumb cell under the pointer"
        );
    }

    /// Every row the thumb can occupy maps to an offset painted on that same
    /// row, and moving the thumb down never scrolls the view up. Otherwise a
    /// drag runs backwards, skips rows, or leaves the thumb off the pointer.
    #[test]
    fn every_thumb_row_maps_to_an_offset_painted_on_that_row() {
        for height in 2..=40 {
            let area = Rect::new(0, 0, 2, height);
            for content in height + 1..=300 {
                let scroll = ScrollArea::<State, Msg>::new(content);
                let (_, thumb_len) = scroll.thumb_span(area, 0).expect("content overflows");
                let movable = height - thumb_len;
                let mut previous = 0;
                for start in 0..=movable {
                    let row = i16::try_from(start).expect("small track");
                    let Some(offset) = scroll.offset_for_thumb_start(area, row) else {
                        assert_eq!(movable, 0, "only a thumb filling the track cannot move");
                        continue;
                    };
                    assert_eq!(
                        scroll.thumb_start(area, offset),
                        Some(start),
                        "height {height}, content {content}: row {start} maps to {offset}"
                    );
                    assert!(offset >= previous, "height {height}, content {content}");
                    previous = offset;
                }
            }
        }
    }

    /// The thumb rows painted in the gutter column of a 2-wide area.
    fn painted_thumb_rows(driver: &Driver<State, Msg>, height: u16) -> Vec<u16> {
        (0..height)
            .filter(|&row| {
                driver
                    .terminal
                    .backend()
                    .buffer()
                    .cell((1, row))
                    .is_some_and(|cell| cell.fg == Color::Yellow)
            })
            .collect()
    }

    fn render_thumb_fixture(driver: &mut Driver<State, Msg>, state: &State, content: u16) {
        let area = driver.area();
        driver.render(state, |ctx| {
            ctx.component(
                "scroll",
                scroll_area(content, |_| {}).style(|_| ScrollAreaStyle {
                    thumb: Color::Yellow,
                    track: Color::Blue,
                }),
                area,
            );
        });
    }

    /// Dragging a one-row thumb up one row must scroll up and paint the thumb
    /// on that row, even where the thumb's length is clamped to one cell.
    #[test]
    fn dragging_a_short_thumb_up_one_row_scrolls_up_to_that_row() {
        let mut driver = driver(2, 10);
        let mut state = State {
            offset: 210,
            ..State::default()
        };
        render_thumb_fixture(&mut driver, &state, 247);
        assert_eq!(painted_thumb_rows(&driver, 10), [9]);

        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 9), &state);
        let EventResult::Emit(Msg::Area(offset)) =
            driver.event(mouse(MouseKind::Moved, 1, 8), &state)
        else {
            panic!("moving the thumb a row must scroll");
        };
        assert!(offset < 210, "the view must scroll up, not to {offset}");
        state.offset = offset;
        render_thumb_fixture(&mut driver, &state, 247);
        assert_eq!(painted_thumb_rows(&driver, 10), [8]);
    }

    /// Moving along the thumb's row leaves the view alone, even at an offset
    /// the wheel left between the offsets two thumb rows stand for.
    #[test]
    fn moving_sideways_on_the_thumb_keeps_the_offset() {
        let mut driver = driver(2, 20);
        let state = State {
            offset: 13,
            ..State::default()
        };
        render_thumb_fixture(&mut driver, &state, 60);
        let thumb = painted_thumb_rows(&driver, 20);
        assert_eq!(thumb.first(), Some(&4), "fixture thumb starts at row 4");

        driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 4), &state);
        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 0, 4), &state),
            EventResult::Consumed,
            "no vertical travel, so no scroll"
        );
    }

    /// A track press puts the thumb's top on the pressed row, so the drag
    /// that follows keeps the thumb under the pointer.
    #[test]
    fn a_track_press_puts_the_thumb_on_the_pressed_row() {
        let mut driver = driver(2, 10);
        let mut state = State::default();
        render_thumb_fixture(&mut driver, &state, 220);

        let EventResult::Emit(Msg::Area(offset)) =
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 8), &state)
        else {
            panic!("a track press must jump");
        };
        state.offset = offset;
        render_thumb_fixture(&mut driver, &state, 220);
        assert_eq!(painted_thumb_rows(&driver, 10), [8]);
    }

    /// A gutter press that does not change the offset still owns the event, so
    /// focus-on-press cannot land on a descendant sitting in the same rows.
    #[test]
    fn a_gutter_press_does_not_move_focus_into_a_descendant() {
        let mut driver = driver(8, 3);
        let state = State::default();
        driver.render(&state, |ctx| {
            ctx.component(
                "scroll",
                scroll_area(9, |ctx| {
                    ctx.component("first", Probe::focusable("first"), Rect::new(0, 0, 7, 1));
                    ctx.component("last", Probe::focusable("last"), Rect::new(0, 6, 7, 1));
                }),
                Rect::new(0, 0, 8, 3),
            );
        });

        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 7, 0), &state),
            EventResult::Consumed,
            "a gutter press at the current offset still owns the event"
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Up(MouseButton::Left), 7, 0), &state),
            EventResult::Consumed
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 7, 2), &state),
            EventResult::Emit(Msg::Area(6)),
            "a gutter press must not emit a focus change"
        );
    }

    /// An unbound area owns the gutter drag the same way it owns the wheel:
    /// the press moves the view and nothing is emitted.
    #[test]
    fn an_unbound_area_drags_its_scrollbar_without_emitting() {
        let mut driver = driver(8, 3);
        let state = State::default();
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
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
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 7, 2), &state),
            EventResult::Consumed,
            "there is no offset binding, so nothing is emitted"
        );
        render(&mut driver, &state);
        assert_eq!(
            &driver.row(0)[..1],
            "g",
            "the gutter press moved the view by itself"
        );

        driver.event(mouse(MouseKind::Up(MouseButton::Left), 7, 2), &state);
        render(&mut driver, &state);
        assert_eq!(
            &driver.row(0)[..1],
            "g",
            "ending the drag must not drop the hold that placed the view"
        );
    }

    /// A one-row track cannot express a range. Jumping to offset 0 would
    /// discard whatever the wheel or a reveal had already placed there.
    #[test]
    fn a_one_row_gutter_press_leaves_the_offset_where_it_is() {
        let mut driver = driver(6, 1);
        let state = State {
            offset: 4,
            ..State::default()
        };
        driver.render(&state, |ctx| {
            ctx.component("scroll", scroll_area(12, |_| {}), Rect::new(0, 0, 6, 1));
        });

        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 5, 0), &state),
            EventResult::Consumed,
            "a one-row gutter still owns the press"
        );
        assert_eq!(state.offset, 4, "the press must not jump the offset to 0");
    }

    /// A descendant that captured the pointer can bubble its drag. The area
    /// must not treat that as its own gutter gesture.
    #[test]
    fn a_captured_descendant_drag_does_not_scroll_the_area() {
        let mut driver = driver(8, 3);
        let state = State::default();
        driver.render(&state, |ctx| {
            ctx.component(
                "scroll",
                scroll_area(9, |ctx| {
                    ctx.component("child", CapturingLeaf, Rect::new(0, 1, 7, 1));
                }),
                Rect::new(0, 0, 8, 3),
            );
        });

        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
            EventResult::Consumed
        );
        let dragged = driver.event(mouse(MouseKind::Moved, 1, 2), &state);
        assert!(
            !matches!(dragged, EventResult::Emit(Msg::Area(_))),
            "a bubbled captured drag must not scroll the area, got {dragged:?}"
        );
        assert_eq!(state.offset, 0);
    }

    /// A gutter drag cut off without its release leaves the area's drag state
    /// behind. A descendant's captured drag that bubbles later must still not
    /// continue it: only the capture owner sees the captured press.
    #[test]
    fn a_descendant_drag_does_not_revive_an_abandoned_gutter_drag() {
        let mut driver = driver(8, 3);
        let state = State::default();
        driver.render(&state, |ctx| {
            ctx.component(
                "scroll",
                scroll_area(9, |ctx| {
                    ctx.component("child", CapturingLeaf, Rect::new(0, 1, 7, 1));
                }),
                Rect::new(0, 0, 8, 3),
            );
        });

        driver.event(mouse(MouseKind::Down(MouseButton::Left), 7, 0), &state);
        driver.event(mouse(MouseKind::Exited, 7, 0), &state);
        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
            EventResult::Consumed
        );
        let dragged = driver.event(mouse(MouseKind::Moved, 1, 2), &state);
        assert!(
            !matches!(dragged, EventResult::Emit(Msg::Area(_))),
            "a descendant's drag must not continue the abandoned gutter drag, got {dragged:?}"
        );
    }

    /// A press on the content still focuses a descendant. The gutter is the
    /// handle; the rest of the area is not.
    #[test]
    fn a_content_press_still_focuses_a_descendant() {
        let mut driver = driver(8, 3);
        let state = State {
            focus: FocusState::intent(["scroll", "first"]),
            ..State::default()
        };
        driver.render(&state, |ctx| {
            ctx.component(
                "scroll",
                scroll_area(9, |ctx| {
                    ctx.component("first", Probe::focusable("first"), Rect::new(0, 0, 7, 1));
                    ctx.component("second", Probe::focusable("second"), Rect::new(0, 1, 7, 1));
                }),
                Rect::new(0, 0, 8, 3),
            );
        });

        assert_eq!(
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 1), &state),
            EventResult::Emit(Msg::Focus(FocusState::intent(["scroll", "second"]))),
            "a content press is still a focus change, not a gutter drag"
        );
    }

    #[test]
    fn zero_sized_viewports_are_inert_without_dropping_declarations() {
        for area in [Rect::new(0, 0, 0, 3), Rect::new(0, 0, 4, 0)] {
            let mut driver = driver(4, 3);
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
                ctx.popup("popup", popup, PopupOptions::default(), move |ctx| {
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
    }

    impl Component<State, Msg> for LayerHost {
        fn declare(&mut self, ctx: &mut DeclareCtx<'_, State, Msg>) {
            let anchor = ctx.area();
            let escaped = Rect::new(anchor.x, anchor.y + 2, 5, 1);
            match self.layer {
                LayerExample::Hint => {
                    ctx.hint("layer", escaped, ScopeOptions::default(), move |ctx| {
                        ctx.paint_widget(Paragraph::new("HINT"), escaped);
                    });
                }
                LayerExample::Popup => {
                    ctx.popup("layer", escaped, PopupOptions::default(), move |ctx| {
                        ctx.paint_widget(Paragraph::new("POPUP"), escaped);
                        ctx.component("item", Probe::focusable("item"), escaped);
                    });
                }
                LayerExample::Modal => {
                    ctx.modal("layer", ModalProbe, escaped);
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

        fn paint(&mut self, ctx: &mut crate::runtime::PaintCtx<'_, State>) {
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

        fn scope_options(&self) -> ScopeOptions {
            ScopeOptions::default().focusable(true)
        }
    }

    fn render_layer_example(driver: &mut Driver<State, Msg>, state: &State, layer: LayerExample) {
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
            let mut driver = driver(10, 6);
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
                        Rect::new(0, 0, 10, 6),
                        PopupOptions::default(),
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
    fn popup_and_modal_keep_existing_escape_routing() {
        let mut popup = driver(8, 6);
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

        let mut modal = driver(8, 6);
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
        let mut driver = driver(10, 8);
        let state = State::default();
        let render = |driver: &mut Driver<State, Msg>| {
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
        let mut driver = driver(16, 8);
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
            let mut visible = driver(8, 6);
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

            let mut offscreen = driver(8, 6);
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
                    ctx.hint("layer", escaped, ScopeOptions::default(), move |ctx| {
                        ctx.paint_widget(Paragraph::new("LAYER"), escaped);
                    });
                }
                LayerExample::Popup => {
                    ctx.popup("layer", escaped, PopupOptions::default(), move |ctx| {
                        ctx.paint_widget(Paragraph::new("LAYER"), escaped);
                    });
                }
                LayerExample::Modal => {}
            }
        }
    }

    /// Captures on press and lets later events bubble, so an ancestor can see
    /// a captured descendant drag.
    struct CapturingLeaf;

    impl Component<State, Msg> for CapturingLeaf {
        fn declare(&mut self, _ctx: &mut DeclareCtx<'_, State, Msg>) {}

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

    #[derive(Debug)]
    struct HoverCaptureProbe {
        hovered: Rc<Cell<bool>>,
    }

    impl Component<State, Msg> for HoverCaptureProbe {
        fn declare(&mut self, _ctx: &mut DeclareCtx<'_, State, Msg>) {}

        fn paint(&mut self, ctx: &mut crate::runtime::PaintCtx<'_, State>) {
            self.hovered.set(ctx.hovered());
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
        let mut driver = driver(8, 4);
        let mut state = State::default();
        let hovered = Rc::new(Cell::new(false));
        let render = |driver: &mut Driver<State, Msg>, state: &State, hovered: &Rc<Cell<bool>>| {
            let hovered = Rc::clone(hovered);
            driver.render(state, move |ctx| {
                let hovered = Rc::clone(&hovered);
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
        let mut driver = driver(12, 8);
        let mut state = State::default();
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
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

        let EventResult::Emit(Msg::Area(offset)) = driver.event(
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
