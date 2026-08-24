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
//! The value sits in a subtle field at rest, then brightens while hovered or
//! focused; it mutes while disabled. That resting field hints that the value
//! is clickable without competing with the setting label.

use std::{fmt, rc::Rc};

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect, Size},
    style::{Color, Style},
    text::Line,
    widgets::Widget,
};

use crate::{
    ListStyle, Theme,
    button_shape::filled_middle,
    linear_nav::{Axis, step_key},
    runtime::{
        Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, KeyEvent, MeasuredComponent,
        MouseButton, MouseKind, PaintCtx, ScopeOptions, Step,
    },
    text_width,
    theme::resolve_style,
};

/// A cycle's colors, sharing the List's three background states.
///
/// At rest [`foreground`](Self::foreground) sits on
/// [`background`](Self::background). Hover and focus use the corresponding
/// List backgrounds, with hover beating focus. Disabled mutes the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleStyle {
    /// Value color at rest.
    pub foreground: Color,
    /// Background at rest.
    pub background: Color,
    /// Value color while focused.
    pub focused_foreground: Color,
    /// Background while focused.
    pub focused_background: Color,
    /// Value color while hovered.
    pub hovered_foreground: Color,
    /// Background while hovered.
    pub hovered_background: Color,
    /// Value color while disabled.
    pub disabled_foreground: Color,
}

impl CycleStyle {
    /// The no-theme starting point: plain ANSI colors that render on any
    /// terminal, with the List's three fallback backgrounds and readable text.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            foreground: Color::Gray,
            background: Color::Reset,
            focused_foreground: Color::Black,
            focused_background: Color::Reset,
            hovered_foreground: Color::Black,
            hovered_background: Color::DarkGray,
            disabled_foreground: Color::DarkGray,
        }
    }

    /// Colors derived from a theme: the List's three backdrops, with the value
    /// keeping the theme's foreground on them.
    #[must_use]
    pub fn from_theme(theme: &Theme) -> Self {
        let list = ListStyle::from_theme(theme);
        Self {
            foreground: list.foreground,
            background: list.background,
            focused_foreground: theme.foreground,
            focused_background: list.focused_background,
            hovered_foreground: theme.foreground,
            hovered_background: list.hovered_background,
            disabled_foreground: theme.muted_foreground,
        }
    }

    /// One paint pass's colors (see [`Self::from_theme`]). Disabled wins over
    /// hover, which wins over focus, which wins over rest.
    fn resolve(self, focused: bool, hovered: bool, disabled: bool) -> Style {
        if disabled {
            Style::default().fg(self.disabled_foreground)
        } else if hovered {
            Style::default()
                .fg(self.hovered_foreground)
                .bg(self.hovered_background)
        } else if focused {
            Style::default()
                .fg(self.focused_foreground)
                .bg(self.focused_background)
        } else {
            Style::default().fg(self.foreground).bg(self.background)
        }
    }
}

/// A cycle that only draws — an ordinary ratatui [`Widget`] with no focus,
/// events, or state. It paints the current option across `area`, the way the
/// closed [`SelectWidget`](crate::SelectWidget) paints its value: the widget
/// takes the one string it shows, and which option that is stays the caller's
/// business.
#[derive(Debug)]
pub struct CycleWidget<'a> {
    value: &'a str,
    focused: bool,
    hovered: bool,
    disabled: bool,
    theme: Option<Theme>,
    style: Option<CycleStyle>,
}

impl<'a> CycleWidget<'a> {
    /// A cycle showing `value`, its current option.
    #[must_use]
    pub const fn new(value: &'a str) -> Self {
        Self {
            value,
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
        if area.width == 0 || area.height == 0 {
            return;
        }
        let style = self
            .resolved_style()
            .resolve(self.focused, self.hovered, self.disabled);
        Line::from(filled_middle(self.value, area.width as usize))
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
/// <kbd>Ctrl+P</kbd>. Home and End step nothing: a ring has no ends.
/// Shift belongs to the app, so Shift+Space passes through untouched.
///
/// The row paints in a subtle field at rest, then brightens on hover or
/// focus, so a column of cycles reads as values without hiding that each one
/// is actionable.
///
/// The selection lives in app state and arrives through
/// [`selection`](Self::selection) as an index; without that binding the Cycle
/// paints but is not focusable and answers no events.
pub struct Cycle<S, M> {
    options: Vec<String>,
    selection: Option<(ReadSelectedFn<S>, OnChangeFn<M>)>,
    disabled: bool,
    style: Option<StyleFn>,
    /// Which edge of the declared area the value hugs.
    align: Alignment,
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
            options: options.into_iter().map(Into::into).collect(),
            selection: None,
            disabled: false,
            style: None,
            align: Alignment::Left,
            resolved_selected: 0,
            resolved_width: 0,
        }
    }

    /// Which edge of the declared area the value hugs; left by default.
    ///
    /// The Cycle stays exactly as wide as its value — this moves that value,
    /// paint and hit target together, within the area the app declares. With
    /// `Alignment::Right` a settings row is one declaration: the name painted
    /// at the left edge, the Cycle hugging the right.
    #[must_use]
    pub const fn align(mut self, align: Alignment) -> Self {
        self.align = align;
        self
    }

    /// The columns that fit every option — the widest value, so an area
    /// reserved from this never truncates and never shifts as the value
    /// cycles. What the Cycle paints each frame is narrower: exactly its
    /// current value.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.options
            .iter()
            .map(|option| text_width::display_width_u16(option))
            .max()
            .unwrap_or(0)
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

    /// The rect the current value paints in and answers events in: one column
    /// of the List-derived field surrounds either side of the value, hugging
    /// the [`align`](Self::align) edge of the declared area.
    fn value_area(&self, area: Rect) -> Rect {
        let width = self.resolved_width.saturating_add(2).min(area.width);
        let x = self.aligned_x(area, width);
        crate::geometry::fixed_height(Rect { x, width, ..area }, 1)
    }

    fn aligned_x(&self, area: Rect, width: u16) -> u16 {
        match self.align {
            Alignment::Left => area.x,
            Alignment::Center => area.x + (area.width - width) / 2,
            Alignment::Right => area.x + (area.width - width),
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

    /// The key map: the one horizontal step map every item control shares —
    /// Left/Right, their vi letters, and the readline chords, asked first
    /// because it owns Ctrl+N/Ctrl+P — then the commit keys, which advance.
    /// Home and End step nothing: a ring has no ends. Shift belongs to the
    /// app, so Shift+Space passes through untouched.
    fn handle_key(&self, key: KeyEvent) -> EventResult<M> {
        if let Some(step) = step_key(key, Axis::Horizontal) {
            return self.step(matches!(step, Step::Backward));
        }
        if key.modifiers.any() {
            return EventResult::Ignored;
        }
        match key.code {
            KeyCode::Enter | KeyCode::Char(' ') => self.step(false),
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
        // `prepare` clamped the selection, so this is `None` only with no
        // options at all — nothing to show, nothing to paint.
        let Some(value) = self.options.get(self.resolved_selected) else {
            return;
        };
        let style = resolve_style(self.style.as_deref(), ctx.theme, CycleStyle::from_theme);
        let widget = CycleWidget::new(value)
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

impl<S: 'static, M: 'static> MeasuredComponent<S, M> for Cycle<S, M> {
    fn measure(&self) -> Size {
        Size::new(self.width(), 1)
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

    /// A themed Cycle paints the List's ordinary and focused backgrounds. Pinned
    /// on the widget, where focus is a plain argument.
    #[test]
    fn focus_lays_the_lists_focused_background_over_the_resting_field() {
        let area = Rect::new(0, 0, 12, 1);
        let theme = Theme::default_dark();
        let list = ListStyle::from_theme(&theme);

        let mut rest = Buffer::empty(area);
        CycleWidget::new("Medium")
            .themed(&theme)
            .render(area, &mut rest);
        assert_eq!(
            rest.cell((3, 0)).expect("cell").bg,
            list.background,
            "rest must use the List's ordinary field"
        );

        let mut focused = Buffer::empty(area);
        CycleWidget::new("Medium")
            .themed(&theme)
            .focused(true)
            .render(area, &mut focused);
        let cell = focused.cell((3, 0)).expect("cell");
        assert_eq!(
            cell.bg, list.focused_background,
            "focus must use the List's focused field"
        );
        assert_ne!(cell.fg, cell.bg, "the value must read on the fill");
    }

    #[test]
    fn a_themed_cycle_uses_the_lists_backgrounds_in_every_focus_state() {
        let theme = Theme::default_dark();
        let cycle = CycleStyle::from_theme(&theme);
        let list = ListStyle::from_theme(&theme);

        assert_eq!(
            cycle.background, list.background,
            "an idle cycle should have the same well as an idle list"
        );
        assert_eq!(
            cycle.foreground, list.foreground,
            "an idle cycle should dim its value like an ordinary list row"
        );
        assert_eq!(
            cycle.focused_background, list.focused_background,
            "focused cycles and lists should use the same emphasis"
        );
        assert_eq!(
            cycle.hovered_background, list.hovered_background,
            "hovered cycles and lists should use the same emphasis"
        );
    }

    /// Disabled is the loudest state: no events, no traversal, no fill.
    #[test]
    fn a_disabled_cycle_is_inert() {
        let mut driver = driver();
        let state = State::default();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("size"),
                Cycle::new(SIZES)
                    .disabled(true)
                    .selection(|s: &State| s.size, Msg::Size),
                Rect::new(2, 2, 10, 1),
            );
        });

        assert!(matches!(
            driver.event(key(KeyCode::Char(' ')), &state),
            EventResult::Ignored
        ));
        assert!(matches!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 5, 2), &state),
            EventResult::Ignored
        ));
        assert!(matches!(
            driver.event(key(KeyCode::Tab), &state),
            EventResult::Ignored
        ));
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

    /// A right-aligned Cycle hugs the right edge of the area it is declared
    /// on — paint and hit target together: the value's columns answer, the
    /// empty columns to their left do not.
    #[test]
    fn a_right_aligned_cycle_answers_at_the_right_edge() {
        let mut driver = driver();
        let state = State::default();
        // Declared 20 wide at x=2; "Small" has one padded column on each side,
        // so its control occupies x 15..22.
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("size"),
                Cycle::new(SIZES)
                    .selection(|s: &State| s.size, Msg::Size)
                    .align(Alignment::Right),
                Rect::new(2, 2, 20, 1),
            );
        });

        assert_eq!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 21, 2), &state),
            EventResult::Emit(Msg::Size(1)),
            "the value's own columns must answer"
        );
        assert!(
            matches!(
                driver.event(mouse(MouseKind::Click(MouseButton::Left), 5, 2), &state),
                EventResult::Ignored
            ),
            "the empty columns left of the value must not"
        );
    }

    /// The component measures the columns that fit every option, so a layout
    /// reserved from it never truncates and never shifts as the value cycles.
    #[test]
    fn the_component_measures_its_widest_option() {
        let cycle: Cycle<State, Msg> = Cycle::new(SIZES);
        assert_eq!(cycle.width(), 6, "\"Medium\" is the widest option");
        assert_eq!(cycle.measure(), Size::new(6, 1));
    }

    #[test]
    fn a_cycle_pads_its_current_value_by_one_column_on_each_side() {
        let mut cycle: Cycle<State, Msg> = Cycle::new(SIZES)
            .selection(|state: &State| state.size, Msg::Size)
            .align(Alignment::Right);
        cycle.prepare(&State {
            size: 1,
            ..State::default()
        });

        assert_eq!(
            cycle.value_area(Rect::new(2, 2, 20, 1)),
            Rect::new(14, 2, 8, 1),
            "a six-column value receives one visible field column on either side"
        );
    }

    /// A stored index that outlived its options list clamps to the last
    /// option instead of panicking: stepping forward from it wraps to the
    /// first, which only the clamped position gives.
    #[test]
    fn an_out_of_range_selection_clamps_to_the_last_option() {
        let mut driver = driver();
        let state = State {
            size: 9,
            ..State::default()
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(key(KeyCode::Right), &state),
            EventResult::Emit(Msg::Size(0))
        );
    }

    /// A Cycle with no options declares, paints nothing, and answers nothing —
    /// there is no index to show and nothing to advance.
    #[test]
    fn a_cycle_with_no_options_is_inert() {
        let mut driver = driver();
        let state = State::default();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("size"),
                Cycle::new(Vec::<String>::new()).selection(|s: &State| s.size, Msg::Size),
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
}
