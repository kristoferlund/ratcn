//! A short explanation floated beside the content it describes.
//!
//! Two nouns recur here:
//!
//! - The **trigger** is the content the tooltip explains — a button, a label,
//!   a chart. [`Tooltip`] wraps it: the trigger is declared *inside* the
//!   tooltip's area, so pointer and keyboard events over the trigger pass
//!   through the tooltip on their way up.
//! - The **bubble** is the bordered box holding the tooltip text. It is
//!   declared as a *hint layer* — a subtree painted above everything else that
//!   takes no input at all (see [`DeclareCtx::hint`]), which is what keeps a
//!   tooltip from swallowing the click on the control it describes.

use std::rc::Rc;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
};

use crate::Theme;
use crate::runtime::{
    Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, MouseKind, ScopeOptions,
    wrapped_height,
};
use crate::text_width::{display_width_u16, wrap_to_width};
use crate::theme::resolve_style;

/// Cells of padding between the border and the text, left and right.
const PADDING: u16 = 1;
/// Cells of chrome across the bubble's width: the two border columns plus
/// [`PADDING`] on each side. Measuring and painting both derive from this, so
/// the width [`TooltipWidget::width`] reports is the width that paints.
const CHROME: u16 = 2 + PADDING * 2;
/// The bubble's border rows, top and bottom.
const BORDER_ROWS: u16 = 2;
/// Child id of the hint layer [`Tooltip`] declares.
const BUBBLE_ID: &str = "bubble";

/// Which side of the trigger the tooltip bubble is placed on.
///
/// This is a preference, not a guarantee: [`Tooltip`] flips to the opposite
/// side when the preferred one has no room inside the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TooltipSide {
    /// Above the trigger, horizontally centered on it. The default.
    #[default]
    Top,
    /// Below the trigger, horizontally centered on it.
    Bottom,
    /// Left of the trigger, vertically centered on it.
    Left,
    /// Right of the trigger, vertically centered on it.
    Right,
}

impl TooltipSide {
    /// The side a tooltip flips to when this one does not fit.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// Every color a tooltip can paint.
///
/// A tooltip has one surface — the bubble — and it does not vary with
/// interaction: a tooltip is never focused, hovered, or disabled, so there is
/// no state resolver here, only three flat colors.
///
/// [`from_theme`](Self::from_theme) derives all of them from a [`Theme`];
/// build one by hand only for colors the theme cannot express, and pass it via
/// [`Tooltip::style`] or [`TooltipWidget::style`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooltipStyle {
    /// Tooltip text color.
    pub foreground: Color,
    /// Fill behind the text and inside the border.
    pub background: Color,
    /// Border color around the bubble.
    pub border: Color,
}

impl TooltipStyle {
    /// A neutral style using plain ANSI colors, for use without a [`Theme`].
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            foreground: Color::White,
            background: Color::Reset,
            border: Color::DarkGray,
        }
    }

    /// Derive every tooltip color from `theme`.
    ///
    /// This is what [`Tooltip`] calls when no custom style is configured. Call
    /// it directly when a tooltip should start from the active theme and alter
    /// only selected colors.
    #[must_use]
    pub const fn from_theme(theme: &Theme) -> Self {
        Self {
            foreground: theme.foreground,
            background: theme.surface,
            border: theme.border,
        }
    }
}

/// A tooltip bubble that only draws, with no focus, events, or app state.
///
/// **Usable in any ratatui app.** Nothing here depends on
/// [`Ratcn`](crate::runtime::Ratcn) or the component layer: render it wherever
/// you want a bordered explanation and keep deciding when it shows however you
/// already do. [`width`](Self::width) and [`height`](Self::height) report the
/// area it needs, so a caller placing it by hand measures the same box that
/// paints.
///
/// It paints a rounded border, fills the inside, and lays the text out with one
/// cell of padding on each side, wrapped to the area it is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooltipWidget<'a> {
    text: &'a str,
    style: TooltipStyle,
}

impl<'a> TooltipWidget<'a> {
    /// Construct a bubble showing `text`.
    ///
    /// Embedded newlines start a new line; anything longer than the area
    /// wraps.
    #[must_use]
    pub const fn new(text: &'a str) -> Self {
        Self {
            text,
            style: TooltipStyle::fallback(),
        }
    }

    /// Take colors from `theme`.
    #[must_use]
    pub const fn themed(mut self, theme: &Theme) -> Self {
        self.style = TooltipStyle::from_theme(theme);
        self
    }

    /// Use these exact colors.
    #[must_use]
    pub const fn style(mut self, style: TooltipStyle) -> Self {
        self.style = style;
        self
    }

    /// The width the text needs without wrapping: the widest line plus the
    /// border and padding columns.
    ///
    /// This is a natural size, not a limit — the widget paints into whatever
    /// area it is given and wraps to fit. Pair it with
    /// [`height`](Self::height) after capping it to the width you can afford.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.text
            .lines()
            .map(display_width_u16)
            .max()
            .unwrap_or(0)
            .saturating_add(CHROME)
    }

    /// The rows the text needs inside a bubble `width` cells wide: the wrapped
    /// line count plus the two border rows.
    ///
    /// `width` is the outer width, border columns included, so pass the same
    /// value you will paint with. A width with no room for text still costs
    /// the two border rows.
    #[must_use]
    pub fn height(&self, width: u16) -> u16 {
        let inner = width.saturating_sub(CHROME);
        if inner == 0 {
            return BORDER_ROWS;
        }
        wrapped_height(self.text, inner).saturating_add(BORDER_ROWS)
    }
}

impl Widget for TooltipWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let block = Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(self.style.border).bg(self.style.background))
            .style(Style::new().bg(self.style.background));
        let inner = block.inner(area);
        block.render(area, buf);
        let text_area = Rect {
            x: inner.x.saturating_add(PADDING),
            width: inner.width.saturating_sub(PADDING * 2),
            ..inner
        }
        .intersection(*buf.area());
        if text_area.is_empty() {
            return;
        }
        let lines = wrap_to_width(self.text, usize::from(text_area.width))
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>();
        Paragraph::new(lines)
            .style(
                Style::new()
                    .fg(self.style.foreground)
                    .bg(self.style.background),
            )
            .render(text_area, buf);
    }
}

type ReadOpenFn<S> = Rc<dyn Fn(&S, bool) -> bool>;
type OnOpenChangeFn<M> = Rc<dyn Fn(bool) -> M>;
type OpenBinding<S, M> = (ReadOpenFn<S>, Option<OnOpenChangeFn<M>>);
type StyleFn = Rc<dyn Fn(&Theme) -> TooltipStyle>;
/// The trigger closure, boxed for storage until the declaration runs it.
type TriggerFn<S, M> = Box<dyn FnOnce(&mut DeclareCtx<'_, S, M>)>;

/// A wrapper that floats an explanation beside the content it describes.
///
/// The Tooltip occupies the area it is declared with, and that area *is* the
/// trigger: whatever [`trigger`](Self::trigger) declares fills it, becomes a
/// child of the Tooltip, and keeps its own focus and click behavior. The
/// Tooltip adds nothing over it — it paints nothing of its own there and never
/// takes focus, so wrapping a button in one changes neither its looks nor its
/// keyboard order.
///
/// When [`open`](Self::open) reads `true`, the bubble is declared as a *hint
/// layer*: a subtree painted above everything else that takes no input at all
/// (see [`DeclareCtx::hint`]). A press over the bubble reaches whatever the
/// bubble covers, focus never moves into it, and it dims nothing. That is the
/// whole reason a tooltip can safely float over the control it explains.
///
/// # Who decides when it shows
///
/// Hover, unless the app says otherwise. Bound to nothing, a Tooltip shows its
/// bubble while the pointer rests anywhere inside the trigger and hides it
/// again when the pointer leaves — the runtime owns hover, and
/// [`DeclareCtx::pointer_within`] answers for this declaration while it is
/// being declared, so nothing is stored and no message is routed.
///
/// [`open_when`](Self::open_when) replaces that rule with one of the app's
/// own. The reader receives both app state and the same hover answer, so the
/// two compose:
///
/// - `|_, hovered| hovered` is the default, spelled out.
/// - `|state, hovered| hovered && !state.controls_disabled` gates it.
/// - `|state, hovered| hovered || state.focus.contains_path(["save"])` adds
///   keyboard focus, which the component cannot observe on its own: focus
///   changes are the runtime's messages, not this component's events.
///
/// Whichever decides it, the answer starts with the hover the *previous* frame
/// resolved. The Tooltip also checks the pointer against the trigger's current
/// declaration geometry, so reflow or scrolling that moves the trigger away
/// closes the bubble on that redraw. Other identity changes with no pointer
/// motion — a modal opening over the trigger, for example — can still settle
/// one frame later.
///
/// [`open`](Self::open) is the same reader plus a message, for an app that
/// keeps a flag the Tooltip should change. It asks for two:
///
/// - `on_open_change(true)` when the pointer moves over the trigger while the
///   tooltip is closed.
/// - `on_open_change(false)` on an unmodified Esc, while the tooltip is open
///   and focus is somewhere inside the trigger — the key bubbles out of the
///   focused child to the Tooltip.
///
/// # Placement
///
/// [`side`](Self::side) picks the preferred side and defaults to
/// [`TooltipSide::Top`]. The bubble is centered on the trigger's other axis,
/// and if the preferred side has no room inside
/// [`frame_area`](DeclareCtx::frame_area) it flips to the opposite side; if
/// neither side fits it stays on the preferred one. Either way the bubble is
/// finally clamped inside the frame, so it is always fully visible.
///
/// Width is the text's natural width capped by [`max_width`](Self::max_width)
/// and by the frame; height follows from wrapping the text to that width.
///
/// # Reserved id
///
/// The bubble is declared with the child id `"bubble"` inside the Tooltip's
/// own identity, so do not give a trigger child that id.
pub struct Tooltip<S, M> {
    text: String,
    side: TooltipSide,
    max_width: u16,
    open: Option<OpenBinding<S, M>>,
    /// Taken by the declaration that runs it; the Tooltip needs nothing of it
    /// afterwards.
    trigger: Option<TriggerFn<S, M>>,
    style: Option<StyleFn>,
    /// Whether the pointer was inside this declaration, as the last pass
    /// read it. Kept so event handling — which has no declaration context —
    /// resolves openness the same way the declaration did.
    pointer_within: bool,
}

impl<S, M> std::fmt::Debug for Tooltip<S, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tooltip")
            .field("text", &self.text)
            .field("side", &self.side)
            .field("max_width", &self.max_width)
            .field("open", &self.open.is_some())
            .field("trigger", &self.trigger.is_some())
            .finish_non_exhaustive()
    }
}

impl<S, M> Tooltip<S, M> {
    /// Default cap on the bubble's outer width, border columns included.
    pub const DEFAULT_MAX_WIDTH: u16 = 40;

    /// Construct a tooltip showing `text`.
    ///
    /// It shows while the pointer is inside it until
    /// [`open_when`](Self::open_when) or [`open`](Self::open) says otherwise,
    /// and nothing sits under it until [`trigger`](Self::trigger) declares the
    /// content it describes.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            side: TooltipSide::default(),
            max_width: Self::DEFAULT_MAX_WIDTH,
            open: None,
            trigger: None,
            style: None,
            pointer_within: false,
        }
    }

    /// Decide when the bubble shows, with nothing to write back.
    ///
    /// The usual form when the default — showing while the pointer is inside
    /// the trigger — is not quite the rule. `read` receives app state and
    /// whether the pointer is within this declaration, and returns whether to
    /// declare the bubble:
    ///
    /// ```
    /// # use ratcn::{Tooltip, runtime::FocusState};
    /// # #[derive(Default)]
    /// # struct AppState { focus: FocusState }
    /// # enum Msg {}
    /// let tip = Tooltip::<AppState, Msg>::new("Save the current file")
    ///     .open_when(|state: &AppState, hovered| {
    ///         hovered || state.focus.contains_path(["save_tip"])
    ///     });
    /// ```
    ///
    /// The focus query is a root-anchored prefix, so the id to pass is the
    /// Tooltip's own — its trigger's children sit beneath it.
    ///
    /// Note what the focus half costs: a click focuses what it hits, so on its
    /// own that reader leaves the bubble showing after a press, until focus
    /// moves elsewhere. If that is not what you want, pair focus with your own
    /// record of which device the user is on — the app sees every event, so
    /// `state.keyboard = !matches!(event, Event::Mouse(_))` before routing is
    /// enough. That is the distinction the web draws between `:focus` and
    /// `:focus-visible`, and `demos/tooltip` shows it end to end.
    ///
    /// Use [`open`](Self::open) instead when the app keeps a flag of its own
    /// that the Tooltip should change: with this form the component emits
    /// nothing, so its pointer and Esc handling do not apply.
    #[must_use]
    pub fn open_when(mut self, read: impl Fn(&S, bool) -> bool + 'static) -> Self {
        self.open = Some((Rc::new(read), None));
        self
    }

    /// Bind whether the bubble is showing, and the message that changes it.
    ///
    /// The [`open_when`](Self::open_when) reader plus a writer, for an app that
    /// keeps its own flag — `|state, _| state.tip_open` is the shape. `read`
    /// runs during rendering and event handling; the bubble is declared only
    /// when it returns `true`.
    ///
    /// Reach for [`open_when`](Self::open_when) instead unless the app keeps
    /// such a flag: showing is usually a view of hover and focus, and then
    /// there is nothing to write.
    ///
    /// `on_open_change` receives each requested state — showing is a
    /// continuously tracked value, like a cursor, not a one-shot commit.
    /// The component asks for `true` when the pointer moves onto the trigger,
    /// and for `false` on Esc while open. Nothing is emitted for the pointer
    /// leaving: motion away from the trigger routes elsewhere and never
    /// reaches the Tooltip, so a reader that ignores its `hovered` argument
    /// keeps the bubble up until the app says otherwise.
    #[must_use]
    pub fn open(
        mut self,
        read: impl Fn(&S, bool) -> bool + 'static,
        on_open_change: impl Fn(bool) -> M + 'static,
    ) -> Self {
        self.open = Some((Rc::new(read), Some(Rc::new(on_open_change))));
        self
    }

    /// Declare the content the tooltip describes.
    ///
    /// The closure runs during the Tooltip's own `declare`, once per
    /// declaration pass, with a [`DeclareCtx`] whose
    /// [`area`](DeclareCtx::area) is the Tooltip's area — paint into it with
    /// the standard paint methods and declare children with
    /// [`component`](DeclareCtx::component). Those children belong
    /// to the Tooltip's identity and take part in focus traversal normally; the
    /// Tooltip itself never competes with them for focus.
    ///
    /// Without a trigger the Tooltip paints nothing in its area and is only a
    /// hover target.
    #[must_use]
    pub fn trigger(mut self, f: impl FnOnce(&mut DeclareCtx<'_, S, M>) + 'static) -> Self {
        self.trigger = Some(Box::new(f));
        self
    }

    /// Set the preferred side of the trigger. Defaults to
    /// [`TooltipSide::Top`].
    ///
    /// The bubble flips to [`the opposite side`](TooltipSide::opposite) when
    /// this one has no room inside the frame.
    #[must_use]
    pub const fn side(mut self, side: TooltipSide) -> Self {
        self.side = side;
        self
    }

    /// Cap the bubble's outer width, border columns included.
    ///
    /// Text wider than this wraps, making the bubble taller. Defaults to
    /// [`DEFAULT_MAX_WIDTH`](Self::DEFAULT_MAX_WIDTH). The frame is a further
    /// cap: a bubble never grows wider than the terminal.
    #[must_use]
    pub const fn max_width(mut self, max_width: u16) -> Self {
        self.max_width = max_width;
        self
    }

    /// Replace the theme-derived style.
    ///
    /// The closure receives the active theme each render, so its result follows
    /// runtime theme changes.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> TooltipStyle + 'static) -> Self {
        self.style = Some(Rc::new(style));
        self
    }

    /// Whether the bubble should show, from the app's rule or — with none
    /// bound — from the pointer alone.
    fn is_open(&self, state: &S) -> bool {
        self.open.as_ref().map_or(self.pointer_within, |(read, _)| {
            read(state, self.pointer_within)
        })
    }

    fn request(&self, open: bool) -> EventResult<M> {
        match self.open.as_ref() {
            Some((_, Some(on_open_change))) => EventResult::Emit(on_open_change(open)),
            // Read-only: the app derives showing from state it already keeps,
            // so there is nothing to ask it for. The key still bubbles.
            _ => EventResult::Ignored,
        }
    }
}

impl<S: 'static, M: 'static> Component<S, M> for Tooltip<S, M> {
    fn declare(&mut self, ctx: &mut DeclareCtx<'_, S, M>) {
        // Read before the trigger declares: the question is about this
        // Tooltip's subtree, which is what is open right now.
        self.pointer_within = ctx.pointer_within_area();
        let open = self.is_open(ctx.state());
        if let Some(trigger) = self.trigger.take() {
            trigger(ctx);
        }
        if !open {
            return;
        }
        let style = resolve_style(self.style.as_deref(), ctx.theme, TooltipStyle::from_theme);
        let widget = TooltipWidget::new(&self.text).style(style);
        let bounds = ctx.frame_area();
        let width = widget.width().min(self.max_width).min(bounds.width);
        let height = widget.height(width).min(bounds.height);
        let Some(area) = bubble_area(ctx.area(), bounds, self.side, width, height) else {
            return;
        };
        // The bubble paints after the walk, so it has to own its text rather
        // than borrow the declaration that sized it.
        let text = self.text.clone();
        // A hint layer: painted above everything, and inert. The press it
        // floats over still reaches the trigger underneath.
        ctx.hint(BUBBLE_ID, ScopeOptions::default(), area, move |ctx| {
            ctx.paint(move |ctx| {
                ctx.widget(TooltipWidget::new(&text).style(style), area);
            });
        });
    }

    fn handle_event(
        &mut self,
        event: &Event,
        state: &S,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<M> {
        let open = self.is_open(state);
        match event {
            // Esc bubbles here out of whatever inside the trigger holds focus.
            Event::Key(key) if open && key.code == KeyCode::Esc && !key.modifiers.any() => {
                self.request(false)
            }
            // The pointer entering the trigger is the one hover transition a
            // component can see; leaving routes elsewhere and never reaches
            // here, so closing on leave is read from app state instead.
            Event::Mouse(mouse) if !open && mouse.kind == MouseKind::Moved => self.request(true),
            _ => EventResult::Ignored,
        }
    }
}

/// Place a `width`×`height` bubble beside `trigger`, preferring `side`,
/// flipping when it does not fit, and clamped inside `bounds`.
///
/// `None` when the bubble has no area to paint into at all.
fn bubble_area(
    trigger: Rect,
    bounds: Rect,
    side: TooltipSide,
    width: u16,
    height: u16,
) -> Option<Rect> {
    if width == 0 || height == 0 || bounds.is_empty() {
        return None;
    }
    let side = if fits(trigger, bounds, side, width, height) {
        side
    } else {
        let flipped = side.opposite();
        if fits(trigger, bounds, flipped, width, height) {
            flipped
        } else {
            side
        }
    };
    let (x, y) = match side {
        TooltipSide::Top => (
            center(trigger.x, trigger.width, width),
            trigger.y.saturating_sub(height),
        ),
        TooltipSide::Bottom => (center(trigger.x, trigger.width, width), trigger.bottom()),
        TooltipSide::Left => (
            trigger.x.saturating_sub(width),
            center(trigger.y, trigger.height, height),
        ),
        TooltipSide::Right => (trigger.right(), center(trigger.y, trigger.height, height)),
    };
    Some(Rect::new(
        clamp(x, width, bounds.x, bounds.right()),
        clamp(y, height, bounds.y, bounds.bottom()),
        width,
        height,
    ))
}

/// Whether a `width`×`height` bubble fits between the trigger and the `bounds`
/// edge on `side`.
const fn fits(trigger: Rect, bounds: Rect, side: TooltipSide, width: u16, height: u16) -> bool {
    match side {
        TooltipSide::Top => trigger.y >= bounds.y.saturating_add(height),
        TooltipSide::Bottom => trigger.bottom().saturating_add(height) <= bounds.bottom(),
        TooltipSide::Left => trigger.x >= bounds.x.saturating_add(width),
        TooltipSide::Right => trigger.right().saturating_add(width) <= bounds.right(),
    }
}

/// The start coordinate that centers `size` on the span `start..start + extent`.
const fn center(start: u16, extent: u16, size: u16) -> u16 {
    start
        .saturating_add(extent / 2)
        .saturating_sub(size.div_ceil(2))
}

/// Pull `start` back until `size` fits between `low` and `high`.
const fn clamp(start: u16, size: u16, low: u16, high: u16) -> u16 {
    let last = high.saturating_sub(size);
    if start > last {
        last
    } else if start < low {
        low
    } else {
        start
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;
    use crate::Button;
    use crate::runtime::{FocusState, KeyEvent, Modifiers, MouseButton, MouseEvent, Ratcn};

    const TIP: &str = "Save the file";

    #[derive(Debug, PartialEq)]
    enum Msg {
        Open(bool),
        Pressed,
        Focus(FocusState),
    }

    #[derive(Default)]
    struct State {
        open: bool,
        focus: FocusState,
    }

    fn tooltip(side: TooltipSide) -> Tooltip<State, Msg> {
        Tooltip::new(TIP)
            .side(side)
            .open(|state: &State, _| state.open, Msg::Open)
            .trigger(|ctx| {
                let area = ctx.area();
                ctx.component("save", Button::new("Save").on_press(|| Msg::Pressed), area);
            })
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

        fn render(&mut self, state: &State, area: Rect, side: TooltipSide) {
            let theme = Theme::default_dark();
            self.terminal
                .draw(|frame| {
                    self.ratcn.render(frame, state, &theme, |ctx| {
                        // A focusable sibling declared first, so startup focus
                        // has somewhere to land that is not the tooltip.
                        ctx.component(
                            "before",
                            Button::new("Open").on_press(|| Msg::Pressed),
                            Rect::new(0, 0, 6, 1),
                        );
                        ctx.component("tip", tooltip(side), area);
                    });
                })
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

    fn mouse(kind: MouseKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: Modifiers::NONE,
        })
    }

    /// The read-only binding is the recommended shape: the app derives
    /// showing from state it already keeps, so the component emits nothing.
    #[test]
    fn a_read_only_binding_shows_the_bubble_and_emits_nothing() {
        let mut driver = Driver::new(30, 10);
        let state = State {
            open: true,
            ..State::default()
        };
        let theme = Theme::default_dark();
        driver
            .terminal
            .draw(|frame| {
                driver.ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.component(
                        "before",
                        Button::new("Open").on_press(|| Msg::Pressed),
                        Rect::new(0, 0, 6, 1),
                    );
                    ctx.component(
                        "tip",
                        Tooltip::new(TIP)
                            .open_when(|state: &State, _| state.open)
                            .trigger(|ctx| {
                                let area = ctx.area();
                                ctx.component(
                                    "save",
                                    Button::new("Save").on_press(|| Msg::Pressed),
                                    area,
                                );
                            }),
                        Rect::new(2, 5, 8, 3),
                    );
                });
            })
            .expect("draw");

        assert!(
            (0..10).any(|row| driver.row(row).contains(TIP)),
            "a read-only binding still shows the bubble"
        );
        assert_eq!(
            driver.event(Event::Key(KeyEvent::new(KeyCode::Esc)), &state),
            EventResult::Ignored,
            "with nothing to write back, Esc bubbles to the app instead"
        );
        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 4, 6), &state),
            EventResult::Consumed,
            "and pointer motion asks for nothing — the runtime's own hover moved, \
             which is all `Consumed` reports here"
        );
    }

    /// The app's rule replaces the default rather than adding to it: a reader
    /// that never says yes keeps the bubble hidden with the pointer sitting on
    /// the trigger.
    #[test]
    fn an_open_reader_overrides_the_hover_default() {
        let mut driver = Driver::new(30, 10);
        let state = State::default();
        let theme = Theme::default_dark();
        let render = |driver: &mut Driver| {
            driver
                .terminal
                .draw(|frame| {
                    driver.ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.component(
                            "tip",
                            Tooltip::new(TIP)
                                .open_when(|_: &State, _| false)
                                .trigger(|ctx| {
                                    let area = ctx.area();
                                    ctx.component(
                                        "save",
                                        Button::new("Save").on_press(|| Msg::Pressed),
                                        area,
                                    );
                                }),
                            Rect::new(2, 5, 8, 3),
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut driver);
        driver.event(mouse(MouseKind::Moved, 4, 6), &state);
        render(&mut driver);
        assert!(
            (0..10).all(|row| !driver.row(row).contains(TIP)),
            "the reader said no, so hovering does not show the bubble"
        );
    }

    // Show-on-hover: the one hover transition a component can observe is the
    // pointer arriving, and it must ask the app rather than store the flag.
    #[test]
    fn pointer_over_the_trigger_asks_to_open_once() {
        let mut driver = Driver::new(30, 10);
        let mut state = State::default();
        driver.render(&state, Rect::new(2, 5, 8, 3), TooltipSide::Top);

        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 4, 6), &state),
            EventResult::Emit(Msg::Open(true)),
            "the motion that arrives on the trigger asks the app to open"
        );
        // Already open, the same motion is not re-announced: the runtime
        // consumes it as the ordinary redraw signal instead.
        state.open = true;
        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 4, 6), &state),
            EventResult::Consumed
        );
    }

    // The ordinary tooltip: nothing bound, nothing stored, the bubble follows
    // the pointer in and out. Leaving is the half the component cannot see —
    // motion away routes to whatever it hits and never reaches the Tooltip —
    // so it is the runtime's hover that closes the bubble again.
    #[test]
    fn an_unbound_tooltip_opens_and_closes_with_the_pointer() {
        let mut driver = Driver::new(30, 10);
        let state = State::default();
        let theme = Theme::default_dark();
        let render = |driver: &mut Driver| {
            driver
                .terminal
                .draw(|frame| {
                    driver.ratcn.render(frame, &state, &theme, |ctx| {
                        ctx.component(
                            "tip",
                            Tooltip::new(TIP).trigger(|ctx| {
                                let area = ctx.area();
                                ctx.component(
                                    "save",
                                    Button::new("Save").on_press(|| Msg::Pressed),
                                    area,
                                );
                            }),
                            Rect::new(2, 5, 8, 3),
                        );
                    });
                })
                .expect("draw");
        };

        render(&mut driver);
        assert!(
            (0..10).all(|row| !driver.row(row).contains(TIP)),
            "nothing is hovered yet"
        );

        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 4, 6), &state),
            EventResult::Consumed,
            "the motion changed hover, so the frame it produced is stale"
        );
        render(&mut driver);
        assert!(
            (0..10).any(|row| driver.row(row).contains(TIP)),
            "the pointer is inside the tooltip, so the bubble shows"
        );

        assert_eq!(
            driver.event(mouse(MouseKind::Moved, 25, 9), &state),
            EventResult::Consumed,
            "leaving for empty space empties hover"
        );
        render(&mut driver);
        assert!(
            (0..10).all(|row| !driver.row(row).contains(TIP)),
            "and the bubble goes with it"
        );
    }

    // Esc reaches the Tooltip by bubbling out of the focused trigger child, so
    // a keyboard user can dismiss an explanation without touching the mouse.
    #[test]
    fn escape_while_open_asks_to_close() {
        let mut driver = Driver::new(30, 10);
        let state = State {
            open: true,
            focus: FocusState::intent(["tip", "save"]),
        };
        driver.render(&state, Rect::new(2, 5, 8, 3), TooltipSide::Top);

        assert_eq!(
            driver.event(Event::Key(KeyEvent::new(KeyCode::Esc)), &state),
            EventResult::Emit(Msg::Open(false))
        );
        // Closed, Esc belongs to the app again.
        let closed = State {
            focus: FocusState::intent(["tip", "save"]),
            ..State::default()
        };
        assert_eq!(
            driver.event(Event::Key(KeyEvent::new(KeyCode::Esc)), &closed),
            EventResult::Ignored
        );
    }

    // The bubble must stay wholly on screen, so a preferred side with no room
    // flips rather than clipping.
    #[test]
    fn the_bubble_flips_to_the_opposite_side_when_the_preferred_one_has_no_room() {
        let bounds = Rect::new(0, 0, 30, 10);
        let top_trigger = Rect::new(4, 0, 8, 1);
        let placed = bubble_area(top_trigger, bounds, TooltipSide::Top, 10, 3).expect("bubble");
        assert_eq!(placed.y, top_trigger.bottom(), "flipped below");

        let bottom_trigger = Rect::new(4, 9, 8, 1);
        let placed =
            bubble_area(bottom_trigger, bounds, TooltipSide::Bottom, 10, 3).expect("bubble");
        assert_eq!(placed.y, bottom_trigger.y - 3, "flipped above");

        let left_trigger = Rect::new(0, 4, 8, 1);
        let placed = bubble_area(left_trigger, bounds, TooltipSide::Left, 10, 3).expect("bubble");
        assert_eq!(placed.x, left_trigger.right(), "flipped right");

        // Neither side fits: the bubble keeps the preferred side and is pulled
        // inside the frame rather than dropped.
        let tall = Rect::new(0, 0, 30, 3);
        let placed =
            bubble_area(Rect::new(4, 1, 8, 1), tall, TooltipSide::Top, 10, 3).expect("bubble");
        assert_eq!(placed.y, 0);
        assert!(tall.contains(ratatui::layout::Position::new(placed.x, placed.y)));
    }

    // Cross-axis centering and clamping: a bubble beside a trigger at the
    // frame's edge is shifted, never allowed to hang off it.
    #[test]
    fn the_bubble_is_centred_on_the_trigger_and_clamped_inside_the_frame() {
        let bounds = Rect::new(0, 0, 30, 10);
        let placed =
            bubble_area(Rect::new(10, 5, 8, 1), bounds, TooltipSide::Top, 10, 3).expect("bubble");
        assert_eq!(placed.x, 9, "centred on the trigger");
        assert_eq!(placed.y, 2);

        let placed =
            bubble_area(Rect::new(26, 5, 4, 1), bounds, TooltipSide::Top, 10, 3).expect("bubble");
        assert_eq!(placed.right(), bounds.right(), "pulled inside the frame");
    }

    // A tooltip explains, it does not act: it must never become a Tab stop,
    // and startup focus has to resolve straight through it to the trigger.
    #[test]
    fn the_tooltip_is_never_a_focus_stop() {
        let mut driver = Driver::new(30, 10);
        let state = State::default();
        driver.render(&state, Rect::new(2, 5, 8, 3), TooltipSide::Top);

        assert_eq!(
            driver.event(Event::Key(KeyEvent::new(KeyCode::Tab)), &state),
            EventResult::Emit(Msg::Focus(FocusState::intent(["tip", "save"]))),
            "focus lands on the trigger, never on its tooltip"
        );
    }

    // The whole reason the bubble is a hint layer: it floats over the control
    // it explains, and a press must still reach that control.
    #[test]
    fn a_press_over_the_bubble_reaches_the_trigger_beneath_it() {
        // A frame with room for the bubble on neither side, so it is clamped
        // on top of the trigger it explains.
        let frame = Rect::new(0, 0, 30, 4);
        let trigger = Rect::new(2, 1, 20, 1);
        let mut driver = Driver::new(frame.width, frame.height);
        let state = State {
            open: true,
            ..State::default()
        };
        driver.render(&state, trigger, TooltipSide::Top);

        let width = TooltipWidget::new(TIP).width();
        let bubble = bubble_area(trigger, frame, TooltipSide::Top, width, 3).expect("bubble");
        assert!(bubble.intersects(trigger), "{bubble:?}");

        driver.event(
            mouse(MouseKind::Down(MouseButton::Left), trigger.x, trigger.y),
            &state,
        );
        assert_eq!(
            driver.event(
                mouse(MouseKind::Click(MouseButton::Left), trigger.x, trigger.y),
                &state
            ),
            EventResult::Emit(Msg::Pressed),
            "the hint layer takes no pointer input, so the trigger still presses"
        );
    }

    // The bubble paints above siblings declared after it, since a hint layer
    // composites over the frame.
    #[test]
    fn the_bubble_paints_above_everything_else() {
        let mut driver = Driver::new(30, 10);
        let state = State {
            open: true,
            ..State::default()
        };
        driver.render(&state, Rect::new(2, 5, 8, 1), TooltipSide::Top);
        assert!(driver.row(3).contains(TIP), "{}", driver.row(3));
        assert!(driver.row(2).contains('╭'), "{}", driver.row(2));
    }

    // The paint half stands alone: an ordinary ratatui widget, measuring the
    // same box it draws.
    #[test]
    fn the_widget_measures_and_paints_the_same_bordered_box() {
        let widget = TooltipWidget::new("Save").style(TooltipStyle::fallback());
        assert_eq!(widget.width(), 4 + CHROME);
        assert_eq!(widget.height(widget.width()), 1 + BORDER_ROWS);

        let area = Rect::new(0, 0, widget.width(), widget.height(widget.width()));
        let mut buffer = Buffer::empty(area);
        widget.render(area, &mut buffer);

        let row: String = (0..area.width)
            .map(|column| buffer.cell((column, 1)).expect("cell").symbol())
            .collect();
        assert_eq!(row, "│ Save │");
        assert_eq!(buffer.cell((0, 0)).expect("corner").symbol(), "╭");
        assert_eq!(
            buffer.cell((2, 1)).expect("text").style().fg,
            Some(TooltipStyle::fallback().foreground)
        );
    }

    // Wrapping is what makes `max_width` usable: the text that does not fit
    // costs rows instead of columns, and both halves agree on how many.
    #[test]
    fn narrow_widths_wrap_and_are_measured_as_extra_rows() {
        let widget = TooltipWidget::new("Save the file");
        assert!(widget.height(9) > widget.height(widget.width()));
        // No room for text at all still costs the two border rows.
        assert_eq!(widget.height(CHROME), BORDER_ROWS);
    }

    #[test]
    fn theme_colors_separate_the_bubble_from_the_surface_behind_it() {
        for theme in Theme::presets() {
            let style = TooltipStyle::from_theme(theme);
            assert_ne!(style.background, style.foreground, "{}", theme.name);
            assert_ne!(style.border, style.background, "{}", theme.name);
        }
    }
}
