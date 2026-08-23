//! A labeled boolean control: the marker on the left, the label on the right.
//!
//! ```text
//! ☑ Vim bindings
//! ```
//!
//! The whole row is one hit target — a click on the label checks the box just
//! as a click on the marker does — and Enter or Space toggles while focused.
//! At rest a checkbox reads as text on the surface it sits on; hover and
//! focus lay the quiet ghost-button fill over the row, so keyboard users can
//! always find it and pointer users can see what they are about to flip.
//!
//! Because the checked and unchecked markers are yours to choose, the same
//! component covers switches and toggles: `[x]`/`[ ]` for an ASCII look, or
//! `[ON]`/`[off]` when the words are the point.
//!
//! State is app-owned. The component reads `checked` from app state through a
//! binding and emits a message each time the user asks to flip it.

use std::{fmt, rc::Rc};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Widget,
};

use crate::{
    Theme,
    color::{FOCUS_SHIFT, HOVER_SHIFT, away_from, dim},
    runtime::{
        Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, KeyEvent, MouseButton,
        MouseKind, PaintCtx, ScopeOptions,
    },
    text_width,
    theme::resolve_style,
};

// The default markers: the ballot-box pair most terminal fonts carry.
const CHECKED_MARKER: &str = "☑";
const UNCHECKED_MARKER: &str = "☐";

/// A checkbox's colors.
///
/// At rest the label carries [`foreground`](Self::foreground), the checked
/// marker [`checked_marker_color`](Self::checked_marker_color), and the
/// unchecked marker [`unchecked_marker_color`](Self::unchecked_marker_color),
/// all on nothing — the surface shows through. Hover and focus lay their
/// background over the row, the way a small ghost button raises; disabled
/// mutes everything and wins over both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckboxStyle {
    /// Label color at rest.
    pub foreground: Color,
    /// Label color while focused.
    pub focused_foreground: Color,
    /// Background while focused.
    pub focused_background: Color,
    /// Background while hovered.
    pub hovered_background: Color,
    /// Label and marker color while disabled.
    pub disabled_foreground: Color,
    /// Checked marker color.
    pub checked_marker_color: Color,
    /// Unchecked marker color.
    pub unchecked_marker_color: Color,
}

impl CheckboxStyle {
    /// The no-theme starting point: plain ANSI colors that render on any
    /// terminal.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            foreground: Color::Gray,
            focused_foreground: Color::Cyan,
            focused_background: Color::DarkGray,
            hovered_background: Color::DarkGray,
            disabled_foreground: Color::DarkGray,
            checked_marker_color: Color::Cyan,
            unchecked_marker_color: Color::Gray,
        }
    }

    /// Colors derived from a theme, solving the ghost button's fills: both
    /// climb away from the screen color, so they read as raised rather than
    /// pressed.
    #[must_use]
    pub fn from_theme(theme: &Theme) -> Self {
        let raised = away_from(theme.background);
        Self {
            foreground: theme.foreground,
            focused_foreground: theme.primary,
            focused_background: dim(theme.secondary, raised, FOCUS_SHIFT),
            hovered_background: dim(theme.secondary, raised, HOVER_SHIFT),
            disabled_foreground: theme.muted_foreground,
            checked_marker_color: theme.primary,
            unchecked_marker_color: theme.muted_foreground,
        }
    }

    /// One paint pass's colors (see [`Self::from_theme`]). Disabled wins over
    /// focus, which wins over hover, which wins over rest.
    fn resolve(
        self,
        checked: bool,
        focused: bool,
        hovered: bool,
        disabled: bool,
    ) -> ResolvedCheckboxStyle {
        let foreground = if disabled {
            self.disabled_foreground
        } else if focused {
            self.focused_foreground
        } else {
            self.foreground
        };
        let marker = if disabled {
            self.disabled_foreground
        } else if checked {
            self.checked_marker_color
        } else {
            self.unchecked_marker_color
        };
        let background = if disabled {
            None
        } else if focused {
            Some(self.focused_background)
        } else if hovered {
            Some(self.hovered_background)
        } else {
            None
        };
        ResolvedCheckboxStyle {
            foreground,
            marker,
            background,
        }
    }
}

/// One paint pass's resolved colors (see [`CheckboxStyle::resolve`]). A `None`
/// background leaves the surface behind the checkbox alone.
struct ResolvedCheckboxStyle {
    foreground: Color,
    marker: Color,
    background: Option<Color>,
}

/// A checkbox that only draws — an ordinary ratatui [`Widget`] with no focus,
/// events, or state. One instantiation is one checkbox.
///
/// The marker and label never paint a background, so the surface the checkbox
/// sits on shows through everywhere.
#[derive(Debug)]
pub struct CheckboxWidget<'a> {
    label: &'a str,
    checked: bool,
    checked_marker: &'a str,
    unchecked_marker: &'a str,
    focused: bool,
    hovered: bool,
    disabled: bool,
    theme: Option<Theme>,
    style: Option<CheckboxStyle>,
}

impl<'a> CheckboxWidget<'a> {
    /// A checkbox showing `label`, checked when `checked`.
    #[must_use]
    pub fn new(label: &'a str, checked: bool) -> Self {
        Self {
            label,
            checked,
            checked_marker: CHECKED_MARKER,
            unchecked_marker: UNCHECKED_MARKER,
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
    pub const fn style(mut self, style: CheckboxStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// The marker shown while checked.
    ///
    /// Any string works, including multi-character pairs like `[x]`; the
    /// layout measures what you pass.
    #[must_use]
    pub const fn checked_marker(mut self, marker: &'a str) -> Self {
        self.checked_marker = marker;
        self
    }

    /// The marker shown while unchecked.
    #[must_use]
    pub const fn unchecked_marker(mut self, marker: &'a str) -> Self {
        self.unchecked_marker = marker;
        self
    }

    /// Paint the focused label color and fill.
    #[must_use]
    pub const fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Paint the hovered fill.
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

    /// Columns this checkbox paints: marker, one space, label.
    #[must_use]
    pub fn width(&self) -> u16 {
        text_width::display_width_u16(self.marker())
            .saturating_add(1)
            .saturating_add(text_width::display_width_u16(self.label))
    }

    fn marker(&self) -> &'a str {
        if self.checked {
            self.checked_marker
        } else {
            self.unchecked_marker
        }
    }

    fn resolved_style(&self) -> CheckboxStyle {
        match (self.style, self.theme) {
            (Some(style), _) => style,
            (None, Some(theme)) => CheckboxStyle::from_theme(&theme),
            (None, None) => CheckboxStyle::fallback(),
        }
    }
}

impl Widget for CheckboxWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let style =
            self.resolved_style()
                .resolve(self.checked, self.focused, self.hovered, self.disabled);
        let mut marker_style = Style::default().fg(style.marker);
        let mut label_style = Style::default().fg(style.foreground);
        if let Some(background) = style.background {
            marker_style = marker_style.bg(background);
            label_style = label_style.bg(background);
            buf.set_style(area, Style::default().bg(background));
        }

        // Marker column, then one space, then as much label as fits.
        let marker = self.marker();
        let marker_width = text_width::display_width_u16(marker);
        let mut cursor = area.x;
        if area.width >= marker_width {
            Line::from(marker)
                .style(marker_style)
                .render(Rect::new(cursor, area.y, marker_width.max(1), 1), buf);
            cursor = cursor.saturating_add(marker_width);
        }
        let painted = cursor - area.x;
        if !self.label.is_empty() && area.width > painted {
            Span::raw(" ")
                .style(label_style)
                .render(Rect::new(cursor, area.y, 1, 1), buf);
            let available = area.width - painted - 1;
            let label = text_width::truncate_to_width(self.label, usize::from(available));
            Span::raw(label.to_owned())
                .style(label_style)
                .render(Rect::new(cursor + 1, area.y, available, 1), buf);
        }
    }
}

type ReadCheckedFn<S> = Rc<dyn Fn(&S) -> bool>;
type OnToggleFn<M> = Rc<dyn Fn(bool) -> M>;
type StyleFn = Rc<dyn Fn(&Theme) -> CheckboxStyle>;

/// A boolean control: marker left, label right, the whole row one hit target.
///
/// A click anywhere on the row toggles, as do <kbd>Enter</kbd> and
/// <kbd>Space</kbd> while focused. Hover paints nothing and nothing looks like
/// a button; focus recolors the label, which is the one state a keyboard user
/// needs to see.
///
/// The checked state lives in app state and arrives through
/// [`checked`](Self::checked); without that binding the checkbox paints but is
/// not focusable and answers no events. With the markers chosen freely, the
/// same component serves as a switch (`[ON]`/`[off]`) or an ASCII toggle
/// (`[x]`/`[ ]`) — see the [checkbox demo](https://github.com/kristoferlund/ratcn/tree/main/demos/checkbox).
pub struct Checkbox<S, M> {
    label: String,
    checked_marker: String,
    unchecked_marker: String,
    checked: Option<(ReadCheckedFn<S>, OnToggleFn<M>)>,
    disabled: bool,
    style: Option<StyleFn>,
    /// The bound checked value, resolved once per declaration.
    resolved_checked: bool,
}

impl<S, M> fmt::Debug for Checkbox<S, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Checkbox")
            .field("label", &self.label)
            .field("checked", &self.checked.is_some())
            .field("disabled", &self.disabled)
            .field("style", &self.style.is_some())
            .finish_non_exhaustive()
    }
}

impl<S, M> Checkbox<S, M> {
    /// Construct a checkbox labelled `label`.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            checked_marker: CHECKED_MARKER.to_owned(),
            unchecked_marker: UNCHECKED_MARKER.to_owned(),
            checked: None,
            disabled: false,
            style: None,
            resolved_checked: false,
        }
    }

    /// The marker shown while checked.
    ///
    /// Any string works, including multi-character pairs like `[x]`; the
    /// layout measures what you pass. Pair it with
    /// [`unchecked_marker`](Self::unchecked_marker) so both states read as one
    /// control — `[ON]`/`[off]` is a switch, `[x]`/`[ ]` an ASCII checkbox.
    #[must_use]
    pub fn checked_marker(mut self, marker: impl Into<String>) -> Self {
        self.checked_marker = marker.into();
        self
    }

    /// The marker shown while unchecked.
    #[must_use]
    pub fn unchecked_marker(mut self, marker: impl Into<String>) -> Self {
        self.unchecked_marker = marker.into();
        self
    }

    /// Bind the checked state and the message that flips it.
    ///
    /// `read` runs against current app state during rendering and event
    /// handling. `on_change` receives the requested state — `true` after a
    /// toggle onto checked, `false` after one onto unchecked. Without this
    /// binding the checkbox is not focusable and answers no events.
    #[must_use]
    pub fn checked(
        mut self,
        read: impl Fn(&S) -> bool + 'static,
        on_change: impl Fn(bool) -> M + 'static,
    ) -> Self {
        self.checked = Some((Rc::new(read), Rc::new(on_change)));
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
    pub fn style(mut self, style: impl Fn(&Theme) -> CheckboxStyle + 'static) -> Self {
        self.style = Some(Rc::new(style));
        self
    }

    fn is_bound(&self) -> bool {
        self.checked.is_some()
    }

    fn can_act(&self) -> bool {
        !self.disabled && self.is_bound()
    }

    /// The message that flips the bound state.
    fn toggle(&self) -> EventResult<M> {
        let Some((_, on_change)) = &self.checked else {
            return EventResult::Ignored;
        };
        EventResult::Emit(on_change(!self.resolved_checked))
    }
}

impl<S: 'static, M: 'static> Component<S, M> for Checkbox<S, M> {
    fn prepare(&mut self, state: &S) {
        self.resolved_checked = self.checked.as_ref().is_some_and(|(read, _)| read(state));
    }

    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, S, M>) {
        // Everything a checkbox is lives on its own node: the paint below and
        // the events answered here. There is nothing to declare inside it.
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, S>) {
        let style = resolve_style(self.style.as_deref(), ctx.theme, CheckboxStyle::from_theme);
        let widget = CheckboxWidget::new(&self.label, self.resolved_checked)
            .checked_marker(&self.checked_marker)
            .unchecked_marker(&self.unchecked_marker)
            .focused(ctx.focused())
            .hovered(ctx.hovered())
            .disabled(self.disabled)
            .style(style);
        ctx.widget(widget, ctx.area());
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
                MouseKind::Click(MouseButton::Left) => self.toggle(),
                _ => EventResult::Ignored,
            },
            Event::Key(key) => self.handle_key(*key),
            _ => EventResult::Ignored,
        }
    }

    fn scope_options(&self) -> ScopeOptions {
        ScopeOptions::default().focusable(self.can_act())
    }
}

impl<S, M> Checkbox<S, M> {
    /// The keys a checkbox answers: its commit keys, and nothing else.
    /// Modified keys belong to the app, so Shift+Enter passes through.
    fn handle_key(&self, key: KeyEvent) -> EventResult<M> {
        if key.modifiers.any() {
            return EventResult::Ignored;
        }
        match key.code {
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle(),
            _ => EventResult::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::runtime::{ChildId, FocusState, Modifiers, Ratcn, TabWrap};
    use crate::test_support::{Driver, key, key_with, mouse};

    #[derive(Default)]
    struct State {
        focus: FocusState,
        vim: bool,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Msg {
        Focus(FocusState),
        Vim(bool),
    }

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
                ChildId::Static("vim"),
                Checkbox::new("Vim bindings").checked(|state: &State| state.vim, Msg::Vim),
                Rect::new(2, 2, 20, 1),
            );
        });
    }

    /// The whole row is one hit target: the label toggles as well as the
    /// marker, because a two-cell-wide hit target would be a misery to aim at.
    #[test]
    fn a_click_on_the_label_toggles() {
        let mut driver = driver();
        let state = State::default();
        render(&mut driver, &state);

        let result = driver.event(mouse(MouseKind::Click(MouseButton::Left), 8, 2), &state);
        assert_eq!(result, EventResult::Emit(Msg::Vim(true)));
    }

    #[test]
    fn enter_and_space_toggle_and_modified_keys_pass_through() {
        let mut driver = driver();
        let state = State {
            vim: true,
            ..State::default()
        };
        render(&mut driver, &state);

        assert_eq!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::Vim(false))
        );
        assert_eq!(
            driver.event(key(KeyCode::Char(' ')), &state),
            EventResult::Emit(Msg::Vim(false))
        );
        // Modified keys belong to the app.
        assert_eq!(
            driver.event(
                key_with(
                    KeyCode::Enter,
                    Modifiers {
                        shift: true,
                        ..Modifiers::NONE
                    }
                ),
                &state
            ),
            EventResult::Ignored
        );
    }

    /// Hover and focus raise the row the ghost-button way: a fill appears
    /// where rest had none. Pinned on the widget, where both are arguments.
    #[test]
    fn hover_and_focus_lay_a_fill_and_rest_does_not() {
        let area = Rect::new(0, 0, 20, 1);
        let theme = Theme::default_dark();

        let mut rest = Buffer::empty(area);
        CheckboxWidget::new("Vim bindings", true)
            .themed(&theme)
            .render(area, &mut rest);
        assert_eq!(
            rest.cell((3, 0)).expect("cell").bg,
            Color::Reset,
            "rest painted a background"
        );

        let mut hovered = Buffer::empty(area);
        CheckboxWidget::new("Vim bindings", true)
            .hovered(true)
            .themed(&theme)
            .render(area, &mut hovered);
        assert_ne!(
            hovered.cell((3, 0)).expect("cell").bg,
            Color::Reset,
            "hover must show what the pointer is about to flip"
        );

        let mut focused = Buffer::empty(area);
        CheckboxWidget::new("Vim bindings", true)
            .focused(true)
            .themed(&theme)
            .render(area, &mut focused);
        assert_ne!(
            focused.cell((3, 0)).expect("cell").bg,
            Color::Reset,
            "focus must be findable by its fill"
        );
    }

    #[test]
    fn a_resting_checkbox_paints_no_background() {
        // Pinned on the widget: the runtime focuses the first focusable
        // control it renders, so a driven frame would already be focused.
        let theme = Theme::default_dark();
        let area = Rect::new(0, 0, 22, 1);
        let mut buffer = Buffer::empty(area);
        CheckboxWidget::new("Vim bindings", true)
            .themed(&theme)
            .render(area, &mut buffer);

        for column in 0..22u16 {
            let cell = buffer.cell((column, 0)).expect("cell");
            assert_eq!(
                cell.bg,
                Color::Reset,
                "column {column} painted a background"
            );
        }
    }

    #[test]
    fn the_markers_are_yours_to_choose() {
        let theme = Theme::default_dark();
        let area = Rect::new(0, 0, 24, 1);
        let mut buffer = Buffer::empty(area);

        CheckboxWidget::new("Vim bindings", true)
            .checked_marker("[x]")
            .unchecked_marker("[ ]")
            .themed(&theme)
            .render(area, &mut buffer);

        let row: String = (0..24u16)
            .map(|column| buffer.cell((column, 0)).expect("cell").symbol())
            .collect();
        assert!(row.starts_with("[x] Vim bindings"), "{row:?}");
    }

    #[test]
    fn an_unbound_checkbox_is_not_focusable_and_answers_nothing() {
        let mut driver = driver();
        let state = State::default();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("unbound"),
                Checkbox::new("Unbound"),
                Rect::new(2, 2, 20, 1),
            );
        });

        // Nothing here may take or answer focus: were the unbound checkbox
        // focusable, Tab would resolve into it and emit a focus message.
        assert!(matches!(
            driver.event(key(KeyCode::Tab), &state),
            EventResult::Ignored
        ));
        assert!(matches!(
            driver.event(key(KeyCode::Char(' ')), &state),
            EventResult::Ignored
        ));
        assert!(matches!(
            driver.event(mouse(MouseKind::Click(MouseButton::Left), 8, 2), &state),
            EventResult::Ignored
        ));
    }

    /// Bound checkboxes are first-class traversal stops: Tab walks between
    /// them like between any other controls.
    #[test]
    fn tab_walks_between_bound_checkboxes() {
        let mut driver = Driver::with(
            Ratcn::new()
                .focus(|state: &State| &state.focus, Msg::Focus)
                .tab_wrap(TabWrap::Wrap),
            30,
            6,
        );
        let state = State::default();
        driver.render(&state, |ctx| {
            ctx.component(
                ChildId::Static("first"),
                Checkbox::new("First").checked(|s: &State| s.vim, Msg::Vim),
                Rect::new(2, 1, 20, 1),
            );
            ctx.component(
                ChildId::Static("second"),
                Checkbox::new("Second").checked(|s: &State| s.vim, Msg::Vim),
                Rect::new(2, 3, 20, 1),
            );
        });

        let EventResult::Emit(msg) = driver.event(key(KeyCode::Tab), &state) else {
            panic!("Tab must reach the first checkbox");
        };
        let focus_after_first = match msg {
            Msg::Focus(focus) => focus,
            other => panic!("expected a focus message, got {other:?}"),
        };
        let state_after_first = State {
            focus: focus_after_first,
            ..State::default()
        };

        let EventResult::Emit(Msg::Focus(_)) = driver.event(key(KeyCode::Tab), &state_after_first)
        else {
            panic!("Tab must walk on to the second checkbox");
        };
    }
}
