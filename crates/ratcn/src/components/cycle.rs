//! A one-of-many control that cycles: it shows the current option, and every
//! act — a click, Enter, Space, an arrow — advances to the next one.
//!
//! ```text
//! Medium
//! ```
//!
//! A Cycle is the settings-row control for more than two options. Two options
//! are a [`Checkbox`](crate::Checkbox) wearing the words as its markers; three
//! or more, or an ordered scale (Small/Medium/Large), are a Cycle.
//!
//! The value paints as plain text in the style of a small ghost button: bare
//! at rest, tinted while hovered or focused, muted while disabled. Nothing
//! about it reads as chrome, and focus stays findable for keyboard users.

use std::{fmt, rc::Rc};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Widget,
};

use crate::{
    Theme,
    button_shape::filled_middle,
    color::{FOCUS_SHIFT, HOVER_SHIFT, away_from, dim},
    runtime::{
        Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, KeyEvent, MouseButton,
        MouseKind, PaintCtx, ScopeOptions,
    },
    text_width,
    theme::resolve_style,
};

/// A cycle's colors — the small ghost button's palette, one role per state.
///
/// At rest only [`foreground`](Self::foreground) paints, on nothing: the
/// surface shows through. Hover and focus each lay their background over the
/// declared area, disabled mutes the text, and each earlier state wins over
/// the later ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleStyle {
    /// Value color at rest; also the text color under hover and focus.
    pub foreground: Color,
    /// Background while focused.
    pub focused_background: Color,
    /// Background while hovered.
    pub hovered_background: Color,
    /// Value color while disabled.
    pub disabled_foreground: Color,
}

impl CycleStyle {
    /// The no-theme starting point: plain ANSI colors that render on any
    /// terminal.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            foreground: Color::Gray,
            focused_background: Color::Cyan,
            hovered_background: Color::LightCyan,
            disabled_foreground: Color::DarkGray,
        }
    }

    /// Colors derived from a theme, solving the same fills the ghost button
    /// solves: both climb away from the screen color, so they read as raised.
    #[must_use]
    pub fn from_theme(theme: &Theme) -> Self {
        let raised = away_from(theme.background);
        Self {
            foreground: theme.foreground,
            focused_background: dim(theme.secondary, raised, FOCUS_SHIFT),
            hovered_background: dim(theme.secondary, raised, HOVER_SHIFT),
            disabled_foreground: theme.muted_foreground,
        }
    }

    /// One paint pass's colors (see [`Self::from_theme`]). Disabled wins over
    /// focus, which wins over hover, which wins over rest.
    fn resolve(self, focused: bool, hovered: bool, disabled: bool) -> Style {
        if disabled {
            Style::default().fg(self.disabled_foreground)
        } else if focused {
            Style::default()
                .fg(self.foreground)
                .bg(self.focused_background)
        } else if hovered {
            Style::default()
                .fg(self.foreground)
                .bg(self.hovered_background)
        } else {
            Style::default().fg(self.foreground)
        }
    }
}

/// A cycle that only draws — an ordinary ratatui [`Widget`] with no focus,
/// events, or state. It paints the current option across `area`.
#[derive(Debug)]
pub struct CycleWidget<'a> {
    options: &'a [&'a str],
    selected: usize,
    focused: bool,
    hovered: bool,
    disabled: bool,
    theme: Option<Theme>,
    style: Option<CycleStyle>,
}

impl<'a> CycleWidget<'a> {
    /// A cycle over `options`, showing `options[selected]`.
    ///
    /// An out-of-range `selected` paints the first option rather than
    /// panicking: the widget half has no contract to enforce, and a caller
    /// that clamps nowhere else still gets a whole frame.
    #[must_use]
    pub fn new(options: &'a [&'a str], selected: usize) -> Self {
        Self {
            options,
            selected,
            focused: false,
            hovered: false,
            disabled: false,
            theme: None,
            style: None,
        }
    }

    /// Take colors from `theme`.
    #[must_use]
    pub const fn themed(mut self, theme: &Theme) -> Self {
        self.theme = Some(*theme);
        self
    }

    /// Exact colors, taking precedence over [`themed`](Self::themed).
    #[must_use]
    pub const fn style(mut self, style: CycleStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Paint the focused background.
    #[must_use]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Paint the hovered background.
    #[must_use]
    pub const fn hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Paint muted.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn resolved_style(&self) -> CycleStyle {
        match (self.style, self.theme) {
            (Some(style), _) => style,
            (None, Some(theme)) => CycleStyle::from_theme(&theme),
            (None, None) => CycleStyle::fallback(),
        }
    }
}

impl Widget for CycleWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 || self.options.is_empty() {
            return;
        }
        let selected = self.selected.min(self.options.len() - 1);
        let style = self
            .resolved_style()
            .resolve(self.focused, self.hovered, self.disabled);
        Line::from(filled_middle(self.options[selected], area.width as usize))
            .style(style)
            .render(area, buf);
    }
}

type ReadSelectedFn<S> = Rc<dyn Fn(&S) -> usize>;
type OnChangeFn<M> = Rc<dyn Fn(usize) -> M>;
type StyleFn = Rc<dyn Fn(&Theme) -> CycleStyle>;

/// A control that cycles through its options in place: the current value is
/// all it shows, and every click, <kbd>Enter</kbd>, <kbd>Space</kbd>,
/// <kbd>Right</kbd>/<kbd>l</kbd>, or <kbd>Ctrl+N</kbd> advances to the next,
/// wrapping at the end. <kbd>Left</kbd>/<kbd>h</kbd> walks backward, as does
/// <kbd>Ctrl+P</kbd>.
///
/// The row paints like a small ghost button — plain text at rest, a quiet
/// fill on hover or focus — so a column of cycles reads as values, not as a
/// wall of chrome.
///
/// The selection lives in app state and arrives through
/// [`selection`](Self::selection) as an index; without that binding the Cycle
/// paints but is not focusable and answers no events.
pub struct Cycle<S, M> {
    options: Rc<[Box<str>]>,
    selection: Option<(ReadSelectedFn<S>, OnChangeFn<M>)>,
    disabled: bool,
    style: Option<StyleFn>,
    /// The bound selection, resolved and clamped once per declaration.
    resolved_selected: usize,
    /// The columns the current option paints — the Cycle is as wide as the
    /// value it shows, no wider.
    resolved_width: u16,
}

impl<S, M> fmt::Debug for Cycle<S, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cycle")
            .field("options", &self.options)
            .field("selection", &self.selection.is_some())
            .field("disabled", &self.disabled)
            .field("style", &self.style.is_some())
            .finish_non_exhaustive()
    }
}

impl<S, M> Cycle<S, M> {
    /// Construct a Cycle from its options, in cycling order.
    #[must_use]
    pub fn new(options: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            options: options
                .into_iter()
                .map(|option| option.into().into_boxed_str())
                .collect(),
            selection: None,
            disabled: false,
            style: None,
            resolved_selected: 0,
            resolved_width: 0,
        }
    }

    /// Bind the selection and the message that moves it.
    ///
    /// `read` returns the index of the option shown; an out-of-range answer
    /// clamps to the last option rather than panicking. `on_change` receives
    /// the index the user advanced or backed up to. Without this binding the
    /// Cycle is not focusable and answers no events.
    #[must_use]
    pub fn selection(
        mut self,
        read: impl Fn(&S) -> usize + 'static,
        on_change: impl Fn(usize) -> M + 'static,
    ) -> Self {
        self.selection = Some((Rc::new(read), Rc::new(on_change)));
        self
    }

    /// Paint and answer muted, ignoring every event.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Supply exact colors, taking precedence over the theme.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> CycleStyle + 'static) -> Self {
        self.style = Some(Rc::new(style));
        self
    }

    fn can_act(&self) -> bool {
        !self.disabled && !self.options.is_empty() && self.selection.is_some()
    }

    /// The rect the current value paints in and answers events in: the Cycle
    /// is exactly as wide as the text it shows.
    fn value_area(&self, area: Rect) -> Rect {
        Rect {
            width: self.resolved_width.min(area.width),
            ..area
        }
    }

    /// Advance one option. Forward past the end wraps to the first; backward
    /// before the start wraps to the last.
    fn step(&self, backward: bool) -> EventResult<M> {
        let Some((_, on_change)) = &self.selection else {
            return EventResult::Ignored;
        };
        let len = self.options.len();
        let next = if backward {
            (self.resolved_selected + len - 1) % len
        } else {
            (self.resolved_selected + 1) % len
        };
        EventResult::Emit(on_change(next))
    }

    /// The key map, horizontal like the tab strip's: Left/Right and their vi
    /// letters step, the readline chords step too, commit keys advance. Shift
    /// belongs to the app, so Shift+Space passes through untouched.
    fn handle_key(&self, key: KeyEvent) -> EventResult<M> {
        let plain = !key.modifiers.any();
        let ctrl_only = key.modifiers.ctrl && !key.modifiers.alt && !key.modifiers.shift;
        match key.code {
            KeyCode::Right | KeyCode::Enter | KeyCode::Char('l' | ' ') if plain => self.step(false),
            KeyCode::Left | KeyCode::Char('h') if plain => self.step(true),
            KeyCode::Char('n') if ctrl_only => self.step(false),
            KeyCode::Char('p') if ctrl_only => self.step(true),
            _ => EventResult::Ignored,
        }
    }
}

impl<S: 'static, M: 'static> Component<S, M> for Cycle<S, M> {
    fn prepare(&mut self, state: &S) {
        self.resolved_selected = self
            .selection
            .as_ref()
            .map_or(0, |(read, _)| read(state))
            .min(self.options.len().saturating_sub(1));
        self.resolved_width = self
            .options
            .get(self.resolved_selected)
            .map_or(0, |option| text_width::display_width_u16(option));
    }

    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, S, M>) {
        // Everything a Cycle is lives on its own node: the paint below and
        // the events answered here. There is nothing to declare inside it.
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, S>) {
        let style = resolve_style(self.style.as_deref(), ctx.theme, CycleStyle::from_theme);
        let labels: Vec<&str> = self
            .options
            .iter()
            .map(std::convert::AsRef::as_ref)
            .collect();
        let widget = CycleWidget::new(&labels, self.resolved_selected)
            .focused(ctx.focused())
            .hovered(ctx.hovered())
            .disabled(self.disabled)
            .style(style);
        ctx.widget(widget, self.value_area(ctx.area()));
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _state: &S,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<M> {
        if !self.can_act() {
            return EventResult::Ignored;
        }
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseKind::Click(MouseButton::Left) => self.step(false),
                _ => EventResult::Ignored,
            },
            Event::Key(key) => self.handle_key(*key),
            _ => EventResult::Ignored,
        }
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default().focusable(self.can_act())
    }

    fn interaction_area(&self, area: Rect) -> Rect {
        self.value_area(area)
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::runtime::{ChildId, FocusState, Modifiers, Ratcn};
    use crate::test_support::{Driver, key, key_with, mouse};

    #[derive(Default)]
    struct State {
        focus: FocusState,
        size: usize,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Focus(FocusState),
        Size(usize),
    }

    const SIZES: [&str; 3] = ["Small", "Medium", "Large"];

    fn driver() -> Driver<State, Msg> {
        Driver::with(
            Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
            30,
            6,
        )
    }

    fn render(driver: &mut Driver<State, Msg>, state: &State) {
        driver.render(state, |ctx| {
            ctx.component(
                ChildId::Static("size"),
                Cycle::new(SIZES).selection(|state: &State| state.size, Msg::Size),
                Rect::new(2, 2, 10, 1),
            );
        });
    }

    /// Cycling wraps: the control is a ring, so advancing past the last option
    /// lands on the first rather than stopping. Each event runs through the
    /// app's update and a fresh declaration, exactly as a frame would.
    #[test]
    fn space_enter_and_right_advance_wrapping() {
        let mut driver = driver();
        let mut state = State {
            size: 2,
            ..State::default()
        };
        render(&mut driver, &state);
        let EventResult::Emit(msg) = driver.event(key(KeyCode::Char(' ')), &state) else {
            panic!("Space must cycle");
        };
        state.size = match msg {
            Msg::Size(size) => size,
            Msg::Focus(_) => panic!("unexpected focus message"),
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::Size(1))
        );
    }

    #[test]
    fn left_h_and_ctrl_p_walk_backward_wrapping() {
        let mut driver = driver();
        let mut state = State::default();
        render(&mut driver, &state);

        assert_eq!(
            driver.event(key(KeyCode::Left), &state),
            EventResult::Emit(Msg::Size(2))
        );
        state.size = 2;
        render(&mut driver, &state);
        assert_eq!(
            driver.event(key(KeyCode::Char('h')), &state),
            EventResult::Emit(Msg::Size(1))
        );
        state.size = 1;
        render(&mut driver, &state);
        assert_eq!(
            driver.event(
                key_with(
                    KeyCode::Char('p'),
                    Modifiers {
                        ctrl: true,
                        ..Modifiers::NONE
                    }
                ),
                &state
            ),
            EventResult::Emit(Msg::Size(0))
        );
    }

    /// Shift belongs to the app: Shift+Space is not this control's key.
    #[test]
    fn modified_commit_keys_pass_through() {
        let mut driver = driver();
        let state = State::default();
        render(&mut driver, &state);

        assert!(matches!(
            driver.event(
                key_with(
                    KeyCode::Char(' '),
                    Modifiers {
                        shift: true,
                        ..Modifiers::NONE
                    }
                ),
                &state
            ),
            EventResult::Ignored
        ));
    }

    #[test]
    fn a_click_advances_to_the_next_option() {
        let mut driver = driver();
        let state = State::default();
        render(&mut driver, &state);

        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 5, 2), &state),
            EventResult::Emit(Msg::Size(1))
        );
    }

    /// The ghost styling is the contract: bare text at rest, a fill when
    /// focused. Pinned on the widget, where focus is a plain argument.
    #[test]
    fn focus_lays_a_background_over_the_value_and_rest_does_not() {
        let area = Rect::new(0, 0, 12, 1);

        let mut rest = Buffer::empty(area);
        CycleWidget::new(&SIZES, 1).render(area, &mut rest);
        assert_eq!(
            rest.cell((3, 0)).expect("cell").bg,
            Color::Reset,
            "rest painted a background"
        );

        let mut focused = Buffer::empty(area);
        CycleWidget::new(&SIZES, 1)
            .focused(true)
            .render(area, &mut focused);
        assert_ne!(
            focused.cell((3, 0)).expect("cell").bg,
            Color::Reset,
            "a focused cycle must be findable by its fill"
        );
    }

    #[test]
    fn an_unbound_cycle_is_not_focusable_and_answers_nothing() {
        let mut driver = driver();
        let state = State::default();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("size"),
                Cycle::new(SIZES),
                Rect::new(2, 2, 10, 1),
            );
        });

        assert!(matches!(
            driver.event(key(KeyCode::Tab), &state),
            EventResult::Ignored
        ));
        assert!(matches!(
            driver.event(key(KeyCode::Char(' ')), &state),
            EventResult::Ignored
        ));
    }

    /// A stored index that outlived its options list clamps instead of
    /// panicking or painting nothing.
    #[test]
    fn an_out_of_range_selection_clamps_to_the_last_option() {
        let area = Rect::new(0, 0, 12, 1);
        let mut buffer = Buffer::empty(area);
        CycleWidget::new(&SIZES, 9).render(area, &mut buffer);

        let row: String = (0..12u16)
            .map(|column| buffer.cell((column, 0)).expect("cell").symbol())
            .collect();
        assert!(row.contains("Large"), "{row:?}");
    }
}
