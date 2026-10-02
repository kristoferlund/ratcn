//! A multi-line text field: the text in a well as tall as the area it is
//! given, a block cursor while focused, and an optional titled border around
//! it.
//!
//! ```text
//! ┌Notes─────────┐
//! │Met Ada today.│
//! │She counts.   │
//! └──────────────┘
//! ```
//!
//! The text is app-owned, like every other value here: the field reads a
//! [`TextAreaState`] from app state and emits a new one for each keystroke.
//! Editing, cursor movement, selection, wrapping, and scrolling belong to the
//! editor inside that state; what lives in this module is the look, the keys
//! the field takes and the ones it leaves to bubble, and how a paste goes in.

use std::{fmt, rc::Rc};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Text},
    widgets::{Block, Widget},
};

use crate::{
    Theme,
    color::{FIELD_FOCUS_SHIFT, FIELD_HOVER_SHIFT, away_from, dim},
    runtime::{
        Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, KeyEvent, PaintCtx,
        ScopeOptions,
    },
    text_edit::{Editor, TextAreaState, WrapMode, editor_input},
    theme::resolve_style,
};

/// A text area's colors.
///
/// At rest [`foreground`](Self::foreground) sits on
/// [`background`](Self::background). Focus and hover swap the background, with
/// hover beating focus; disabled mutes the text and wins over both. Invalid
/// recolors the text, the border, and the title, and is independent of the
/// rest: it says something about the content, not about the interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextAreaStyle {
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
    /// The border a [`title`](TextArea::title) draws.
    pub border: Color,
}

impl TextAreaStyle {
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

/// A multi-line text field that only draws — an ordinary ratatui [`Widget`]
/// with no focus, events, or state of its own.
///
/// It paints the [`TextAreaState`] it is given over the whole area: the
/// text, scrolled to keep the cursor in view, the cursor when
/// [`focused`](Self::focused), and any selection. Driving the editing is the
/// caller's business; [`TextArea`] is the field that does it for you, and
/// paints through this widget.
#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the four independent states a field paints; none combine into an enum"
)]
pub struct TextAreaWidget<'a> {
    editor: Editor<'static>,
    placeholder: &'a str,
    title: Option<&'a str>,
    wrap_mode: WrapMode,
    focused: bool,
    hovered: bool,
    disabled: bool,
    invalid: bool,
    style: TextAreaStyle,
}

impl<'a> TextAreaWidget<'a> {
    /// A field showing `state`.
    #[must_use]
    pub fn new(state: &TextAreaState) -> Self {
        Self {
            editor: state.editor().clone(),
            placeholder: "",
            title: None,
            wrap_mode: WrapMode::None,
            focused: false,
            hovered: false,
            disabled: false,
            invalid: false,
            style: TextAreaStyle::fallback(),
        }
    }

    /// Take colors from `theme`.
    #[must_use]
    pub fn themed(mut self, theme: &Theme) -> Self {
        self.style = TextAreaStyle::from_theme(theme);
        self
    }

    /// Use these exact colors, ignoring any theme.
    #[must_use]
    pub const fn style(mut self, style: TextAreaStyle) -> Self {
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

    /// Draw a border around the field with `title` on it. The text then
    /// loses a row and a column on each side.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// How a line longer than the field is shown. [`WrapMode::None`], the
    /// default, scrolls it sideways; the other modes break it over several
    /// rows. Only the paint changes: the state's lines stay as typed.
    #[must_use]
    pub const fn wrap_mode(mut self, wrap_mode: WrapMode) -> Self {
        self.wrap_mode = wrap_mode;
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
    /// now knows the view it was drawn in: how far it is scrolled, how tall a
    /// page is, and where its lines wrap. [`TextArea`] keeps it, so the next
    /// edit starts from what is on screen.
    fn paint(mut self, area: Rect, buf: &mut Buffer) -> Editor<'static> {
        let style = self
            .style
            .resolve(self.focused, self.hovered, self.disabled, self.invalid);
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

        if self.editor.wrap_mode() != self.wrap_mode {
            self.editor.set_wrap_mode(self.wrap_mode);
        }
        self.editor.set_style(style.text);
        self.editor.set_cursor_style(style.cursor);
        self.editor.set_cursor_line_style(Style::default());
        self.editor.set_selection_style(style.selection);
        if self.editor.is_empty() && !self.focused {
            Text::styled(self.placeholder, style.placeholder).render(field, buf);
        } else {
            (&self.editor).render(field, buf);
        }
        self.editor
    }
}

impl Widget for TextAreaWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.paint(area, buf);
    }
}

type ReadValueFn<S> = Rc<dyn Fn(&S) -> &TextAreaState>;
type OnChangeFn<M> = Rc<dyn Fn(TextAreaState) -> M>;
type OnSubmitFn<M> = Rc<dyn Fn() -> M>;
type StyleFn = Rc<dyn Fn(&Theme) -> TextAreaStyle>;

/// A multi-line text field the user types in.
///
/// The multi-line sibling of [`Input`](crate::Input), bound the same way: the
/// text lives in app state as a [`TextAreaState`] and arrives through
/// [`value`](Self::value); every edit and every cursor movement emits a new
/// state for the app to store. Without that binding the field paints empty,
/// is not focusable, and answers no events. The field fills the area it is
/// declared in and scrolls to keep the cursor in view.
///
/// The editor's keys all work — arrows, <kbd>Home</kbd>/<kbd>End</kbd>,
/// <kbd>Page Up</kbd>/<kbd>Page Down</kbd>, <kbd>Ctrl</kbd>+<kbd>←</kbd>/<kbd>→</kbd>
/// by word, <kbd>Shift</kbd> with a movement to select, <kbd>Ctrl+W</kbd>,
/// <kbd>Ctrl+K</kbd>, and the rest of the readline set. <kbd>Enter</kbd>
/// inserts a line break, so <kbd>Ctrl+Enter</kbd> is the one that emits
/// [`on_submit`](Self::on_submit). <kbd>Tab</kbd>, <kbd>Esc</kbd>, and the
/// function keys are left to bubble, as is any chord that changes nothing
/// here, so focus traversal, an enclosing dialog, and the app's shortcuts
/// keep working around a focused field. A paste goes in as it is, line breaks
/// included.
///
/// ```
/// # use ratcn::{TextArea, TextAreaState, text_edit::WrapMode};
/// # struct AppState { notes: TextAreaState }
/// # enum Msg { Notes(TextAreaState), Save }
/// let notes = TextArea::new()
///     .value(|state: &AppState| &state.notes, Msg::Notes)
///     .title("Notes")
///     .placeholder("What happened today?")
///     .wrap_mode(WrapMode::WordOrGlyph)
///     .on_submit(|| Msg::Save);
/// # let _: TextArea<AppState, Msg> = notes;
/// ```
pub struct TextArea<S, M> {
    value: Option<(ReadValueFn<S>, OnChangeFn<M>)>,
    on_submit: Option<OnSubmitFn<M>>,
    placeholder: String,
    title: Option<String>,
    wrap_mode: WrapMode,
    disabled: bool,
    invalid: bool,
    style: Option<StyleFn>,
    /// The editor the last paint drew, with the version of the state it was
    /// cloned from. It alone knows how far the view is scrolled, how tall it
    /// is, and where its lines wrap.
    painted: Option<(u64, Editor<'static>)>,
}

impl<S, M> fmt::Debug for TextArea<S, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextArea")
            .field("value", &self.value.is_some())
            .field("on_submit", &self.on_submit.is_some())
            .field("placeholder", &self.placeholder)
            .field("title", &self.title)
            .field("wrap_mode", &self.wrap_mode)
            .field("disabled", &self.disabled)
            .field("invalid", &self.invalid)
            .field("style", &self.style.is_some())
            .finish_non_exhaustive()
    }
}

impl<S, M> Default for TextArea<S, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S, M> TextArea<S, M> {
    /// An empty, unbound field. Bind it with [`value`](Self::value).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            value: None,
            on_submit: None,
            placeholder: String::new(),
            title: None,
            wrap_mode: WrapMode::None,
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
        read: impl Fn(&S) -> &TextAreaState + 'static,
        on_change: impl Fn(TextAreaState) -> M + 'static,
    ) -> Self {
        self.value = Some((Rc::new(read), Rc::new(on_change)));
        self
    }

    /// The message <kbd>Ctrl+Enter</kbd> emits. Plain <kbd>Enter</kbd> stays
    /// a line break. Without it Ctrl+Enter bubbles.
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

    /// Draw a border around the field with `title` on it. The text then
    /// loses a row and a column on each side.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// How a line longer than the field is shown. [`WrapMode::None`], the
    /// default, scrolls it sideways; the other modes break it over several
    /// rows, and <kbd>↑</kbd>/<kbd>↓</kbd> then move by the rows on screen.
    /// The state's lines stay as typed.
    #[must_use]
    pub const fn wrap_mode(mut self, wrap_mode: WrapMode) -> Self {
        self.wrap_mode = wrap_mode;
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
    pub fn style(mut self, style: impl Fn(&Theme) -> TextAreaStyle + 'static) -> Self {
        self.style = Some(Rc::new(style));
        self
    }

    /// The editor an event edits: the painted one while the state it was
    /// painted from is still the app's, since only it knows the view — its
    /// scroll, its height, its wrapped rows. A state that changed since the
    /// paint is edited itself — it descends from a painted editor through
    /// the event that produced it.
    fn editor(&self, state: &TextAreaState) -> Editor<'static> {
        match &self.painted {
            Some((version, editor)) if *version == state.version() => editor.clone(),
            _ => state.editor().clone(),
        }
    }

    /// The key policy. The keys in [`bubbles`] are not the field's;
    /// everything else is the editor's, Enter included. A key that changes
    /// something emits the new state. One that changes nothing is consumed when it is plain
    /// typing or movement — Up on the first line must not walk out into the
    /// enclosing component — and bubbles when it is a chord, which is how
    /// the app's own shortcuts pass through a focused field.
    fn handle_key(
        &self,
        key: KeyEvent,
        state: &TextAreaState,
    ) -> Option<EventResult<TextAreaState>> {
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
            Some(EventResult::Emit(TextAreaState::edited(editor)))
        } else if chord {
            None
        } else {
            Some(EventResult::Consumed)
        }
    }

    /// Insert a paste as it is, at the cursor.
    fn handle_paste(
        &self,
        text: &str,
        state: &TextAreaState,
    ) -> Option<EventResult<TextAreaState>> {
        if text.is_empty() {
            return None;
        }
        let mut editor = self.editor(state);
        editor.insert_str(line_feeds(text));
        Some(EventResult::Emit(TextAreaState::edited(editor)))
    }
}

impl<S: 'static, M: 'static> Component<S, M> for TextArea<S, M> {
    fn declare(&mut self, _ctx: &mut DeclareCtx<'_, S, M>) {
        // Everything a TextArea is lives on its own node: the paint below and
        // the events answered here. There is nothing to declare inside it.
    }

    fn paint(&mut self, ctx: &mut PaintCtx<'_, S>) {
        let unbound;
        let state = if let Some((read, _)) = &self.value {
            read(ctx.state())
        } else {
            unbound = TextAreaState::default();
            &unbound
        };
        let style = resolve_style(self.style.as_deref(), ctx.theme, TextAreaStyle::from_theme);
        let mut widget = TextAreaWidget::new(state)
            .placeholder(&self.placeholder)
            .wrap_mode(self.wrap_mode)
            .focused(ctx.focused())
            .hovered(ctx.hovered())
            .disabled(self.disabled)
            .invalid(self.invalid)
            .style(style);
        widget.title = self.title.as_deref();
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
            Event::Key(key) if key.code == KeyCode::Enter && key.modifiers.ctrl => {
                return match &self.on_submit {
                    Some(on_submit) if !key.modifiers.alt && !key.modifiers.shift => {
                        EventResult::Emit(on_submit())
                    }
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
}

/// The keys a field leaves alone. Tab and `BackTab` belong to focus traversal,
/// Esc and the function keys to enclosing components and the app. Ctrl+U and
/// Ctrl+R are undo and redo, which a state replaced on every keystroke does
/// not keep.
fn bubbles(key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc | KeyCode::F(_) => true,
        KeyCode::Char(char) if key.modifiers.ctrl && !key.modifiers.alt => {
            matches!(char.to_ascii_lowercase(), 'u' | 'r')
        }
        _ => false,
    }
}

/// A paste with one kind of line break. Terminals send a pasted break as
/// `\r\n`, `\n`, or a bare `\r`; left in a line, a carriage return would be
/// text the user cannot see.
fn line_feeds(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{ChildId, FocusState, Modifiers, MouseButton, MouseKind, Ratcn};
    use crate::test_support::{Driver, key, key_with, mouse, styled_snapshot};
    use crate::text_edit::CursorMove;

    #[derive(Default)]
    struct State {
        focus: FocusState,
        notes: TextAreaState,
    }

    #[derive(Debug)]
    enum Msg {
        Focus(FocusState),
        Notes(TextAreaState),
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
    /// by three rows, so ten lines have to scroll and a long one to wrap.
    const FIELD: Rect = Rect::new(0, 0, 6, 3);

    /// Ten lines, `0` to `9`.
    const TEN_LINES: &str = "0\n1\n2\n3\n4\n5\n6\n7\n8\n9";

    fn driver() -> Driver<State, Msg> {
        Driver::with(
            Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
            12,
            5,
        )
    }

    /// A state holding `value`, with the cursor at the end of the text.
    fn state(value: &str) -> State {
        State {
            notes: TextAreaState::new(value),
            ..State::default()
        }
    }

    /// The same, with the cursor at the very start.
    fn state_at_top(value: &str) -> State {
        let mut editor = TextAreaState::new(value).editor().clone();
        editor.move_cursor(CursorMove::Top);
        editor.move_cursor(CursorMove::Head);
        State {
            notes: TextAreaState::edited(editor),
            ..State::default()
        }
    }

    fn textarea() -> TextArea<State, Msg> {
        TextArea::new()
            .value(|state: &State| &state.notes, Msg::Notes)
            .on_submit(|| Msg::Submit)
    }

    fn wrapping() -> TextArea<State, Msg> {
        textarea().wrap_mode(WrapMode::Word)
    }

    fn render_with(
        driver: &mut Driver<State, Msg>,
        state: &State,
        textarea: fn() -> TextArea<State, Msg>,
    ) {
        driver.render(state, |ctx| {
            ctx.component(ChildId::Static("notes"), textarea(), FIELD);
        });
    }

    fn render(driver: &mut Driver<State, Msg>, state: &State) {
        render_with(driver, state, textarea);
    }

    /// Route one event and store what it emits, as the app's update would.
    fn send(driver: &mut Driver<State, Msg>, state: &mut State, event: Event) {
        match driver.event(event, state) {
            EventResult::Emit(Msg::Notes(notes)) => state.notes = notes,
            EventResult::Emit(Msg::Focus(focus)) => state.focus = focus,
            other => panic!("expected a new state, got {other:?}"),
        }
    }

    /// What the field's own cells show, row by row.
    fn field(driver: &Driver<State, Msg>) -> Vec<String> {
        (0..FIELD.height)
            .map(|row| driver.row(row).chars().take(FIELD.width.into()).collect())
            .collect()
    }

    /// The whole contract in one pass: each key takes the state the app
    /// holds and emits the next one, which the app stores. Enter is text
    /// here, in every form short of the submit chord.
    #[test]
    fn typing_and_enter_each_emit_the_next_state() {
        let mut driver = driver();
        let mut state = state("ab");
        render(&mut driver, &state);

        send(&mut driver, &mut state, key(KeyCode::Char('c')));
        assert_eq!(
            (state.notes.value().as_str(), state.notes.cursor()),
            ("abc", (0, 3))
        );
        send(&mut driver, &mut state, key(KeyCode::Enter));
        assert_eq!(state.notes.lines(), ["abc", ""]);
        send(&mut driver, &mut state, key(KeyCode::Char('d')));
        send(&mut driver, &mut state, key_with(KeyCode::Enter, SHIFT));
        assert_eq!(state.notes.lines(), ["abc", "d", ""]);
        send(&mut driver, &mut state, key(KeyCode::Backspace));
        send(&mut driver, &mut state, key(KeyCode::Up));
        send(&mut driver, &mut state, key(KeyCode::Home));
        send(&mut driver, &mut state, key(KeyCode::Delete));
        assert_eq!(
            (state.notes.value().as_str(), state.notes.cursor()),
            ("bc\nd", (0, 0))
        );

        render(&mut driver, &state);
        assert_eq!(field(&driver), ["bc    ", "d     ", "      "]);
    }

    /// Enter is taken, so a multi-line field submits on Ctrl+Enter — and
    /// that chord must never also split the line.
    #[test]
    fn ctrl_enter_submits_and_inserts_nothing() {
        let mut driver = driver();
        let state = state("note");
        render(&mut driver, &state);

        assert!(matches!(
            driver.event(key_with(KeyCode::Enter, CTRL), &state),
            EventResult::Emit(Msg::Submit)
        ));
        assert!(
            matches!(
                driver.event(
                    key_with(
                        KeyCode::Enter,
                        Modifiers {
                            shift: true,
                            ..CTRL
                        }
                    ),
                    &state
                ),
                EventResult::Ignored
            ),
            "another chord on Enter is neither a submit nor a line break"
        );

        render_with(&mut driver, &state, || {
            TextArea::new().value(|state: &State| &state.notes, Msg::Notes)
        });
        assert!(
            matches!(
                driver.event(key_with(KeyCode::Enter, CTRL), &state),
                EventResult::Ignored
            ),
            "with nothing to submit, the chord belongs to whatever encloses the field"
        );
    }

    /// A field that swallowed Tab would trap focus — and the editor would
    /// insert one — and one that swallowed Esc would keep its dialog open.
    /// Undo has no history to act on, and an unknown chord is the app's
    /// shortcut.
    #[test]
    fn keys_that_are_not_the_fields_bubble() {
        let mut driver = Driver::<State, Msg>::new(12, 5);
        let state = state("note");
        render(&mut driver, &state);

        for event in [
            key(KeyCode::Tab),
            key(KeyCode::BackTab),
            key(KeyCode::Esc),
            key(KeyCode::F(5)),
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

    /// Up on the first line changes nothing, but it is still the field's
    /// key: bubbling it would move the list or the form the field sits in.
    #[test]
    fn a_movement_key_at_the_edge_is_consumed() {
        let mut driver = driver();
        let state = state("");
        render(&mut driver, &state);

        for code in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::Backspace,
        ] {
            assert!(
                matches!(driver.event(key(code), &state), EventResult::Consumed),
                "{code:?}"
            );
        }
    }

    /// A paste keeps its lines, whichever line break the terminal sent, and
    /// no carriage return is left behind as invisible text.
    #[test]
    fn a_paste_keeps_its_line_breaks() {
        let mut driver = driver();
        let mut state = state("a");
        render(&mut driver, &state);

        send(
            &mut driver,
            &mut state,
            Event::Paste("b\r\nc\nd\re\tf".to_owned()),
        );
        assert_eq!(state.notes.lines(), ["ab", "c", "d", "e\tf"]);
        assert_eq!(state.notes.cursor(), (3, 3));
        assert!(
            matches!(
                driver.event(Event::Paste(String::new()), &state),
                EventResult::Ignored
            ),
            "a paste with nothing to insert is not an edit"
        );
    }

    /// Disabled is the loudest state: no keys, no paste, no focus.
    #[test]
    fn a_disabled_field_ignores_input() {
        let mut driver = driver();
        let state = state("note");
        render_with(&mut driver, &state, || textarea().disabled(true));

        for event in [
            key(KeyCode::Char('x')),
            key(KeyCode::Enter),
            key_with(KeyCode::Enter, CTRL),
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
            TextAreaStyle::from_theme(&Theme::default_dark()).background,
            "a disabled field shows no cursor"
        );
    }

    #[test]
    fn an_unbound_field_is_not_focusable_and_answers_nothing() {
        let mut driver = driver();
        let state = State::default();
        render_with(&mut driver, &state, TextArea::new);

        for event in [key(KeyCode::Tab), key(KeyCode::Char('x'))] {
            assert!(matches!(driver.event(event, &state), EventResult::Ignored));
        }
    }

    /// The regression the painted editor exists for. Ten lines in three
    /// rows are scrolled so the cursor on the last one shows. Moving the
    /// cursor up one line keeps it inside the view, so nothing may scroll —
    /// but only the editor that was painted knows the view is scrolled at
    /// all. Editing the state's own editor, which has never been drawn,
    /// re-derives the view from the first line and shifts it by one.
    #[test]
    fn moving_the_cursor_inside_the_view_does_not_scroll_it() {
        let mut driver = driver();
        let mut state = state(TEN_LINES);
        render(&mut driver, &state);
        assert_eq!(field(&driver)[0], "7     ", "scrolled to the last line");

        send(&mut driver, &mut state, key(KeyCode::Up));
        assert_eq!(state.notes.cursor(), (8, 1));
        render(&mut driver, &state);
        assert_eq!(
            field(&driver),
            ["7     ", "8     ", "9     "],
            "the view must not jump"
        );
    }

    /// A page is as tall as the field was painted, which the state's own
    /// editor has never been told: from it, `PageDown` goes nowhere.
    #[test]
    fn page_down_moves_by_the_painted_height() {
        let mut driver = driver();
        let mut state = state_at_top(TEN_LINES);
        render(&mut driver, &state);
        assert_eq!(field(&driver), ["0     ", "1     ", "2     "]);

        send(&mut driver, &mut state, key(KeyCode::PageDown));
        assert_eq!(state.notes.cursor().0, 3, "one page of three rows");
        render(&mut driver, &state);
        assert_eq!(field(&driver), ["3     ", "4     ", "5     "]);
    }

    /// With soft wrap, Down moves by the rows on screen, not by the lines
    /// in the text. Where a line wraps depends on the width it was painted
    /// at, which again only the painted editor knows: for the state's own
    /// editor this is one line with nothing below it.
    #[test]
    fn down_moves_by_visual_row_in_a_wrapped_line() {
        let mut driver = driver();
        let mut state = state_at_top("aaa bbb ccc");
        render_with(&mut driver, &state, wrapping);
        assert_eq!(field(&driver), ["aaa   ", "bbb   ", "ccc   "]);

        send(&mut driver, &mut state, key(KeyCode::Down));
        assert_eq!(
            state.notes.cursor(),
            (0, 4),
            "onto the second row of the same line"
        );
        assert_eq!(state.notes.lines().len(), 1, "wrapping changes no text");
    }

    /// Two keys can arrive between frames. The second finds a state newer
    /// than the paint, and must build on it rather than on the painted
    /// editor — and that state still carries the painted view.
    #[test]
    fn two_events_between_renders_compose_and_keep_the_view() {
        let mut driver = driver();
        let mut state = state(TEN_LINES);
        render(&mut driver, &state);

        send(&mut driver, &mut state, key(KeyCode::Up));
        send(&mut driver, &mut state, key(KeyCode::Up));
        assert_eq!(
            state.notes.cursor(),
            (7, 1),
            "the second Up built on the first"
        );
        send(&mut driver, &mut state, key(KeyCode::Char('X')));
        assert_eq!(state.notes.lines()[7], "7X");

        render(&mut driver, &state);
        assert_eq!(
            field(&driver),
            ["7X    ", "8     ", "9     "],
            "the view must not jump"
        );
    }

    /// A state the app replaced wholesale — a form reset — is not the one
    /// that was painted, whatever the painted editor still holds.
    #[test]
    fn a_state_the_app_replaced_wins_over_the_painted_editor() {
        let mut driver = driver();
        let mut state = state("old\ntext");
        render(&mut driver, &state);

        state.notes = TextAreaState::new("");
        send(&mut driver, &mut state, key(KeyCode::Char('n')));
        assert_eq!(state.notes.value(), "n");
    }

    #[test]
    fn an_unfocused_field_hides_the_cursor() {
        let theme = Theme::default_dark();
        let style = TextAreaStyle::from_theme(&theme);
        let state = TextAreaState::new("ab\ncd");
        let paint = |focused| {
            let mut buffer = Buffer::empty(FIELD);
            TextAreaWidget::new(&state)
                .themed(&theme)
                .focused(focused)
                .render(FIELD, &mut buffer);
            buffer.cell((2, 1)).expect("the cell after the text").bg
        };

        assert_eq!(paint(true), style.cursor_background);
        assert_eq!(paint(false), style.background);
    }

    /// The placeholder says what belongs in an empty field, and gives way
    /// to the cursor once the field is focused and to the text once there
    /// is any.
    #[test]
    fn the_placeholder_shows_in_an_empty_unfocused_field() {
        let rows = |state: &TextAreaState, focused| {
            let area = Rect::new(0, 0, 5, 2);
            let mut buffer = Buffer::empty(area);
            TextAreaWidget::new(state)
                .placeholder("Notes\nhere")
                .focused(focused)
                .render(area, &mut buffer);
            buffer
                .content
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        };

        assert_eq!(rows(&TextAreaState::default(), false), "Noteshere ");
        assert_eq!(rows(&TextAreaState::default(), true), "          ");
        assert_eq!(rows(&TextAreaState::new("Ada"), false), "Ada       ");
    }

    /// A titled field keeps its text inside the border, and the whole frame
    /// is the field: the runtime routes and focuses on exactly what is
    /// painted.
    #[test]
    fn a_titled_field_draws_its_border_and_types_inside_it() {
        let mut driver = driver();
        let mut state = state("");
        let declare = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    ChildId::Static("notes"),
                    textarea().title("Notes"),
                    Rect::new(0, 0, 12, 4),
                );
            });
        };
        declare(&mut driver, &state);
        send(&mut driver, &mut state, key(KeyCode::Char('A')));
        send(&mut driver, &mut state, key(KeyCode::Enter));
        send(&mut driver, &mut state, key(KeyCode::Char('B')));
        declare(&mut driver, &state);

        assert_eq!(driver.row(0), "┌Notes─────┐");
        assert_eq!(driver.row(1), "│A         │");
        assert_eq!(driver.row(2), "│B         │");
        assert_eq!(driver.row(3), "└──────────┘");
    }

    /// The look of every state, pinned: rest, focused, hovered over focus,
    /// disabled over everything, and invalid.
    #[test]
    fn every_state_paints_its_own_look() {
        let theme = Theme::default_dark();
        let state = TextAreaState::new("Ada");
        let area = Rect::new(0, 0, 6, 5);
        let mut buffer = Buffer::empty(area);
        let widget = || TextAreaWidget::new(&state).themed(&theme);
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
