//! A single-line text field: the value in a well, a block cursor while
//! focused, and an optional titled border around it.
//!
//! ```text
//! ┌Name──────────┐
//! │Ada Lovelace  │
//! └──────────────┘
//! ```
//!
//! The text is app-owned, like every other value here: the field reads an
//! [`InputState`] from app state and emits a new one for each keystroke.
//! Editing, cursor movement, selection, and horizontal scrolling belong to the
//! editor inside that state; what lives in this module is the look, the keys
//! the field takes and the ones it leaves to bubble, and how a paste becomes
//! one line.

use std::{fmt, rc::Rc};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Widget},
};

use crate::{
    Theme,
    color::{FIELD_FOCUS_SHIFT, FIELD_HOVER_SHIFT, away_from, dim},
    geometry::fixed_height,
    runtime::{
        Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, KeyEvent, PaintCtx,
        ScopeOptions,
    },
    text_edit::{Editor, InputState, editor_input},
    theme::resolve_style,
};

/// An input's colors.
///
/// At rest [`foreground`](Self::foreground) sits on
/// [`background`](Self::background). Focus and hover swap the background, with
/// hover beating focus; disabled mutes the text and wins over both. Invalid
/// recolors the text, the border, and the title, and is independent of the
/// rest: it says something about the content, not about the interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputStyle {
    /// Text color, and the title's.
    pub foreground: Color,
    /// Background at rest.
    pub background: Color,
    /// Background while focused.
    pub focused_background: Color,
    /// Background while hovered.
    pub hovered_background: Color,
    /// Placeholder color.
    pub placeholder_foreground: Color,
    /// Text and title color while disabled.
    pub disabled_foreground: Color,
    /// Text, border, and title color while invalid.
    pub invalid_foreground: Color,
    /// The character under the cursor.
    pub cursor_foreground: Color,
    /// The cursor block.
    pub cursor_background: Color,
    /// Selected text.
    pub selection_foreground: Color,
    /// Background behind selected text.
    pub selection_background: Color,
    /// The border a [`title`](Input::title) draws.
    pub border: Color,
}

impl InputStyle {
    /// The no-theme starting point: plain ANSI colors that render on any
    /// terminal, with the terminal's own text and background for the field.
    #[must_use]
    pub const fn fallback() -> Self {
        Self {
            foreground: Color::Reset,
            background: Color::Reset,
            focused_background: Color::Reset,
            hovered_background: Color::Reset,
            placeholder_foreground: Color::DarkGray,
            disabled_foreground: Color::DarkGray,
            invalid_foreground: Color::Red,
            cursor_foreground: Color::Black,
            cursor_background: Color::Gray,
            selection_foreground: Color::White,
            selection_background: Color::Blue,
            border: Color::DarkGray,
        }
    }

    /// Colors derived from a theme: the field well, shifted away from the
    /// page background for focus and further for hover, the way a List's
    /// rows are.
    #[must_use]
    pub fn from_theme(theme: &Theme) -> Self {
        let away = away_from(theme.background);
        Self {
            foreground: theme.foreground,
            background: theme.field,
            focused_background: dim(theme.field, away, FIELD_FOCUS_SHIFT),
            hovered_background: dim(theme.field, away, FIELD_HOVER_SHIFT),
            placeholder_foreground: theme.muted_foreground,
            disabled_foreground: theme.muted_foreground,
            invalid_foreground: theme.destructive,
            cursor_foreground: theme.background,
            cursor_background: theme.cursor,
            selection_foreground: theme.primary_foreground,
            selection_background: theme.primary,
            border: theme.border,
        }
    }

    /// One paint pass's styles. Disabled wins over hover, which wins over
    /// focus, which wins over rest. The cursor shows only on a focused,
    /// enabled field; everywhere else it takes the text's style and vanishes.
    #[allow(
        clippy::fn_params_excessive_bools,
        reason = "the four independent states a field paints; none combine into an enum"
    )]
    fn resolve(self, focused: bool, hovered: bool, disabled: bool, invalid: bool) -> ResolvedStyle {
        let background = if disabled {
            self.background
        } else if hovered {
            self.hovered_background
        } else if focused {
            self.focused_background
        } else {
            self.background
        };
        let foreground = if disabled {
            self.disabled_foreground
        } else if invalid {
            self.invalid_foreground
        } else {
            self.foreground
        };
        let text = Style::default().fg(foreground).bg(background);
        ResolvedStyle {
            text,
            placeholder: Style::default()
                .fg(self.placeholder_foreground)
                .bg(background),
            cursor: if focused && !disabled {
                Style::default()
                    .fg(self.cursor_foreground)
                    .bg(self.cursor_background)
            } else {
                text
            },
            selection: Style::default()
                .fg(self.selection_foreground)
                .bg(self.selection_background),
            border: Style::default().fg(if invalid {
                self.invalid_foreground
            } else {
                self.border
            }),
            title: Style::default().fg(foreground),
        }
    }
}

/// The styles one paint pass draws with.
struct ResolvedStyle {
    text: Style,
    placeholder: Style,
    cursor: Style,
    selection: Style,
    border: Style,
    title: Style,
}

/// The rows a field occupies in `area`: one, or three with a titled border.
/// Shorter than that and there is no field at all.
const fn field_rows(area: Rect, titled: bool) -> Rect {
    fixed_height(area, if titled { 3 } else { 1 })
}

/// A single-line text field that only draws — an ordinary ratatui [`Widget`]
/// with no focus, events, or state of its own.
///
/// It paints the [`InputState`] it is given: the text, scrolled to keep the
/// cursor in view, the cursor when [`focused`](Self::focused), and any
/// selection. Driving the editing is the caller's business; [`Input`] is the
/// field that does it for you, and paints through this widget.
#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the four independent states a field paints; none combine into an enum"
)]
pub struct InputWidget<'a> {
    editor: Editor<'static>,
    placeholder: &'a str,
    title: Option<&'a str>,
    mask_char: Option<char>,
    focused: bool,
    hovered: bool,
    disabled: bool,
    invalid: bool,
    style: InputStyle,
}

impl<'a> InputWidget<'a> {
    /// A field showing `state`.
    #[must_use]
    pub fn new(state: &InputState) -> Self {
        Self {
            editor: state.editor().clone(),
            placeholder: "",
            title: None,
            mask_char: None,
            focused: false,
            hovered: false,
            disabled: false,
            invalid: false,
            style: InputStyle::fallback(),
        }
    }

    /// Take colors from `theme`.
    #[must_use]
    pub fn themed(mut self, theme: &Theme) -> Self {
        self.style = InputStyle::from_theme(theme);
        self
    }

    /// Use these exact colors, ignoring any theme.
    #[must_use]
    pub const fn style(mut self, style: InputStyle) -> Self {
        self.style = style;
        self
    }

    /// The muted text shown while the field is empty and not focused. A
    /// focused empty field shows its cursor instead.
    #[must_use]
    pub const fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Draw a border around the field with `title` on it. The field then
    /// takes three rows instead of one.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// Paint every character as `mask_char`, for a secret. Only the paint
    /// changes: the state keeps the text as typed.
    #[must_use]
    pub const fn mask_char(mut self, mask_char: char) -> Self {
        self.mask_char = Some(mask_char);
        self
    }

    /// Paint the focused background and show the cursor.
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

    /// Paint muted, with no cursor.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Paint the text, border, and title in the invalid color.
    #[must_use]
    pub const fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Paint the field and hand back the editor it was painted from, which
    /// now knows the view it was drawn in. [`Input`] keeps it, so the next
    /// edit starts from what is on screen.
    fn paint(mut self, area: Rect, buf: &mut Buffer) -> Editor<'static> {
        let style = self
            .style
            .resolve(self.focused, self.hovered, self.disabled, self.invalid);
        let area = field_rows(area, self.title.is_some());
        let field = match self.title {
            Some(title) => {
                let block = Block::bordered()
                    .border_style(style.border)
                    .title(Line::styled(title, style.title));
                let inner = block.inner(area);
                block.render(area, buf);
                inner
            }
            None => area,
        };
        buf.set_style(field, style.text);

        match self.mask_char {
            Some(mask_char) => self.editor.set_mask_char(mask_char),
            None => self.editor.clear_mask_char(),
        }
        self.editor.set_style(style.text);
        self.editor.set_cursor_style(style.cursor);
        self.editor.set_cursor_line_style(Style::default());
        self.editor.set_selection_style(style.selection);
        if self.editor.is_empty() && !self.focused {
            Line::styled(self.placeholder, style.placeholder).render(field, buf);
        } else {
            (&self.editor).render(field, buf);
        }
        self.editor
    }
}

impl Widget for InputWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.paint(area, buf);
    }
}

type ReadValueFn<S> = Rc<dyn Fn(&S) -> &InputState>;
type OnChangeFn<M> = Rc<dyn Fn(InputState) -> M>;
type OnSubmitFn<M> = Rc<dyn Fn() -> M>;
type StyleFn = Rc<dyn Fn(&Theme) -> InputStyle>;

/// A single-line text field the user types in.
///
/// The text lives in app state as an [`InputState`] and arrives through
/// [`value`](Self::value); every edit and every cursor movement emits a new
/// state for the app to store. Without that binding the field paints empty,
/// is not focusable, and answers no events.
///
/// The editor's keys all work — arrows, <kbd>Home</kbd>/<kbd>End</kbd>,
/// <kbd>Ctrl</kbd>+<kbd>←</kbd>/<kbd>→</kbd> by word, <kbd>Shift</kbd> with a
/// movement to select, <kbd>Ctrl+W</kbd>, <kbd>Ctrl+K</kbd>, and the rest of
/// the readline set. <kbd>Enter</kbd> emits [`on_submit`](Self::on_submit)
/// and never inserts a line break. <kbd>Tab</kbd>, <kbd>Esc</kbd>, the
/// function keys, and the vertical keys are left to bubble, as is any chord
/// that changes nothing here, so focus traversal, an enclosing dialog, and
/// the app's shortcuts keep working around a focused field. A paste is
/// flattened to one line.
///
/// ```
/// # use ratcn::{Input, InputState};
/// # struct AppState { name: InputState }
/// # enum Msg { Name(InputState), Save }
/// let input = Input::new()
///     .value(|state: &AppState| &state.name, Msg::Name)
///     .title("Name")
///     .placeholder("Ada Lovelace")
///     .on_submit(|| Msg::Save);
/// # let _: Input<AppState, Msg> = input;
/// ```
pub struct Input<S, M> {
    value: Option<(ReadValueFn<S>, OnChangeFn<M>)>,
    on_submit: Option<OnSubmitFn<M>>,
    placeholder: String,
    title: Option<String>,
    mask_char: Option<char>,
    disabled: bool,
    invalid: bool,
    style: Option<StyleFn>,
    /// The editor the last paint drew, with the version of the state it was
    /// cloned from. It alone knows how far the view is scrolled.
    painted: Option<(u64, Editor<'static>)>,
}

impl<S, M> fmt::Debug for Input<S, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Input")
            .field("value", &self.value.is_some())
            .field("on_submit", &self.on_submit.is_some())
            .field("placeholder", &self.placeholder)
            .field("title", &self.title)
            .field("mask_char", &self.mask_char)
            .field("disabled", &self.disabled)
            .field("invalid", &self.invalid)
            .field("style", &self.style.is_some())
            .finish_non_exhaustive()
    }
}

impl<S, M> Default for Input<S, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S, M> Input<S, M> {
    /// An empty, unbound field. Bind it with [`value`](Self::value).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            value: None,
            on_submit: None,
            placeholder: String::new(),
            title: None,
            mask_char: None,
            disabled: false,
            invalid: false,
            style: None,
            painted: None,
        }
    }

    /// Bind the text and the message that replaces it.
    ///
    /// `read` returns the state the field shows. `on_change` receives the
    /// state after each edit or cursor movement: store it as it is. Without
    /// this binding the field is not focusable and answers no events.
    #[must_use]
    pub fn value(
        mut self,
        read: impl Fn(&S) -> &InputState + 'static,
        on_change: impl Fn(InputState) -> M + 'static,
    ) -> Self {
        self.value = Some((Rc::new(read), Rc::new(on_change)));
        self
    }

    /// The message unmodified <kbd>Enter</kbd> emits. Without it Enter
    /// bubbles, to a dialog's default action for instance.
    #[must_use]
    pub fn on_submit(mut self, on_submit: impl Fn() -> M + 'static) -> Self {
        self.on_submit = Some(Rc::new(on_submit));
        self
    }

    /// The muted text shown while the field is empty and not focused. A
    /// focused empty field shows its cursor instead.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Draw a border around the field with `title` on it. The field then
    /// takes three rows instead of one.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Paint every character as `mask_char`, for a secret. Only the paint
    /// changes: the state keeps the text as typed.
    #[must_use]
    pub const fn mask_char(mut self, mask_char: char) -> Self {
        self.mask_char = Some(mask_char);
        self
    }

    /// Paint muted, out of focus traversal, ignoring every event.
    #[must_use]
    pub const fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Paint the text, border, and title in the invalid color. Purely a
    /// look: the field stays editable, and what counts as invalid is the
    /// app's decision.
    #[must_use]
    pub const fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Supply exact colors, taking precedence over the theme.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> InputStyle + 'static) -> Self {
        self.style = Some(Rc::new(style));
        self
    }

    /// The editor an event edits: the painted one while the state it was
    /// painted from is still the app's, since only it knows how far the view
    /// is scrolled. A state that changed since the paint is edited itself —
    /// it descends from a painted editor through the event that produced it.
    fn editor(&self, state: &InputState) -> Editor<'static> {
        match &self.painted {
            Some((version, editor)) if *version == state.version() => editor.clone(),
            _ => state.editor().clone(),
        }
    }

    /// The key policy. Enter submits and never reaches the editor; the keys
    /// in [`bubbles`] are not the field's; everything else is the editor's.
    /// A key that changes something emits the new state. One that changes
    /// nothing is consumed when it is plain typing or movement — Left at the
    /// start of the text must not walk out into the enclosing component —
    /// and bubbles when it is a chord, which is how the app's own shortcuts
    /// pass through a focused field.
    fn handle_key(&self, key: KeyEvent, state: &InputState) -> Option<EventResult<InputState>> {
        if bubbles(key) {
            return None;
        }
        let input = editor_input(&key)?;
        let mut editor = self.editor(state);
        let before = (
            editor.cursor(),
            editor.selection_range(),
            editor.yank_text(),
        );
        let chord = input.ctrl || input.alt;
        let modified = editor.input(input);
        let after = (
            editor.cursor(),
            editor.selection_range(),
            editor.yank_text(),
        );
        if modified || before != after {
            Some(EventResult::Emit(InputState::edited(editor)))
        } else if chord {
            None
        } else {
            Some(EventResult::Consumed)
        }
    }

    /// Insert a paste as one line.
    fn handle_paste(&self, text: &str, state: &InputState) -> Option<EventResult<InputState>> {
        let text = single_line(text);
        if text.is_empty() {
            return None;
        }
        let mut editor = self.editor(state);
        editor.insert_str(text);
        Some(EventResult::Emit(InputState::edited(editor)))
    }
}

impl<S: 'static, M: 'static> Component<S, M> for Input<S, M> {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, S, M>) {
        // Everything an Input is lives on its own node: the paint below and
        // the events answered here. There is nothing to declare inside it.
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, S>) {
        let unbound;
        let state = if let Some((read, _)) = &self.value {
            read(ctx.state())
        } else {
            unbound = InputState::default();
            &unbound
        };
        let style = resolve_style(self.style.as_deref(), ctx.theme, InputStyle::from_theme);
        let mut widget = InputWidget::new(state)
            .placeholder(&self.placeholder)
            .focused(ctx.focused())
            .hovered(ctx.hovered())
            .disabled(self.disabled)
            .invalid(self.invalid)
            .style(style);
        widget.title = self.title.as_deref();
        widget.mask_char = self.mask_char;
        let editor = ctx.with_buffer(ctx.area(), |area, buf| widget.paint(area, buf));
        self.painted = Some((state.version(), editor));
    }

    fn handle_event(
        &mut self,
        event: &Event,
        state: &S,
        _ctx: &mut EventCtx<'_>,
    ) -> EventResult<M> {
        let Some((read, on_change)) = &self.value else {
            return EventResult::Ignored;
        };
        if self.disabled {
            return EventResult::Ignored;
        }
        let edited = match event {
            Event::Key(key) if key.code == KeyCode::Enter => {
                return match &self.on_submit {
                    Some(on_submit) if !key.modifiers.any() => EventResult::Emit(on_submit()),
                    _ => EventResult::Ignored,
                };
            }
            Event::Key(key) => self.handle_key(*key, read(state)),
            Event::Paste(text) => self.handle_paste(text, read(state)),
            _ => None,
        };
        match edited {
            Some(EventResult::Emit(next)) => EventResult::Emit(on_change(next)),
            Some(EventResult::Consumed) => EventResult::Consumed,
            Some(EventResult::Ignored) | None => EventResult::Ignored,
        }
    }

    fn scope_options(&self, _state: &S) -> ScopeOptions {
        ScopeOptions::default().focusable(self.value.is_some() && !self.disabled)
    }

    fn interaction_area(&self, area: Rect, _state: &S) -> Rect {
        field_rows(area, self.title.is_some())
    }
}

/// The keys a field leaves alone. Tab and `BackTab` belong to focus traversal,
/// Esc and the function keys to enclosing components and the app, and one
/// line has no vertical movement to make. Ctrl+M and a raw line break would
/// insert a newline; Ctrl+U and Ctrl+R are undo and redo, which a state
/// replaced on every keystroke does not keep.
fn bubbles(key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Esc
        | KeyCode::F(_)
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Char('\n' | '\r') => true,
        KeyCode::Char(char) if key.modifiers.ctrl && !key.modifiers.alt => {
            matches!(char.to_ascii_lowercase(), 'm' | 'u' | 'r')
        }
        _ => false,
    }
}

/// A paste as one line: each line break and each tab becomes a space, and
/// every other control character is dropped.
fn single_line(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter_map(|char| match char {
            '\n' | '\r' | '\t' => Some(' '),
            char if char.is_control() => None,
            char => Some(char),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{ChildId, FocusState, Modifiers, MouseButton, MouseKind, Ratcn};
    use crate::test_support::{Driver, key, key_with, mouse, styled_snapshot};

    #[derive(Default)]
    struct State {
        focus: FocusState,
        name: InputState,
    }

    #[derive(Debug)]
    enum Msg {
        Focus(FocusState),
        Name(InputState),
        Submit,
    }

    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };

    /// The field every test declares unless it says otherwise: six columns
    /// wide, so a ten-character value has to scroll.
    const FIELD: Rect = Rect::new(0, 0, 6, 1);

    fn driver() -> Driver<State, Msg> {
        Driver::with(
            Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
            12,
            3,
        )
    }

    fn state(value: &str) -> State {
        State {
            name: InputState::new(value),
            ..State::default()
        }
    }

    fn input() -> Input<State, Msg> {
        Input::new()
            .value(|state: &State| &state.name, Msg::Name)
            .on_submit(|| Msg::Submit)
    }

    fn render_with(
        driver: &mut Driver<State, Msg>,
        state: &State,
        input: fn() -> Input<State, Msg>,
    ) {
        driver.render(state, |ctx| {
            ctx.component(ChildId::Static("name"), input(), FIELD);
        });
    }

    fn render(driver: &mut Driver<State, Msg>, state: &State) {
        render_with(driver, state, input);
    }

    /// Route one event and store what it emits, as the app's update would.
    fn send(driver: &mut Driver<State, Msg>, state: &mut State, event: Event) {
        match driver.event(event, state) {
            EventResult::Emit(Msg::Name(name)) => state.name = name,
            EventResult::Emit(Msg::Focus(focus)) => state.focus = focus,
            other => panic!("expected a new state, got {other:?}"),
        }
    }

    /// What the field's own columns show.
    fn field(driver: &Driver<State, Msg>) -> String {
        driver.row(0).chars().take(FIELD.width.into()).collect()
    }

    /// The whole contract in one pass: each key takes the state the app
    /// holds and emits the next one, which the app stores.
    #[test]
    fn typing_backspace_and_movement_each_emit_the_next_state() {
        let mut driver = driver();
        let mut state = state("ac");
        render(&mut driver, &state);

        send(&mut driver, &mut state, key(KeyCode::Left));
        assert_eq!((state.name.value(), state.name.cursor()), ("ac", 1));
        send(&mut driver, &mut state, key(KeyCode::Char('b')));
        assert_eq!((state.name.value(), state.name.cursor()), ("abc", 2));
        send(&mut driver, &mut state, key(KeyCode::End));
        send(&mut driver, &mut state, key(KeyCode::Backspace));
        assert_eq!((state.name.value(), state.name.cursor()), ("ab", 2));
        send(&mut driver, &mut state, key_with(KeyCode::Char('A'), SHIFT));
        assert_eq!(state.name.value(), "abA");
        send(&mut driver, &mut state, key(KeyCode::Home));
        send(&mut driver, &mut state, key(KeyCode::Delete));
        assert_eq!((state.name.value(), state.name.cursor()), ("bA", 0));
    }

    /// Shift with a movement selects, and typing replaces the selection:
    /// the selection has to survive the round trip through app state.
    #[test]
    fn a_selection_survives_the_round_trip_through_app_state() {
        let mut driver = driver();
        let mut state = state("hello");
        render(&mut driver, &state);

        send(&mut driver, &mut state, key_with(KeyCode::Home, SHIFT));
        assert!(state.name.editor().is_selecting());
        render(&mut driver, &state);
        send(&mut driver, &mut state, key(KeyCode::Char('x')));
        assert_eq!(state.name.value(), "x");
    }

    /// The regression the painted editor exists for. A long value in a
    /// narrow field is scrolled so the cursor at its end shows. Moving the
    /// cursor one step left keeps it well inside the view, so nothing may
    /// scroll — but only the editor that was painted knows the view is
    /// scrolled at all. Editing the state's own editor, which has never been
    /// drawn, re-derives the view from column zero and shifts it by one.
    #[test]
    fn moving_the_cursor_inside_the_view_does_not_scroll_it() {
        let mut driver = driver();
        let mut state = state("abcdefghij");
        render(&mut driver, &state);
        assert_eq!(field(&driver), "fghij ", "scrolled to the end cursor");

        send(&mut driver, &mut state, key(KeyCode::Left));
        render(&mut driver, &state);
        assert_eq!(field(&driver), "fghij ", "the view must not shift");
        assert_eq!(
            driver.cell(4, 0).bg,
            InputStyle::from_theme(&Theme::default_dark()).cursor_background,
            "the cursor moved onto the last character"
        );
    }

    /// Two keys can arrive between frames. The second finds a state newer
    /// than the paint, and must build on it rather than on the painted
    /// editor — and that state still carries the painted view.
    #[test]
    fn two_events_between_renders_compose_and_keep_the_view() {
        let mut driver = driver();
        let mut state = state("abcdefghij");
        render(&mut driver, &state);

        send(&mut driver, &mut state, key(KeyCode::Left));
        send(&mut driver, &mut state, key(KeyCode::Left));
        assert_eq!(state.name.cursor(), 8, "the second Left built on the first");
        send(&mut driver, &mut state, key(KeyCode::Char('X')));
        assert_eq!(state.name.value(), "abcdefghXij");

        render(&mut driver, &state);
        assert_eq!(field(&driver), "fghXij", "the view must not shift");
    }

    /// A state the app replaced wholesale — a form reset — is not the one
    /// that was painted, whatever the painted editor still holds.
    #[test]
    fn a_state_the_app_replaced_wins_over_the_painted_editor() {
        let mut driver = driver();
        let mut state = state("old");
        render(&mut driver, &state);

        state.name = InputState::new("");
        send(&mut driver, &mut state, key(KeyCode::Char('n')));
        assert_eq!(state.name.value(), "n");
    }

    /// Enter is the submit key of a one-line field. It must never reach the
    /// editor, where it would split the line.
    #[test]
    fn enter_submits_and_never_inserts_a_line_break() {
        let mut driver = driver();
        let state = state("name");
        render(&mut driver, &state);

        assert!(matches!(
            driver.event(key(KeyCode::Enter), &state),
            EventResult::Emit(Msg::Submit)
        ));
        for event in [
            key_with(KeyCode::Enter, SHIFT),
            key_with(KeyCode::Char('m'), CTRL),
            key(KeyCode::Char('\n')),
            key(KeyCode::Char('\r')),
        ] {
            assert!(
                matches!(driver.event(event.clone(), &state), EventResult::Ignored),
                "{event:?} must not edit"
            );
        }

        render_with(&mut driver, &state, || {
            Input::new().value(|state: &State| &state.name, Msg::Name)
        });
        assert!(
            matches!(
                driver.event(key(KeyCode::Enter), &state),
                EventResult::Ignored
            ),
            "with nothing to submit, Enter belongs to whatever encloses the field"
        );
    }

    /// A field that swallowed Tab would trap focus, and one that swallowed
    /// Esc would keep its dialog open. Undo has no history to act on, and an
    /// unknown chord is the app's shortcut.
    #[test]
    fn keys_that_are_not_the_fields_bubble() {
        let mut driver = Driver::<State, Msg>::new(12, 3);
        let state = state("name");
        render(&mut driver, &state);

        for event in [
            key(KeyCode::Tab),
            key(KeyCode::BackTab),
            key(KeyCode::Esc),
            key(KeyCode::F(5)),
            key(KeyCode::Up),
            key(KeyCode::Down),
            key_with(KeyCode::Char('u'), CTRL),
            key_with(KeyCode::Char('r'), CTRL),
            key_with(KeyCode::Char('s'), CTRL),
        ] {
            assert!(
                matches!(driver.event(event.clone(), &state), EventResult::Ignored),
                "{event:?} must bubble"
            );
        }
    }

    /// Left at the start changes nothing, but it is still the field's key:
    /// bubbling it would switch the tab or move the list the field sits in.
    #[test]
    fn a_movement_key_at_the_edge_is_consumed() {
        let mut driver = driver();
        let state = state("");
        render(&mut driver, &state);

        for code in [KeyCode::Left, KeyCode::Right, KeyCode::Backspace] {
            assert!(
                matches!(driver.event(key(code), &state), EventResult::Consumed),
                "{code:?}"
            );
        }
    }

    /// A paste can carry anything; the field holds one line.
    #[test]
    fn a_paste_is_flattened_to_one_line() {
        let mut driver = driver();
        let mut state = state("a");
        render(&mut driver, &state);

        send(
            &mut driver,
            &mut state,
            Event::Paste(" b\r\nc\nd\te\u{7}\u{1b}f".to_owned()),
        );
        assert_eq!(state.name.value(), "a b c d ef");
        assert!(
            matches!(
                driver.event(Event::Paste("\u{7}".to_owned()), &state),
                EventResult::Ignored
            ),
            "a paste with nothing to insert is not an edit"
        );
    }

    /// Disabled is the loudest state: no keys, no paste, no focus.
    #[test]
    fn a_disabled_field_ignores_input() {
        let mut driver = driver();
        let state = state("name");
        render_with(&mut driver, &state, || input().disabled(true));

        for event in [
            key(KeyCode::Char('x')),
            key(KeyCode::Enter),
            key(KeyCode::Tab),
            Event::Paste("x".to_owned()),
            mouse(MouseKind::Down(MouseButton::Left), 1, 0),
        ] {
            assert!(
                matches!(driver.event(event.clone(), &state), EventResult::Ignored),
                "{event:?}"
            );
        }
        assert_eq!(
            driver.cell(4, 0).bg,
            InputStyle::from_theme(&Theme::default_dark()).background,
            "a disabled field shows no cursor"
        );
    }

    #[test]
    fn an_unbound_field_is_not_focusable_and_answers_nothing() {
        let mut driver = driver();
        let state = State::default();
        render_with(&mut driver, &state, Input::new);

        for event in [key(KeyCode::Tab), key(KeyCode::Char('x'))] {
            assert!(matches!(driver.event(event, &state), EventResult::Ignored));
        }
    }

    /// Mouse editing is not the field's yet, so a press is left to the
    /// runtime, whose fallback focuses what was pressed.
    #[test]
    fn a_press_focuses_the_field() {
        let mut driver = driver();
        let state = State {
            focus: FocusState::none(),
            ..state("name")
        };
        render(&mut driver, &state);

        let EventResult::Emit(Msg::Focus(focus)) =
            driver.event(mouse(MouseKind::Down(MouseButton::Left), 1, 0), &state)
        else {
            panic!("a press must move focus to the field");
        };
        assert_eq!(focus.path(), [ChildId::Static("name")]);
    }

    /// A mask must hide the text's shape as well as its characters: every
    /// character is one mask cell, so wide characters scroll by the mask's
    /// width. Measured by their own, this field would scroll past its text
    /// and paint blank.
    #[test]
    fn a_masked_field_paints_only_the_mask_and_scrolls_by_its_width() {
        let mut driver = driver();
        let mut state = state("日本語日本語");
        render_with(&mut driver, &state, || input().mask_char('*'));
        assert_eq!(field(&driver), "***** ", "six masks, scrolled by one");

        send(&mut driver, &mut state, key(KeyCode::Char('字')));
        render_with(&mut driver, &state, || input().mask_char('*'));
        assert_eq!(field(&driver), "***** ");
        assert_eq!(
            state.name.value(),
            "日本語日本語字",
            "the state keeps the text"
        );
    }

    #[test]
    fn an_unfocused_field_hides_the_cursor() {
        let theme = Theme::default_dark();
        let style = InputStyle::from_theme(&theme);
        let state = InputState::new("ab");
        let paint = |focused| {
            let mut buffer = Buffer::empty(FIELD);
            InputWidget::new(&state)
                .themed(&theme)
                .focused(focused)
                .render(FIELD, &mut buffer);
            buffer.cell((2, 0)).expect("the cell after the text").bg
        };

        assert_eq!(paint(true), style.cursor_background);
        assert_eq!(paint(false), style.background);
    }

    /// The placeholder says what belongs in an empty field, and gives way
    /// to the cursor once the field is focused and to the text once there
    /// is any.
    #[test]
    fn the_placeholder_shows_in_an_empty_unfocused_field() {
        let row = |state: &InputState, focused| {
            let area = Rect::new(0, 0, 8, 1);
            let mut buffer = Buffer::empty(area);
            InputWidget::new(state)
                .placeholder("Name")
                .focused(focused)
                .render(area, &mut buffer);
            buffer
                .content
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        };

        assert_eq!(row(&InputState::default(), false), "Name    ");
        assert_eq!(row(&InputState::default(), true), "        ");
        assert_eq!(row(&InputState::new("Ada"), false), "Ada     ");
    }

    /// A titled field is three rows, and the whole frame is the field: the
    /// runtime routes and focuses on exactly what is painted.
    #[test]
    fn a_titled_field_draws_its_border_and_types_inside_it() {
        let mut driver = driver();
        let mut state = state("");
        let declare = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    ChildId::Static("name"),
                    input().title("Name"),
                    Rect::new(0, 0, 12, 3),
                );
            });
        };
        declare(&mut driver, &state);
        send(&mut driver, &mut state, key(KeyCode::Char('A')));
        declare(&mut driver, &state);

        assert_eq!(driver.row(0), "┌Name──────┐");
        assert_eq!(driver.row(1), "│A         │");
        assert_eq!(driver.row(2), "└──────────┘");
    }

    /// The look of every state, pinned: rest, focused, hovered over focus,
    /// disabled over everything, and invalid.
    #[test]
    fn every_state_paints_its_own_look() {
        let theme = Theme::default_dark();
        let state = InputState::new("Ada");
        let area = Rect::new(0, 0, 6, 5);
        let mut buffer = Buffer::empty(area);
        let widget = || InputWidget::new(&state).themed(&theme);
        let row = |y| Rect::new(0, y, 6, 1);

        widget().render(row(0), &mut buffer);
        widget().focused(true).render(row(1), &mut buffer);
        widget()
            .focused(true)
            .hovered(true)
            .render(row(2), &mut buffer);
        widget()
            .focused(true)
            .hovered(true)
            .invalid(true)
            .disabled(true)
            .render(row(3), &mut buffer);
        widget().invalid(true).render(row(4), &mut buffer);

        assert_eq!(
            styled_snapshot(&buffer),
            "Ada   |\n\
             Ada   |\n\
             Ada   |\n\
             Ada   |\n\
             Ada   |\n\
             aaaaaa\n\
             bbbcbb\n\
             dddcdd\n\
             eeeeee\n\
             ffffff\n\
             a: #FAFAFA on #1F1F1F NONE\n\
             b: #FAFAFA on #282828 NONE\n\
             c: #0A0A0A on #737373 NONE\n\
             d: #FAFAFA on #313131 NONE\n\
             e: #A1A1A1 on #1F1F1F NONE\n\
             f: #FF6467 on #1F1F1F NONE\n"
        );
    }
}
