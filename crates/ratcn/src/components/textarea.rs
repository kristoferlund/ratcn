//! A multi-line text field: the text in a well as tall as the area it is
//! given, a block cursor while focused, and an optional titled border around
//! it.
//!
//! ```text
//! ┌Notes───────────┐
//! │ Met Ada today. │
//! │ She counts.    │
//! └────────────────┘
//! ```
//!
//! The text is app-owned, like every other value here: the field reads a
//! [`TextAreaState`] from app state and emits a new one for each keystroke.
//! Editing, cursor movement, selection, wrapping, and scrolling belong to the
//! editor inside that state; what lives in this module is the look, the keys
//! the field takes and the ones it leaves to bubble, how a paste goes in, and
//! what the mouse does.

use std::{fmt, rc::Rc};

use ratatui::{
    buffer::Buffer,
    layout::{Margin, Position, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Widget},
};

use crate::{
    Theme,
    color::{FIELD_FOCUS_SHIFT, FIELD_HOVER_SHIFT, away_from, dim},
    runtime::{
        Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, KeyEvent, MouseButton,
        MouseEvent, MouseKind, PaintCtx, ScopeOptions, ScrollDirection,
    },
    text_edit::{
        CursorMove, DataCursor, Editor, TextAreaState, WrapMode, cursor_at, editor_input,
        is_editor_binding, select_dragged,
    },
    theme::resolve_style,
};

/// A text area's colors.
///
/// At rest [`foreground`](Self::foreground) sits on
/// [`background`](Self::background). Focus and hover swap the background, with
/// hover beating focus; disabled mutes the text and wins over both. Invalid
/// recolors the text, the border, and the title, and is independent of focus
/// and hover: it says something about the content, not about the
/// interaction. Disabled wins over it too, since a field the user cannot edit
/// cannot be fixed.
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
            border: Style::default().fg(if invalid && !disabled {
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

/// The cells of `area` the field's background fills: all of it, inside the
/// border when there is one.
fn well(area: Rect, titled: bool) -> Rect {
    if titled {
        Block::bordered().inner(area)
    } else {
        area
    }
}

/// The cells of `area` the text is drawn in: the well, inset a column on
/// each side, as a Select's trigger insets its value. Paint draws the editor
/// and the placeholder here and the mouse is read against it, so the three
/// cannot drift apart.
fn text_rect(area: Rect, titled: bool) -> Rect {
    well(area, titled).inner(Margin::new(1, 0))
}

/// How many rows one notch of the wheel scrolls.
const WHEEL_ROWS: u16 = 3;

/// A multi-line text field that only draws — an ordinary ratatui [`Widget`]
/// with no focus, events, or state of its own.
///
/// It paints the [`TextAreaState`] it is given over the whole area: the
/// text, scrolled to keep the cursor in view, the cursor when
/// [`focused`](Self::focused), and any selection. Driving the editing is the
/// caller's business, from the editor [`paint`](Self::paint) hands back;
/// [`TextArea`] is the field that does it for you, and paints through this
/// widget.
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
            wrap_mode: WrapMode::WordOrGlyph,
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

    /// The muted text shown while the field is empty, focused or not, where
    /// the text would start, a line to a row. A focused field shows its
    /// cursor on its first character.
    #[must_use]
    pub const fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Draw a border around the field with `title` on it, which takes a row
    /// and a column on each side from the text.
    #[must_use]
    pub const fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    /// How a line longer than the field is shown. [`WrapMode::WordOrGlyph`],
    /// the default, breaks it over several rows, at a word boundary where
    /// there is one; [`WrapMode::None`] scrolls it sideways instead. Only the
    /// paint changes: the state's lines stay as typed.
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
    /// page is, and where its lines wrap.
    ///
    /// Rendering as a [`Widget`] throws that editor away. A loop that drives
    /// the editing itself edits this one instead, and stores the result with
    /// [`TextAreaState::from_editor`], so the next paint scrolls from what is
    /// on screen. Which keys reach the editor is the loop's own policy; this
    /// is the least of one, and [`TextArea`] has the whole of it.
    ///
    /// ```
    /// use ratatui::{buffer::Buffer, layout::Rect};
    /// use ratcn::{
    ///     TextAreaState, TextAreaWidget,
    ///     runtime::{KeyCode, KeyEvent},
    ///     text_edit::{Editor, editor_input, is_editor_binding},
    /// };
    ///
    /// // Beside the state, the editor the last paint handed back, with the
    /// // version of the state it was painted from.
    /// let mut state = TextAreaState::new("Met Ada today.\nShe counts.");
    /// let mut painted: Option<(u64, Editor<'static>)> = None;
    ///
    /// // Each frame:
    /// let area = Rect::new(0, 0, 20, 4);
    /// let mut buf = Buffer::empty(area);
    /// let editor = TextAreaWidget::new(&state).focused(true).paint(area, &mut buf);
    /// painted = Some((state.version(), editor));
    ///
    /// // When a key arrives:
    /// fn on_key(
    ///     key: KeyEvent,
    ///     state: &mut TextAreaState,
    ///     painted: &mut Option<(u64, Editor<'static>)>,
    /// ) {
    ///     match key.code {
    ///         // Enter is a line break, so submitting takes a chord. Ctrl+J is
    ///         // how a terminal that sends a line feed reports Ctrl+Enter, and
    ///         // some report it as `J`: either way it must never reach the
    ///         // editor, whose Ctrl+J deletes to the line start.
    ///         KeyCode::Enter | KeyCode::Char('j' | 'J') if key.modifiers.ctrl => { /* submit */ }
    ///         // Focus traversal and the enclosing view.
    ///         KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc => {}
    ///         _ => {
    ///             if let Some(input) = editor_input(&key).filter(is_editor_binding) {
    ///                 // The painted editor, while the state is still the one
    ///                 // it was painted from.
    ///                 let mut editor = match painted.take() {
    ///                     Some((version, editor)) if version == state.version() => editor,
    ///                     _ => state.editor().clone(),
    ///                 };
    ///                 editor.input(input);
    ///                 *state = TextAreaState::from_editor(editor);
    ///             }
    ///         }
    ///     }
    /// }
    ///
    /// on_key(KeyEvent::new(KeyCode::Up), &mut state, &mut painted);
    /// assert_eq!(state.cursor(), (0, 11));
    /// # // A shifted chord is the same chord: it submits, and edits nothing.
    /// # use ratcn::runtime::Modifiers;
    /// # on_key(
    /// #     KeyEvent {
    /// #         code: KeyCode::Char('J'),
    /// #         modifiers: Modifiers { ctrl: true, alt: false, shift: true },
    /// #     },
    /// #     &mut state,
    /// #     &mut painted,
    /// # );
    /// # assert_eq!(state.value(), "Met Ada today.\nShe counts.");
    /// # // A state the app replaces after a paint is the one edited.
    /// # let editor = TextAreaWidget::new(&state).focused(true).paint(area, &mut buf);
    /// # painted = Some((state.version(), editor));
    /// # state = TextAreaState::default();
    /// # on_key(KeyEvent::new(KeyCode::Char('x')), &mut state, &mut painted);
    /// # assert_eq!(state.value(), "x");
    /// ```
    pub fn paint(mut self, area: Rect, buf: &mut Buffer) -> Editor<'static> {
        let style = self
            .style
            .resolve(self.focused, self.hovered, self.disabled, self.invalid);
        if let Some(title) = self.title {
            Block::bordered()
                .border_style(style.border)
                .title(Line::styled(title, style.title))
                .render(area, buf);
        }
        buf.set_style(well(area, self.title.is_some()), style.text);
        let field = text_rect(area, self.title.is_some());

        // The state cleared the editor's own look when it adopted it; what
        // is left is what this field decides. The setters that re-measure the
        // text run only when the setting differs.
        let editor = &mut self.editor;
        if editor.wrap_mode() != self.wrap_mode {
            editor.set_wrap_mode(self.wrap_mode);
        }
        if editor.mask_char().is_some() {
            editor.clear_mask_char();
        }
        editor.set_style(style.text);
        editor.set_cursor_style(style.cursor);
        editor.set_cursor_line_style(Style::default());
        editor.set_selection_style(style.selection);
        (&*editor).render(field, buf);
        // The field paints the placeholder itself: the editor's would start a
        // cell late, behind a cursor of its own.
        if editor.is_empty() && !self.placeholder.is_empty() {
            let placeholder = pasted_text(self.placeholder).replace('\t', " ");
            for (line, y) in placeholder.split('\n').zip(field.y..field.bottom()) {
                buf.set_stringn(
                    field.x,
                    y,
                    line,
                    usize::from(field.width),
                    style.placeholder,
                );
            }
            if self.focused && !self.disabled && !field.is_empty() {
                buf.set_style(Rect::new(field.x, field.y, 1, 1), style.cursor);
            }
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
/// declared in, wraps a line longer than it is wide, and scrolls to keep the
/// cursor in view.
///
/// The editor's keys all work — arrows, <kbd>Home</kbd>/<kbd>End</kbd>,
/// <kbd>Page Up</kbd>/<kbd>Page Down</kbd>, <kbd>Ctrl</kbd>+<kbd>←</kbd>/<kbd>→</kbd>
/// by word, <kbd>Shift</kbd> with a movement to select, <kbd>Ctrl+W</kbd>,
/// <kbd>Ctrl+K</kbd>, and the rest of the readline set; a key the editor
/// binds is the field's even where it changes nothing. <kbd>Enter</kbd>
/// inserts a line break, so <kbd>Ctrl+Enter</kbd> (or <kbd>Ctrl+J</kbd>) is
/// the one that emits [`on_submit`](Self::on_submit). <kbd>Tab</kbd>,
/// <kbd>Esc</kbd>, and the function keys are left to bubble, as is any chord
/// the editor does not bind, so focus traversal, an enclosing dialog, and the
/// app's shortcuts keep working around a focused field. A paste keeps its
/// line breaks and tabs and loses every other control character.
///
/// A click places the cursor at the character clicked, and a drag selects
/// from the character pressed to the one under the pointer, scrolling the
/// text as the pointer moves on past an edge of the field. The wheel scrolls
/// the text, and at either end of it is left to whatever encloses the field.
///
/// ```
/// # use ratcn::{TextArea, TextAreaState};
/// # struct AppState { notes: TextAreaState }
/// # enum Msg { Notes(TextAreaState), Save }
/// let notes = TextArea::new()
///     .value(|state: &AppState| &state.notes, Msg::Notes)
///     .title("Notes")
///     .placeholder("What happened today?")
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
            wrap_mode: WrapMode::WordOrGlyph,
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

    /// The message <kbd>Ctrl+Enter</kbd> emits, and <kbd>Ctrl+J</kbd>, which
    /// is how a terminal that sends a line feed for Ctrl+Enter reports it.
    /// Plain <kbd>Enter</kbd> stays a line break. Without it both bubble.
    #[must_use]
    pub fn on_submit(mut self, on_submit: impl Fn() -> M + 'static) -> Self {
        self.on_submit = Some(Rc::new(on_submit));
        self
    }

    /// The muted text shown while the field is empty, focused or not, where
    /// the text would start, a line to a row. A focused field shows its
    /// cursor on its first character.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Draw a border around the field with `title` on it, which takes a row
    /// and a column on each side from the text.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// How a line longer than the field is shown. [`WrapMode::WordOrGlyph`],
    /// the default, breaks it over several rows, at a word boundary where
    /// there is one, and <kbd>↑</kbd>/<kbd>↓</kbd> then move by the rows on
    /// screen. [`WrapMode::None`] scrolls it sideways instead. The state's
    /// lines stay as typed.
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
    fn editor<'s>(&'s self, state: &'s TextAreaState) -> &'s Editor<'static> {
        match &self.painted {
            Some((version, editor)) if *version == state.version() => editor,
            _ => state.editor(),
        }
    }

    /// The key policy. The submit chord never reaches the editor; the keys in
    /// [`bubbles`] are not the field's. Of the rest, a key the editor binds is
    /// the field's, Enter included: it emits the new state, or is consumed
    /// where it changes nothing — Up on the first line must not walk out into
    /// the enclosing component, and Ctrl+K at the end of a line must not fire
    /// the app's shortcut. A key the editor does not bind bubbles, which is
    /// how the app's own shortcuts pass through a focused field.
    fn handle_key(&self, key: KeyEvent, state: &TextAreaState) -> EventResult<TextAreaState> {
        if bubbles(key) {
            return EventResult::Ignored;
        }
        let Some(input) = editor_input(&key).filter(is_editor_binding) else {
            return EventResult::Ignored;
        };
        let mut editor = self.editor(state).clone();
        // The yank buffer changes only with the text, or on a copy, which
        // ends the selection: neither goes unseen here.
        let before = (editor.cursor(), editor.selection_range());
        let top = editor.scroll_offset();
        let modified = editor.input(input);
        // A page key can scroll the view and leave the cursor in it, and that
        // scroll is an effect. Scrolled past the end of the text, off the
        // cursor, the view is one the next paint scrolls straight back.
        let (row, _) = editor.scroll_offset();
        let scrolled =
            editor.scroll_offset() != top && editor.screen_cursor().row >= usize::from(row);
        if modified || scrolled || before != (editor.cursor(), editor.selection_range()) {
            EventResult::Emit(TextAreaState::from_editor(editor))
        } else {
            EventResult::Consumed
        }
    }

    /// Insert a paste at the cursor, as the text [`pasted_text`] makes of it.
    fn handle_paste(&self, text: &str, state: &TextAreaState) -> EventResult<TextAreaState> {
        let text = pasted_text(text);
        if text.is_empty() {
            return EventResult::Ignored;
        }
        let mut editor = self.editor(state).clone();
        editor.insert_str(text);
        EventResult::Emit(TextAreaState::from_editor(editor))
    }

    /// The mouse policy. A press on the field claims the rest of the gesture
    /// and remembers the character it landed on, and is otherwise left to the
    /// runtime, which focuses the field: an event carries one message, and a
    /// press spends it on focus. The cursor moves when the gesture says what
    /// it is — to the pressed character on a click, and on a drag to the
    /// character under the pointer, selecting from the pressed one. A drag
    /// that comes back to where it began selects nothing.
    ///
    /// The wheel scrolls the text a few rows, and bubbles once there is no
    /// further to go, so a form the field sits in scrolls on from there.
    fn handle_mouse(
        &self,
        mouse: &MouseEvent,
        state: &TextAreaState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<TextAreaState> {
        let titled = self.title.is_some();
        let text = text_rect(ctx.area(), titled);
        let pointer = Position::new(mouse.column, mouse.row);
        let painted = self.editor(state);
        let editor = match mouse.kind {
            MouseKind::Down(MouseButton::Left) if well(ctx.area(), titled).contains(pointer) => {
                ctx.capture_pointer(MouseButton::Left);
                *ctx.transient() = DragAnchor(cursor_at(painted, text, pointer));
                return EventResult::Ignored;
            }
            MouseKind::Click(MouseButton::Left) if ctx.pointer_captured() => {
                let mut editor = painted.clone();
                editor.cancel_selection();
                editor.move_cursor(cursor_at(&editor, text, pointer));
                editor
            }
            MouseKind::Drag(MouseButton::Left) if ctx.pointer_captured() => {
                let DragAnchor(anchor) = *ctx.transient();
                let mut editor = painted.clone();
                let pointer = cursor_at(&editor, text, pointer);
                select_dragged(&mut editor, anchor, pointer);
                editor
            }
            MouseKind::Scroll(direction) => {
                let (top, _) = painted.scroll_offset();
                let last = screen_rows(painted).saturating_sub(text.height);
                let rows = match direction {
                    ScrollDirection::Up => -top.min(WHEEL_ROWS).cast_signed(),
                    ScrollDirection::Down => last.saturating_sub(top).min(WHEEL_ROWS).cast_signed(),
                    ScrollDirection::Left | ScrollDirection::Right => 0,
                };
                if rows == 0 {
                    return EventResult::Ignored;
                }
                let mut editor = painted.clone();
                // The editor's scroll extends an active selection, which the
                // next key typed would then replace.
                editor.cancel_selection();
                editor.scroll((rows, 0));
                editor
            }
            _ => return EventResult::Ignored,
        };
        let view = |editor: &Editor<'_>| {
            (
                editor.cursor(),
                editor.selection_range(),
                editor.scroll_offset(),
            )
        };
        if view(painted) == view(&editor) {
            EventResult::Consumed
        } else {
            EventResult::Emit(TextAreaState::from_editor(editor))
        }
    }
}

/// The character a press landed on, kept at the field's identity while the
/// button is held: where the selection a drag makes begins. It is a position
/// in the text, not on screen, so it holds while the drag scrolls the view.
#[derive(Clone, Copy)]
struct DragAnchor(CursorMove);

impl Default for DragAnchor {
    fn default() -> Self {
        Self(CursorMove::Jump(0, 0))
    }
}

/// How many rows the text takes on screen, a wrapped line counting once for
/// each row it covers. The editor does not say, but it does say which row its
/// cursor is on, so this asks a copy of it with the cursor at the very end.
fn screen_rows(editor: &Editor<'_>) -> u16 {
    let mut end = editor.clone();
    let DataCursor(row, column) = end.screen_to_data(usize::MAX, usize::MAX);
    end.move_cursor(CursorMove::Jump(
        u16::try_from(row).unwrap_or(u16::MAX),
        u16::try_from(column).unwrap_or(u16::MAX),
    ));
    u16::try_from(end.screen_cursor().row + 1).unwrap_or(u16::MAX)
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
        if let Some(title) = &self.title {
            widget = widget.title(title);
        }
        let editor = ctx.with_buffer(ctx.area(), |area, buf| widget.paint(area, buf));
        self.painted = Some((state.version(), editor));
    }

    fn handle_event(&mut self, event: &Event, state: &S, ctx: &mut EventCtx<'_>) -> EventResult<M> {
        let Some((read, on_change)) = &self.value else {
            return EventResult::Ignored;
        };
        if self.disabled {
            return EventResult::Ignored;
        }
        let edited = match event {
            Event::Key(key) if is_submit_chord(*key) => {
                return match &self.on_submit {
                    Some(on_submit) if !key.modifiers.alt && !key.modifiers.shift => {
                        EventResult::Emit(on_submit())
                    }
                    _ => EventResult::Ignored,
                };
            }
            Event::Key(key) => self.handle_key(*key, read(state)),
            Event::Paste(text) => self.handle_paste(text, read(state)),
            Event::Mouse(mouse) => self.handle_mouse(mouse, read(state), ctx),
            #[allow(
                unreachable_patterns,
                reason = "`Event` is non-exhaustive in a copy of this file, outside ratcn"
            )]
            _ => EventResult::Ignored,
        };
        match edited {
            EventResult::Emit(next) => EventResult::Emit(on_change(next)),
            EventResult::Consumed => EventResult::Consumed,
            EventResult::Ignored => EventResult::Ignored,
        }
    }

    fn scope_options(&self, _state: &S) -> ScopeOptions {
        ScopeOptions::default().focusable(self.value.is_some() && !self.disabled)
    }
}

/// Ctrl+Enter, or Ctrl+J: a terminal that sends a line feed for Ctrl+Enter
/// reports it as Ctrl+J. Neither ever reaches the editor, which binds Ctrl+J to
/// deleting back to the start of the line.
fn is_submit_chord(key: KeyEvent) -> bool {
    key.modifiers.ctrl && matches!(key.code, KeyCode::Enter | KeyCode::Char('j' | 'J'))
}

/// The keys a field leaves alone, whatever the editor binds them to. Tab and
/// `BackTab` belong to focus traversal, Esc and the function keys to enclosing
/// components and the app.
const fn bubbles(key: KeyEvent) -> bool {
    matches!(
        key.code,
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc | KeyCode::F(_)
    )
}

/// A paste, or a placeholder, as text a field can hold: one kind of line
/// break, and no control character but that and the tab. Terminals send a
/// pasted break as `\r\n`, `\n`, or a bare `\r`; left in a line, a carriage
/// return — or an escape, or a bell — would be text the user cannot see.
fn pasted_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|char| matches!(char, '\n' | '\t') || !char.is_control())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScrollArea;
    use crate::runtime::{ChildId, FocusState, Modifiers, Ratcn};
    use crate::test_support::{Driver, key, key_with, mouse, styled_snapshot};

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
        Scrolled(u16),
    }

    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    const ALT: Modifiers = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    const CTRL_SHIFT: Modifiers = Modifiers {
        ctrl: true,
        alt: false,
        shift: true,
    };

    /// The field every test declares unless it says otherwise: eight columns,
    /// six of them text inside the inset, by three rows, so ten lines have to
    /// scroll and a long one to wrap.
    const FIELD: Rect = Rect::new(0, 0, 8, 3);

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
            notes: TextAreaState::from_editor(editor),
            ..State::default()
        }
    }

    fn textarea() -> TextArea<State, Msg> {
        TextArea::new()
            .value(|state: &State| &state.notes, Msg::Notes)
            .on_submit(|| Msg::Submit)
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

    /// Route one event that may emit nothing, storing what it does emit.
    fn route(driver: &mut Driver<State, Msg>, state: &mut State, event: Event) {
        match driver.event(event, state) {
            EventResult::Emit(Msg::Notes(notes)) => state.notes = notes,
            EventResult::Emit(Msg::Focus(focus)) => state.focus = focus,
            EventResult::Emit(other) => panic!("unexpected {other:?}"),
            EventResult::Consumed | EventResult::Ignored => {}
        }
    }

    /// Press and release the primary button on one cell.
    fn click(driver: &mut Driver<State, Msg>, state: &mut State, column: u16, row: u16) {
        route(driver, state, mouse(LEFT_DOWN, column, row));
        route(driver, state, mouse(LEFT_UP, column, row));
    }

    const LEFT_DOWN: MouseKind = MouseKind::Down(MouseButton::Left);
    const LEFT_DRAG: MouseKind = MouseKind::Drag(MouseButton::Left);
    const LEFT_UP: MouseKind = MouseKind::Up(MouseButton::Left);
    const WHEEL_UP: MouseKind = MouseKind::Scroll(ScrollDirection::Up);
    const WHEEL_DOWN: MouseKind = MouseKind::Scroll(ScrollDirection::Down);

    /// What the field's text cells show, inside the inset, row by row.
    fn field(driver: &Driver<State, Msg>) -> Vec<String> {
        (0..FIELD.height)
            .map(|row| {
                driver
                    .row(row)
                    .chars()
                    .skip(1)
                    .take(usize::from(FIELD.width - 2))
                    .collect()
            })
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

    /// Terminals that send a line feed for Ctrl+Enter deliver it as Ctrl+J,
    /// which the editor binds to deleting back to the start of the line. It
    /// is the submit chord here, and must never reach the editor: pressing
    /// submit would wipe the line instead.
    #[test]
    fn ctrl_j_submits_like_ctrl_enter_and_never_deletes() {
        let mut driver = driver();
        let state = state("one two");
        render(&mut driver, &state);

        assert!(matches!(
            driver.event(key_with(KeyCode::Char('j'), CTRL), &state),
            EventResult::Emit(Msg::Submit)
        ));

        render_with(&mut driver, &state, || {
            TextArea::new().value(|state: &State| &state.notes, Msg::Notes)
        });
        assert!(
            matches!(
                driver.event(key_with(KeyCode::Char('j'), CTRL), &state),
                EventResult::Ignored
            ),
            "with nothing to submit, the chord bubbles and deletes nothing"
        );
    }

    /// The editor's own scroll extends a selection that is active, so a
    /// notch of the wheel would quietly grow it — and the next key typed
    /// would replace text the user never selected. The wheel drops the
    /// selection instead.
    #[test]
    fn the_wheel_never_grows_a_selection() {
        let mut driver = driver();
        let mut state = state_at_top(TEN_LINES);
        render(&mut driver, &state);

        send(&mut driver, &mut state, key_with(KeyCode::Down, SHIFT));
        send(&mut driver, &mut state, key_with(KeyCode::Down, SHIFT));
        render(&mut driver, &state);
        send(&mut driver, &mut state, mouse(WHEEL_DOWN, 1, 1));
        assert!(!state.notes.editor().is_selecting());
        render(&mut driver, &state);
        send(&mut driver, &mut state, key(KeyCode::Char('X')));
        assert_eq!(
            state.notes.value().replace('X', ""),
            TEN_LINES,
            "typing after the wheel inserts and deletes nothing"
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
            key_with(KeyCode::Char('x'), ALT),
            key_with(KeyCode::Up, ALT),
        ] {
            assert!(
                matches!(driver.event(event.clone(), &state), EventResult::Ignored),
                "{event:?} must bubble"
            );
        }
    }

    /// Keys route by binding, not by effect. A chord the editor binds is
    /// the field's wherever the cursor is: if Ctrl+K at the end of a line
    /// bubbled, the app's Ctrl+K shortcut would fire or not depending on
    /// where the cursor happened to be.
    #[test]
    fn a_bound_chord_is_consumed_even_where_it_changes_nothing() {
        let mut driver = driver();
        let state = state("note");
        render(&mut driver, &state);

        for event in [
            key_with(KeyCode::Char('k'), CTRL),
            key_with(KeyCode::Char('n'), CTRL),
            key_with(KeyCode::Char('e'), CTRL),
            key_with(KeyCode::Char('>'), ALT),
            key_with(KeyCode::Char('K'), CTRL_SHIFT),
            key_with(KeyCode::Down, CTRL),
        ] {
            assert!(
                matches!(driver.event(event.clone(), &state), EventResult::Consumed),
                "{event:?} must be consumed"
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

    /// A control character typed as text would sit in the value unseen: a
    /// tab character, or an escape for the terminal to act on when the value
    /// is shown again. Ctrl+Alt on one is a chord, not `AltGr` text.
    #[test]
    fn a_control_character_is_never_typed() {
        let mut driver = driver();
        let state = state("note");
        render(&mut driver, &state);

        for event in [
            key(KeyCode::Char('\t')),
            key(KeyCode::Char('\u{1b}')),
            key_with(KeyCode::Char('\n'), Modifiers { alt: true, ..CTRL }),
        ] {
            assert!(
                matches!(driver.event(event.clone(), &state), EventResult::Ignored),
                "{event:?}"
            );
        }
    }

    /// A paste keeps its lines and tabs, whichever line break the terminal
    /// sent, and nothing invisible comes with it: no carriage return, and no
    /// escape for the terminal to act on when the text is shown again.
    #[test]
    fn a_paste_keeps_its_line_breaks_and_drops_control_characters() {
        let mut driver = driver();
        let mut state = state("a");
        render(&mut driver, &state);

        send(
            &mut driver,
            &mut state,
            Event::Paste("b\r\nc\nd\re\tf\u{1b}\u{7}".to_owned()),
        );
        assert_eq!(state.notes.lines(), ["ab", "c", "d", "e\tf"]);
        assert_eq!(state.notes.cursor(), (3, 3));
        assert!(
            matches!(
                driver.event(Event::Paste("\u{1b}".to_owned()), &state),
                EventResult::Ignored
            ),
            "a paste with nothing to insert is not an edit"
        );
    }

    /// Disabled is the loudest state: no keys, no paste, no mouse, no focus.
    #[test]
    fn a_disabled_field_ignores_input() {
        let mut driver = driver();
        let state = state(TEN_LINES);
        render_with(&mut driver, &state, || textarea().disabled(true));

        for event in [
            key(KeyCode::Char('x')),
            key(KeyCode::Enter),
            key_with(KeyCode::Enter, CTRL),
            key(KeyCode::Tab),
            Event::Paste("x".to_owned()),
            mouse(LEFT_DOWN, 1, 0),
            mouse(MouseKind::Click(MouseButton::Left), 1, 0),
            mouse(LEFT_DRAG, 2, 0),
            mouse(WHEEL_UP, 1, 0),
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

    /// A key can scroll the view and leave the cursor where it was: Page Up
    /// with the cursor on the second line of a view that starts there pages
    /// the first line in. That scroll is the key's whole effect, and the next
    /// paint must show it.
    #[test]
    fn a_key_that_only_scrolls_keeps_the_scroll() {
        let mut driver = driver();
        let lines = (0..30).map(|line| line.to_string()).collect::<Vec<_>>();
        let mut state = state_at_top(&lines.join("\n"));
        render(&mut driver, &state);
        send(&mut driver, &mut state, mouse(WHEEL_DOWN, 1, 1));
        render(&mut driver, &state);
        send(&mut driver, &mut state, key(KeyCode::Up));
        send(&mut driver, &mut state, key(KeyCode::Up));
        render(&mut driver, &state);
        assert_eq!(field(&driver)[0], "1     ");
        assert_eq!(state.notes.cursor(), (1, 0));

        send(&mut driver, &mut state, key(KeyCode::PageUp));
        render(&mut driver, &state);
        assert_eq!(field(&driver)[0], "0     ");
    }

    /// A form's text area wraps: a long line breaks over rows, at a word
    /// boundary where it has one, rather than scrolling out of sight. Down
    /// then moves by the rows on screen, not by the lines in the text. Where
    /// a line wraps depends on the width it was painted at, which again only
    /// the painted editor knows: for the state's own editor this is one line
    /// with nothing below it.
    #[test]
    fn a_long_line_wraps_and_down_moves_by_visual_row() {
        let mut driver = driver();
        let mut state = state_at_top("aaa bbb ccc");
        render(&mut driver, &state);
        assert_eq!(field(&driver), ["aaa   ", "bbb   ", "ccc   "]);

        send(&mut driver, &mut state, key(KeyCode::Down));
        assert_eq!(
            state.notes.cursor(),
            (0, 4),
            "onto the second row of the same line"
        );
        assert_eq!(state.notes.lines().len(), 1, "wrapping changes no text");
    }

    /// A word wider than the field has no boundary to break at, and must
    /// still stay in sight; and the caller who wants one row per line can
    /// have it, scrolled sideways to the cursor.
    #[test]
    fn a_long_word_breaks_by_glyph_unless_wrapping_is_switched_off() {
        let mut driver = driver();
        let state = state("abcdefghij");
        render(&mut driver, &state);
        assert_eq!(field(&driver), ["abcdef", "ghij  ", "      "]);

        render_with(&mut driver, &state, || textarea().wrap_mode(WrapMode::None));
        assert_eq!(field(&driver), ["fghij ", "      ", "      "]);
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

    /// A press has one message to give, and an unfocused field needs it for
    /// focus: typing into a field that took the cursor but not the keyboard
    /// would go somewhere else. The cursor follows on the release.
    #[test]
    fn a_click_focuses_the_field_and_places_the_cursor() {
        let mut driver = driver();
        let mut state = State {
            focus: FocusState::none(),
            ..state("ab\ncd")
        };
        render(&mut driver, &state);

        let EventResult::Emit(Msg::Focus(focus)) = driver.event(mouse(LEFT_DOWN, 2, 0), &state)
        else {
            panic!("a press must move focus to the field");
        };
        assert_eq!(focus.path(), [ChildId::Static("notes")]);
        state.focus = focus;
        render(&mut driver, &state);

        send(&mut driver, &mut state, mouse(LEFT_UP, 2, 0));
        assert_eq!(state.notes.cursor(), (0, 1));
    }

    /// The cursor goes to the character that was clicked — whatever its
    /// width, a tab included — to the end of a line from anywhere past it,
    /// and to the last line from anywhere below the text.
    #[test]
    fn a_click_places_the_cursor_on_the_character_under_it() {
        let mut driver = driver();
        let mut state = state_at_top("ab\n\tc\n日本");
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(ChildId::Static("notes"), textarea(), Rect::new(0, 0, 12, 5));
            });
        };
        render(&mut driver, &state);
        assert_eq!(driver.row(1), "     c      ");

        for ((column, row), cursor) in [
            ((2, 0), (0, 1)),
            ((10, 0), (0, 2)),
            ((2, 1), (1, 0)),
            ((5, 1), (1, 1)),
            ((6, 1), (1, 2)),
            ((1, 2), (2, 0)),
            ((2, 2), (2, 0)),
            ((3, 2), (2, 1)),
            ((3, 4), (2, 1)),
        ] {
            click(&mut driver, &mut state, column, row);
            assert_eq!(state.notes.cursor(), cursor, "a click on {column}, {row}");
            render(&mut driver, &state);
        }
    }

    /// A wrapped line covers several rows, and ten lines in three rows are
    /// scrolled. A click means the character on screen in both cases.
    #[test]
    fn a_click_counts_wrapped_rows_and_scrolled_lines() {
        let mut driver = driver();
        let mut state = state_at_top("aaa bbb ccc");
        render(&mut driver, &state);
        assert_eq!(field(&driver), ["aaa   ", "bbb   ", "ccc   "]);
        click(&mut driver, &mut state, 2, 2);
        assert_eq!(state.notes.cursor(), (0, 9), "the second c of the one line");

        state.notes = TextAreaState::new(TEN_LINES);
        render(&mut driver, &state);
        assert_eq!(field(&driver), ["7     ", "8     ", "9     "]);
        click(&mut driver, &mut state, 1, 0);
        assert_eq!(state.notes.cursor(), (7, 0));
        render(&mut driver, &state);
        assert_eq!(
            field(&driver)[0],
            "7     ",
            "a click must not scroll the view"
        );
    }

    /// The text of a titled field starts inside its border, and the border
    /// itself is not text: a press there focuses and moves no cursor.
    #[test]
    fn a_click_in_a_titled_field_counts_from_inside_the_border() {
        let mut driver = driver();
        let mut state = state_at_top("ab\ncd");
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    ChildId::Static("notes"),
                    textarea().title("Notes"),
                    Rect::new(0, 0, 12, 4),
                );
            });
        };
        render(&mut driver, &state);
        assert_eq!(driver.row(2), "│ cd       │");

        click(&mut driver, &mut state, 3, 2);
        assert_eq!(state.notes.cursor(), (1, 1));

        render(&mut driver, &state);
        click(&mut driver, &mut state, 5, 0);
        assert_eq!(state.notes.cursor(), (1, 1), "the border is not text");
    }

    /// The inset column on either side of the text is painted as field, so
    /// a press there is a press on the field: it places the cursor at the
    /// nearest character.
    #[test]
    fn a_click_on_the_inset_places_the_cursor() {
        let mut driver = driver();
        let mut state = state_at_top("ab\ncd");
        render(&mut driver, &state);

        click(&mut driver, &mut state, 7, 0);
        assert_eq!(state.notes.cursor(), (0, 2), "the right inset");
        render(&mut driver, &state);
        click(&mut driver, &mut state, 0, 1);
        assert_eq!(state.notes.cursor(), (1, 0), "the left inset");
    }

    /// A drag selects from the character pressed to the one under the
    /// pointer, across lines, and the selection is a real one: the next
    /// character typed replaces it.
    #[test]
    fn a_drag_selects_and_typing_replaces_the_selection() {
        let mut driver = driver();
        let mut state = state_at_top("abc\ndef\nghi");
        render(&mut driver, &state);

        route(&mut driver, &mut state, mouse(LEFT_DOWN, 2, 0));
        assert_eq!(state.notes.editor().selection_range(), None);
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 3, 1));
        assert_eq!(
            state.notes.editor().selection_range(),
            Some(((0, 1), (1, 2)))
        );
        route(&mut driver, &mut state, mouse(LEFT_UP, 3, 1));
        assert_eq!(
            state.notes.editor().selection_range(),
            Some(((0, 1), (1, 2))),
            "the release keeps it"
        );

        render(&mut driver, &state);
        send(&mut driver, &mut state, key(KeyCode::Char('X')));
        assert_eq!(state.notes.lines(), ["aXf", "ghi"]);
    }

    /// An empty selection is still a selection to the editor: the next
    /// arrow key would extend it. Neither a click nor a drag that returns to
    /// where it began may leave one — nor an older selection standing.
    #[test]
    fn a_click_or_a_drag_back_to_its_start_leaves_no_selection() {
        let mut driver = driver();
        let mut state = state("abc\ndef");
        render(&mut driver, &state);

        send(&mut driver, &mut state, key_with(KeyCode::Up, SHIFT));
        assert!(state.notes.editor().is_selecting());
        click(&mut driver, &mut state, 3, 0);
        assert!(!state.notes.editor().is_selecting(), "a click deselects");
        assert_eq!(state.notes.cursor(), (0, 2));

        route(&mut driver, &mut state, mouse(LEFT_DOWN, 3, 0));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 2, 1));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 3, 0));
        route(&mut driver, &mut state, mouse(LEFT_UP, 3, 0));
        assert!(!state.notes.editor().is_selecting());
        assert_eq!(state.notes.cursor(), (0, 2));
    }

    /// A selection often has to reach text that is scrolled out of sight.
    /// The pointer keeps its hold on the field after leaving it, and every
    /// move it makes out there takes the selection one row further and the
    /// view with it.
    #[test]
    fn a_drag_past_the_edge_keeps_extending_and_scrolls() {
        let area = Rect::new(0, 1, 8, 3);
        let mut driver = driver();
        let mut state = state_at_top(TEN_LINES);
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(ChildId::Static("notes"), textarea(), area);
            });
        };
        let top = |driver: &Driver<State, Msg>| driver.row(1)[1..2].to_owned();
        render(&mut driver, &state);

        route(&mut driver, &mut state, mouse(LEFT_DOWN, 1, 2));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 1, 4));
        assert_eq!(state.notes.cursor(), (3, 0), "one row past the edge");
        render(&mut driver, &state);
        assert_eq!(top(&driver), "1");

        send(&mut driver, &mut state, mouse(LEFT_DRAG, 9, 4));
        assert_eq!(state.notes.cursor(), (4, 1), "and one more");
        assert_eq!(
            state.notes.editor().selection_range(),
            Some(((1, 0), (4, 1))),
            "still from the line that was pressed"
        );
        render(&mut driver, &state);
        assert_eq!(top(&driver), "2");

        route(&mut driver, &mut state, mouse(LEFT_UP, 9, 4));
        assert_eq!(state.notes.cursor(), (4, 1), "the release moves nothing");

        // And out the other side, back through the line pressed.
        route(&mut driver, &mut state, mouse(LEFT_DOWN, 1, 2));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 1, 0));
        assert_eq!(
            state.notes.editor().selection_range(),
            Some(((1, 0), (3, 0)))
        );
    }

    /// The wheel moves the text three rows a notch, stops where the last
    /// line reaches the bottom of the field rather than scrolling the text
    /// out of it, and needs no focus.
    #[test]
    fn the_wheel_scrolls_the_text() {
        let mut driver = driver();
        let mut state = State {
            focus: FocusState::none(),
            ..state_at_top(TEN_LINES)
        };
        render(&mut driver, &state);
        assert!(
            matches!(
                driver.event(mouse(WHEEL_UP, 1, 1), &state),
                EventResult::Ignored
            ),
            "already at the top"
        );

        send(&mut driver, &mut state, mouse(WHEEL_DOWN, 1, 1));
        render(&mut driver, &state);
        assert_eq!(field(&driver), ["3     ", "4     ", "5     "]);

        send(&mut driver, &mut state, mouse(WHEEL_DOWN, 1, 1));
        send(&mut driver, &mut state, mouse(WHEEL_DOWN, 1, 1));
        render(&mut driver, &state);
        assert_eq!(field(&driver), ["7     ", "8     ", "9     "]);

        send(&mut driver, &mut state, mouse(WHEEL_UP, 1, 1));
        render(&mut driver, &state);
        assert_eq!(field(&driver), ["4     ", "5     ", "6     "]);
    }

    /// Rows are what the wheel scrolls, and a wrapped line is several: the
    /// end of the text is its last row on screen, not its last line.
    #[test]
    fn the_wheel_scrolls_by_wrapped_rows() {
        let mut driver = driver();
        let mut state = state_at_top("aaa bbb ccc ddd eee");
        render(&mut driver, &state);
        assert_eq!(field(&driver), ["aaa   ", "bbb   ", "ccc   "]);

        send(&mut driver, &mut state, mouse(WHEEL_DOWN, 1, 1));
        render(&mut driver, &state);
        assert_eq!(field(&driver), ["ccc   ", "ddd   ", "eee   "]);
        assert!(matches!(
            driver.event(mouse(WHEEL_DOWN, 1, 1), &state),
            EventResult::Ignored
        ));

        let short = state_at_top("aaa bbb");
        render(&mut driver, &short);
        assert!(
            matches!(
                driver.event(mouse(WHEEL_DOWN, 1, 1), &short),
                EventResult::Ignored
            ),
            "text that fits has nothing to scroll"
        );
    }

    /// A field that ate every wheel event would trap the pointer: a form
    /// could not be scrolled past it. With nowhere left to scroll, the wheel
    /// goes to whatever encloses the field.
    #[test]
    fn the_wheel_bubbles_once_the_text_has_nowhere_to_go() {
        let mut driver = driver();
        let mut state = state_at_top(TEN_LINES);
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    ChildId::Static("scroll"),
                    ScrollArea::new(9)
                        .scroll(|_: &State| 0, Msg::Scrolled)
                        .content(|ctx| {
                            let area = ctx.area();
                            ctx.component(
                                ChildId::Static("notes"),
                                textarea(),
                                Rect::new(area.x, area.y, 8, 3),
                            );
                        }),
                    Rect::new(0, 0, 12, 5),
                );
            });
        };
        render(&mut driver, &state);

        let at_top = driver.event(mouse(WHEEL_UP, 1, 1), &state);
        assert!(
            !matches!(at_top, EventResult::Emit(_)),
            "at the top of both, nothing scrolls: {at_top:?}"
        );
        for _ in 0..3 {
            send(&mut driver, &mut state, mouse(WHEEL_DOWN, 1, 1));
            render(&mut driver, &state);
        }
        assert_eq!(field(&driver)[2], "9     ", "the text is at its end");
        assert!(
            matches!(
                driver.event(mouse(WHEEL_DOWN, 1, 1), &state),
                EventResult::Emit(Msg::Scrolled(3))
            ),
            "the next notch scrolls the form"
        );
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
            buffer.cell((3, 1)).expect("the cell after the text").bg
        };

        assert_eq!(paint(true), style.cursor_background);
        assert_eq!(paint(false), style.background);
    }

    /// The placeholder says what belongs in an empty field, and stays while
    /// it is empty: a field that dropped it on focus would lose its label
    /// the moment the user arrived to fill it in. Text replaces it. A
    /// placeholder of several lines keeps them.
    #[test]
    fn the_placeholder_shows_while_the_field_is_empty() {
        let theme = Theme::default_dark();
        let style = TextAreaStyle::from_theme(&theme);
        let area = Rect::new(0, 0, 8, 2);
        let paint = |state: &TextAreaState, focused| {
            let mut buffer = Buffer::empty(area);
            TextAreaWidget::new(state)
                .themed(&theme)
                .placeholder("Notes\nhere")
                .focused(focused)
                .render(area, &mut buffer);
            buffer
        };
        let symbols = |buffer: &Buffer| {
            buffer
                .content
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        };

        let unfocused = paint(&TextAreaState::default(), false);
        assert_eq!(symbols(&unfocused), " Notes   here   ");
        assert_eq!(unfocused[(1, 0)].fg, style.placeholder_foreground);
        let focused = paint(&TextAreaState::default(), true);
        assert_eq!(symbols(&focused), " Notes   here   ");
        assert_eq!(focused[(1, 1)].fg, style.placeholder_foreground);
        assert_eq!(
            symbols(&paint(&TextAreaState::new("Ada"), true)),
            " Ada            "
        );
    }

    /// The placeholder stands where the text will, each of its lines where
    /// a line of text would start. Started a cell later, the first key typed
    /// would visibly shift the field's contents left.
    #[test]
    fn the_placeholder_starts_in_the_column_the_text_does() {
        let area = Rect::new(0, 0, 12, 4);
        let position_of = |state: &TextAreaState, titled, focused, symbol| {
            let widget = TextAreaWidget::new(state)
                .placeholder("Notes\nhere")
                .focused(focused);
            let widget = if titled { widget.title("T") } else { widget };
            let mut buffer = Buffer::empty(area);
            widget.render(area, &mut buffer);
            let index = buffer
                .content
                .iter()
                .position(|cell| cell.symbol() == symbol)
                .expect("painted");
            buffer.pos_of(index)
        };

        for titled in [false, true] {
            for focused in [false, true] {
                let placeholder = position_of(&TextAreaState::default(), titled, focused, "N");
                let text = position_of(&TextAreaState::new("Ada"), titled, focused, "A");
                assert_eq!(placeholder, text, "titled {titled}, focused {focused}");
                let second = position_of(&TextAreaState::default(), titled, focused, "h");
                assert_eq!(second, (text.0, text.1 + 1), "the second line");
            }
        }
    }

    /// A placeholder line breaks wherever a typed one would, a bare carriage
    /// return included, and shows no control character: a tab is a space.
    #[test]
    fn the_placeholder_is_plain_text() {
        let area = Rect::new(0, 0, 8, 3);
        let mut buffer = Buffer::empty(area);
        TextAreaWidget::new(&TextAreaState::default())
            .placeholder("a\rb\tc\nd\u{1b}e")
            .render(area, &mut buffer);
        let symbols: String = buffer
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert_eq!(symbols, " a       b c     de     ");
    }

    /// A focused empty field still shows where typing goes: the cursor
    /// rests on the placeholder's first character, the way a block cursor
    /// rests on the character under it, and the rest stays muted.
    #[test]
    fn a_focused_empty_field_shows_the_cursor_on_the_placeholder() {
        let theme = Theme::default_dark();
        let style = TextAreaStyle::from_theme(&theme);
        let area = Rect::new(0, 0, 8, 2);
        let mut buffer = Buffer::empty(area);
        TextAreaWidget::new(&TextAreaState::default())
            .themed(&theme)
            .placeholder("Notes")
            .focused(true)
            .render(area, &mut buffer);

        let first = &buffer[(1, 0)];
        assert_eq!(
            (first.symbol(), first.fg, first.bg),
            ("N", style.cursor_foreground, style.cursor_background)
        );
        assert_eq!(buffer[(2, 0)].fg, style.placeholder_foreground);
    }

    /// A paint-only field driven by hand keeps its view only if the loop
    /// edits the editor that was painted, which alone knows how far the view
    /// is scrolled. `paint` hands it back for exactly that.
    #[test]
    fn a_standalone_loop_edits_the_painted_editor_and_keeps_its_view() {
        let area = FIELD;
        let first_row = |buffer: &Buffer| {
            buffer.content[..usize::from(area.width)]
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        };
        let mut buffer = Buffer::empty(area);
        let mut editor = TextAreaWidget::new(&TextAreaState::new(TEN_LINES))
            .focused(true)
            .paint(area, &mut buffer);
        assert_eq!(first_row(&buffer), " 7      ");

        editor.input(editor_input(&KeyEvent::new(KeyCode::Up)).expect("an editor key"));
        let state = TextAreaState::from_editor(editor);
        let mut buffer = Buffer::empty(area);
        TextAreaWidget::new(&state)
            .focused(true)
            .render(area, &mut buffer);
        assert_eq!(first_row(&buffer), " 7      ", "the view must not jump");
    }

    /// A state can be built from any editor, configured any way. The field
    /// paints its own look over whatever it was given: a border, line
    /// numbers, or right alignment left in place would move the text away
    /// from where a click is read, and the click would land on the wrong
    /// character.
    #[test]
    fn a_foreign_editors_settings_do_not_reach_the_paint() {
        let mut foreign = Editor::from(["ab", "cd"]);
        foreign.set_block(Block::bordered());
        foreign.set_line_number_style(Style::default());
        foreign.set_alignment(ratatui::layout::Alignment::Right);
        foreign.set_mask_char('*');
        let mut driver = driver();
        let mut state = State {
            notes: TextAreaState::from_editor(foreign),
            ..State::default()
        };
        render(&mut driver, &state);
        assert_eq!(field(&driver), ["ab    ", "cd    ", "      "]);

        click(&mut driver, &mut state, 2, 1);
        assert_eq!(state.notes.cursor(), (1, 1));
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
        assert_eq!(driver.row(1), "│ A        │");
        assert_eq!(driver.row(2), "│ B        │");
        assert_eq!(driver.row(3), "└──────────┘");
    }

    /// Disabled wins over invalid everywhere, the border included: a field
    /// the user cannot edit must not ask them to fix it.
    #[test]
    fn a_disabled_field_draws_no_invalid_border() {
        let theme = Theme::default_dark();
        let style = TextAreaStyle::from_theme(&theme);
        let area = Rect::new(0, 0, 8, 4);
        let state = TextAreaState::new("x");
        let border = |disabled| {
            let mut buffer = Buffer::empty(area);
            TextAreaWidget::new(&state)
                .themed(&theme)
                .title("T")
                .invalid(true)
                .disabled(disabled)
                .render(area, &mut buffer);
            buffer[(0, 0)].fg
        };

        assert_eq!(border(false), style.invalid_foreground);
        assert_eq!(border(true), style.border);
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
            " Ada  |\n\
             \x20Ada  |\n\
             \x20Ada  |\n\
             \x20Ada  |\n\
             \x20Ada  |\n\
             aaaaaa\n\
             bbbbcb\n\
             ddddcd\n\
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
