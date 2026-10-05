//! A single-line text field: the value in a well, a block cursor while
//! focused, and an optional titled border around it.
//!
//! ```text
//! ┌Name──────────┐
//! │ Ada Lovelace │
//! └──────────────┘
//! ```
//!
//! The text is app-owned, like every other value here: the field reads an
//! [`InputState`] from app state and emits a new one for each keystroke.
//! Editing, cursor movement, selection, and horizontal scrolling belong to the
//! [ratatui-textarea](ratatui_textarea) editor inside that state; what lives
//! in this module is the look, the keys the field takes and the ones it leaves
//! to bubble, how a paste becomes one line, and what the mouse does.

use std::{fmt, rc::Rc};

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Widget},
};

use crate::{
    Theme,
    color::{FIELD_FOCUS_SHIFT, FIELD_HOVER_SHIFT, away_from, dim},
    geometry::fixed_height,
    runtime::{
        Component, DeclareCtx, Event, EventCtx, EventResult, KeyCode, KeyEvent, Modifiers,
        MouseButton, MouseEvent, MouseKind, PaintCtx, ScopeOptions,
    },
    text_edit::{
        CursorMove, Editor, InputState, WrapMode, cursor_at, editor_input, is_editor_binding,
        select_dragged,
    },
    theme::resolve_style,
};

/// An input's colors.
///
/// At rest [`foreground`](Self::foreground) sits on
/// [`background`](Self::background). Focus and hover swap the background, with
/// hover beating focus; disabled mutes the text and wins over both. Invalid
/// recolors the text, the border, and the title, and is independent of focus
/// and hover: it says something about the content, not about the
/// interaction. Disabled wins over it too, since a field the user cannot edit
/// cannot be fixed.
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
    /// The [`prefix`](Input::prefix) and [`suffix`](Input::suffix) color,
    /// under whatever style their spans carry.
    pub adornment_foreground: Color,
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
            adornment_foreground: Color::DarkGray,
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
            adornment_foreground: theme.muted_foreground,
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
            adornment: Style::default()
                .fg(if disabled {
                    self.disabled_foreground
                } else {
                    self.adornment_foreground
                })
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
    adornment: Style,
    cursor: Style,
    selection: Style,
    border: Style,
    title: Style,
}

/// How many rows a field is: one, or three with a titled border.
const fn field_height(titled: bool) -> u16 {
    if titled { 3 } else { 1 }
}

/// The rows a field occupies in `area`. Shorter than the field and there is
/// no field at all.
const fn field_rows(area: Rect, titled: bool) -> Rect {
    fixed_height(area, field_height(titled))
}

/// The cells of `area` the field's background fills: its rows, inside the
/// border when there is one.
fn well(area: Rect, titled: bool) -> Rect {
    let field = field_rows(area, titled);
    if titled {
        Block::bordered().inner(field)
    } else {
        field
    }
}

/// Where a field's row puts the prefix, the text, and the suffix.
struct Parts {
    prefix: Rect,
    text: Rect,
    suffix: Rect,
}

/// Lay out the well's row: the prefix at its start, the suffix at its end,
/// each a cell apart from the text between them. Paint draws the editor and
/// the placeholder in `text` and the mouse is read against it, so the three
/// cannot drift apart.
fn parts(area: Rect, titled: bool, prefix: Option<&Line>, suffix: Option<&Line>) -> Parts {
    let row = well(area, titled);
    let prefix = prefix.map_or(0, adornment_width).min(row.width);
    let suffix = suffix.map_or(0, adornment_width).min(row.width - prefix);
    let spaced = |width: u16| if width == 0 { 0 } else { width + 1 };
    let text_x = row.x + spaced(prefix).min(row.width);
    let text_right = row.right().saturating_sub(spaced(suffix)).max(text_x);
    Parts {
        prefix: Rect::new(row.x, row.y, prefix, row.height),
        text: Rect::new(text_x, row.y, text_right - text_x, row.height),
        suffix: Rect::new(row.right() - suffix, row.y, suffix, row.height),
    }
}

/// The cells an adornment takes on screen. A tab in one is not supported:
/// it measures as no cells.
fn adornment_width(line: &Line) -> u16 {
    u16::try_from(line.width()).unwrap_or(u16::MAX)
}

/// Paint an adornment in its cells: the field's adornment style with each
/// span's own over it, or, on a disabled field, the muted style alone.
fn paint_adornment(buf: &mut Buffer, area: Rect, line: &Line, style: Style, disabled: bool) {
    if area.is_empty() {
        return;
    }
    buf.set_style(area, style);
    if disabled {
        let text: String = line.spans.iter().map(|span| &*span.content).collect();
        buf.set_stringn(area.x, area.y, text, usize::from(area.width), style);
    } else {
        buf.set_line(area.x, area.y, line, area.width);
    }
}

/// Where a press at `pointer` puts the cursor. Beside the text in the well
/// is an adornment, and a press there means that end of the text, however
/// it is scrolled. Anywhere else it is the character under the pointer, or
/// one past the edge outside the well, so a drag keeps scrolling.
fn press_target(editor: &Editor<'_>, well: Rect, text: Rect, pointer: Position) -> CursorMove {
    if well.contains(pointer) && pointer.x < text.x {
        CursorMove::Head
    } else if well.contains(pointer) && pointer.x >= text.right() {
        CursorMove::End
    } else {
        cursor_at(editor, text, pointer)
    }
}

/// A single-line text field that only draws — an ordinary ratatui [`Widget`]
/// with no focus, events, or state of its own.
///
/// A themed paint adapter for [ratatui-textarea](ratatui_textarea).
///
/// It paints the [`InputState`] it is given: the text, scrolled to keep the
/// cursor in view, the cursor when [`focused`](Self::focused), and any
/// selection. Driving the editing is the caller's business, from the editor
/// [`paint`](Self::paint) hands back; [`Input`] is the field that does it for
/// you, and paints through this widget.
#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the four independent states a field paints; none combine into an enum"
)]
pub struct InputWidget<'a> {
    editor: Editor<'static>,
    placeholder: &'a str,
    prefix: Option<Line<'a>>,
    suffix: Option<Line<'a>>,
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
            prefix: None,
            suffix: None,
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

    /// The muted text shown while the field is empty, focused or not, where
    /// the text would start. A focused field shows its cursor on its first
    /// character.
    #[must_use]
    pub const fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Muted text before the editable text, inside the field: `https://`
    /// before a URL, an icon before a search. Never masked; a span's own
    /// style paints over the muted one.
    #[must_use]
    pub fn prefix(mut self, prefix: impl Into<Line<'a>>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Muted text after the editable text, inside the field: a unit, a
    /// domain. Never masked; a span's own style paints over the muted one.
    #[must_use]
    pub fn suffix(mut self, suffix: impl Into<Line<'a>>) -> Self {
        self.suffix = Some(suffix.into());
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
    /// changes: the state keeps the text as typed. A masked field never
    /// copies or cuts, so the secret cannot leave it that way.
    #[must_use]
    pub const fn mask_char(mut self, mask_char: char) -> Self {
        self.mask_char = Some(mask_char);
        self
    }

    /// Rows the field occupies: 1, or 3 with a [`title`](Self::title).
    #[must_use]
    pub const fn height(&self) -> u16 {
        field_height(self.title.is_some())
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
    /// now knows the view it was drawn in: how far it is scrolled.
    ///
    /// Rendering as a [`Widget`] throws that editor away. A loop that drives
    /// the editing itself edits this one instead, and stores the result with
    /// [`InputState::from_editor`], so the next paint scrolls from what is on
    /// screen. Which keys reach the editor is the loop's own policy; this is
    /// the least of one, and [`Input`] has the whole of it.
    ///
    /// ```
    /// use ratatui::{buffer::Buffer, layout::Rect};
    /// use ratcn::{
    ///     InputState, InputWidget,
    ///     runtime::{KeyCode, KeyEvent},
    ///     text_edit::{Editor, editor_input, is_editor_binding},
    /// };
    ///
    /// // Beside the state, the editor the last paint handed back, with the
    /// // version of the state it was painted from.
    /// let mut state = InputState::new("Ada Lovelace");
    /// let mut painted: Option<(u64, Editor<'static>)> = None;
    ///
    /// // Each frame:
    /// let area = Rect::new(0, 0, 8, 1);
    /// let mut buf = Buffer::empty(area);
    /// let editor = InputWidget::new(&state).focused(true).paint(area, &mut buf);
    /// painted = Some((state.version(), editor));
    ///
    /// // When a key arrives:
    /// fn on_key(
    ///     key: KeyEvent,
    ///     state: &mut InputState,
    ///     painted: &mut Option<(u64, Editor<'static>)>,
    /// ) {
    ///     match key.code {
    ///         KeyCode::Enter => { /* submit */ }
    ///         // Focus traversal, the enclosing view, and keys with nowhere to
    ///         // go on one line.
    ///         KeyCode::Tab | KeyCode::BackTab | KeyCode::Esc | KeyCode::Up | KeyCode::Down => {}
    ///         // Line breaks, shifted or not: Ctrl+J is a terminal's line feed.
    ///         KeyCode::Char('j' | 'J' | 'm' | 'M') if key.modifiers.ctrl => {}
    ///         KeyCode::Char('\n' | '\r') => {}
    ///         _ => {
    ///             if let Some(input) = editor_input(&key).filter(is_editor_binding) {
    ///                 // The painted editor, while the state is still the one
    ///                 // it was painted from.
    ///                 let mut editor = match painted.take() {
    ///                     Some((version, editor)) if version == state.version() => editor,
    ///                     _ => state.editor().clone(),
    ///                 };
    ///                 editor.input(input);
    ///                 *state = InputState::from_editor(editor);
    ///             }
    ///         }
    ///     }
    /// }
    ///
    /// on_key(KeyEvent::new(KeyCode::Left), &mut state, &mut painted);
    /// assert_eq!(state.cursor(), 11);
    /// # // A shifted chord is the same chord: neither line break edits.
    /// # use ratcn::runtime::Modifiers;
    /// # let shifted = |char| KeyEvent {
    /// #     code: KeyCode::Char(char),
    /// #     modifiers: Modifiers { ctrl: true, alt: false, shift: true },
    /// # };
    /// # on_key(shifted('J'), &mut state, &mut painted);
    /// # on_key(shifted('M'), &mut state, &mut painted);
    /// # assert_eq!(state.value(), "Ada Lovelace");
    /// # // A state the app replaces after a paint is the one edited.
    /// # let editor = InputWidget::new(&state).focused(true).paint(area, &mut buf);
    /// # painted = Some((state.version(), editor));
    /// # state = InputState::default();
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
                .render(field_rows(area, true), buf);
        }
        let titled = self.title.is_some();
        buf.set_style(well(area, titled), style.text);
        let parts = parts(area, titled, self.prefix.as_ref(), self.suffix.as_ref());
        for (adornment, cells) in [(&self.prefix, parts.prefix), (&self.suffix, parts.suffix)] {
            if let Some(line) = adornment {
                paint_adornment(buf, cells, line, style.adornment, self.disabled);
            }
        }
        let field = parts.text;

        // The state cleared the editor's own look when it adopted it; what
        // is left is what this field decides. The setters that re-measure the
        // text run only when the setting differs.
        let editor = &mut self.editor;
        if editor.wrap_mode() != WrapMode::None {
            editor.set_wrap_mode(WrapMode::None);
        }
        if editor.mask_char() != self.mask_char {
            match self.mask_char {
                Some(mask_char) => editor.set_mask_char(mask_char),
                None => editor.clear_mask_char(),
            }
        }
        editor.set_style(style.text);
        editor.set_cursor_style(style.cursor);
        editor.set_cursor_line_style(Style::default());
        editor.set_selection_style(style.selection);
        (&*editor).render(field, buf);
        // The field paints the placeholder itself: the editor's would start a
        // cell late, behind a cursor of its own.
        if editor.is_empty() && !self.placeholder.is_empty() {
            buf.set_stringn(
                field.x,
                field.y,
                single_line(self.placeholder),
                usize::from(field.width),
                style.placeholder,
            );
            if self.focused && !self.disabled && !field.is_empty() {
                buf.set_style(Rect::new(field.x, field.y, 1, 1), style.cursor);
            }
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
/// Built on [ratatui-textarea](ratatui_textarea), which handles editing,
/// cursor movement, selection, and horizontal scrolling.
///
/// The text lives in app state as an [`InputState`] and arrives through
/// [`value`](Self::value); every edit and every cursor movement emits a new
/// state for the app to store. Without that binding the field paints empty,
/// is not focusable, and answers no events.
///
/// The editor's keys all work — arrows, <kbd>Home</kbd>/<kbd>End</kbd>,
/// <kbd>Ctrl</kbd>+<kbd>←</kbd>/<kbd>→</kbd> by word, <kbd>Shift</kbd> with a
/// movement to select, <kbd>Ctrl+W</kbd>, <kbd>Ctrl+K</kbd>, and the rest of
/// the readline set; a key the editor binds is the field's even where it
/// changes nothing. <kbd>Enter</kbd> emits [`on_submit`](Self::on_submit)
/// and never inserts a line break. <kbd>Tab</kbd>, <kbd>Esc</kbd>, the
/// function keys, and the vertical keys are left to bubble, as is any chord
/// the editor does not bind, so focus traversal, an enclosing dialog, and the
/// app's shortcuts keep working around a focused field. So do two chords the
/// editor does bind: <kbd>Ctrl+M</kbd>, a line break, and <kbd>Ctrl+J</kbd>,
/// a terminal's line feed, which the editor reads as "delete to the start". A
/// paste is flattened to one line.
///
/// <kbd>Ctrl+C</kbd> and <kbd>Ctrl+X</kbd> copy and cut the selection to the
/// system clipboard, as a browser's copy and cut
/// ([`Event::Copy`], [`Event::Cut`]) do; a copy keeps the selection. With
/// nothing selected they bubble, so in a terminal the app's own
/// <kbd>Ctrl+C</kbd> keeps working. <kbd>Ctrl+Y</kbd> pastes the last copy or
/// cut back.
///
/// A click places the cursor at the character clicked, and a drag selects
/// from the character pressed to the one under the pointer, scrolling the
/// text as the pointer moves on past either end of the field.
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
    prefix: Option<Line<'static>>,
    suffix: Option<Line<'static>>,
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
            .field("prefix", &self.prefix)
            .field("suffix", &self.suffix)
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
            prefix: None,
            suffix: None,
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

    /// The muted text shown while the field is empty, focused or not, where
    /// the text would start. A focused field shows its cursor on its first
    /// character.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Muted text before the editable text, inside the field: `https://`
    /// before a URL, an icon before a search. The text starts a cell after
    /// it. Never masked; a span's own style paints over the muted one. A
    /// click on it puts the cursor at the start of the text.
    #[must_use]
    pub fn prefix(mut self, prefix: impl Into<Line<'static>>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Muted text after the editable text, inside the field: a unit, a
    /// domain. The text ends a cell before it. Never masked; a span's own
    /// style paints over the muted one. A click on it puts the cursor at the
    /// end of the text.
    #[must_use]
    pub fn suffix(mut self, suffix: impl Into<Line<'static>>) -> Self {
        self.suffix = Some(suffix.into());
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

    /// Rows the field occupies: 1, or 3 with a [`title`](Self::title).
    #[must_use]
    pub const fn height(&self) -> u16 {
        field_height(self.title.is_some())
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

    /// The cells of `area` the text is drawn in, between the adornments.
    fn text_rect(&self, area: Rect) -> Rect {
        let titled = self.title.is_some();
        parts(area, titled, self.prefix.as_ref(), self.suffix.as_ref()).text
    }

    /// The editor an event edits: the painted one while the state it was
    /// painted from is still the app's, since only it knows how far the view
    /// is scrolled. A state that changed since the paint is edited itself —
    /// it descends from a painted editor through the event that produced it.
    fn editor<'s>(&'s self, state: &'s InputState) -> &'s Editor<'static> {
        match &self.painted {
            Some((version, editor)) if *version == state.version() => editor,
            _ => state.editor(),
        }
    }

    /// The key policy. Enter submits and never reaches the editor; the keys
    /// in [`bubbles`] are not the field's. Of the rest, a key the editor binds
    /// is the field's: it emits the new state, or is consumed where it changes
    /// nothing — Left at the start of the text must not walk out into the
    /// enclosing component, and Ctrl+K at its end must not fire the app's
    /// shortcut. A key the editor does not bind bubbles, which is how the
    /// app's own shortcuts pass through a focused field.
    fn handle_key(&self, key: KeyEvent, state: &InputState) -> EventResult<InputState> {
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
        let modified = editor.input(input);
        if modified || before != (editor.cursor(), editor.selection_range()) {
            EventResult::Emit(InputState::from_editor(editor))
        } else {
            EventResult::Consumed
        }
    }

    /// Copy the selection to the clipboard, or cut it there. The editor's
    /// yank buffer takes it too, so Ctrl+Y pastes it back; a copy keeps the
    /// selection. With nothing selected there is nothing to copy, and the
    /// request bubbles: that is how an app's Ctrl+C reaches it through a
    /// focused field. A masked field never copies or cuts, as a
    /// browser's password field does not: the secret would leave it.
    fn handle_clip(
        &self,
        clip: Clip,
        state: &InputState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<InputState> {
        let mut editor = self.editor(state).clone();
        if self.mask_char.is_some() || editor.selection_range().is_none_or(|(from, to)| from == to)
        {
            return EventResult::Ignored;
        }
        match clip {
            Clip::Cut => {
                editor.cut();
            }
            Clip::Copy => {
                // Upstream's copy ends the selection, and a copy should not: a
                // second Ctrl+C would no longer find one, and bubble.
                let mut copied = editor.clone();
                copied.copy();
                editor.set_yank_text(copied.yank_text());
            }
        }
        ctx.set_clipboard(editor.yank_text());
        EventResult::Emit(InputState::from_editor(editor))
    }

    /// Insert a paste as one line.
    fn handle_paste(&self, text: &str, state: &InputState) -> EventResult<InputState> {
        let text = single_line(text);
        if text.is_empty() {
            return EventResult::Ignored;
        }
        let mut editor = self.editor(state).clone();
        editor.insert_str(text);
        EventResult::Emit(InputState::from_editor(editor))
    }

    /// The mouse policy. A press on the field claims the rest of the gesture
    /// and remembers the character it landed on, and is otherwise left to the
    /// runtime, which focuses the field: an event carries one message, and a
    /// press spends it on focus. The cursor moves when the gesture says what
    /// it is — to the pressed character on a click, and on a drag to the
    /// character under the pointer, selecting from the pressed one. A drag
    /// that comes back to where it began selects nothing.
    fn handle_mouse(
        &self,
        mouse: &MouseEvent,
        state: &InputState,
        ctx: &mut EventCtx<'_>,
    ) -> EventResult<InputState> {
        let well = well(ctx.area(), self.title.is_some());
        let text = self.text_rect(ctx.area());
        let pointer = Position::new(mouse.column, mouse.row);
        let painted = self.editor(state);
        let editor = match mouse.kind {
            MouseKind::Down(MouseButton::Left) if well.contains(pointer) => {
                ctx.capture_pointer(MouseButton::Left);
                *ctx.transient() = DragAnchor(press_target(painted, well, text, pointer));
                return EventResult::Ignored;
            }
            MouseKind::Click(MouseButton::Left) if ctx.pointer_captured() => {
                let mut editor = painted.clone();
                editor.cancel_selection();
                editor.move_cursor(press_target(&editor, well, text, pointer));
                editor
            }
            MouseKind::Drag(MouseButton::Left) if ctx.pointer_captured() => {
                let DragAnchor(anchor) = *ctx.transient();
                let mut editor = painted.clone();
                let pointer = cursor_at(&editor, text, pointer);
                select_dragged(&mut editor, anchor, pointer);
                editor
            }
            _ => return EventResult::Ignored,
        };
        if (painted.cursor(), painted.selection_range())
            == (editor.cursor(), editor.selection_range())
        {
            EventResult::Consumed
        } else {
            EventResult::Emit(InputState::from_editor(editor))
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
        if let Some(title) = &self.title {
            widget = widget.title(title);
        }
        if let Some(mask_char) = self.mask_char {
            widget = widget.mask_char(mask_char);
        }
        if let Some(prefix) = &self.prefix {
            widget = widget.prefix(prefix.clone());
        }
        if let Some(suffix) = &self.suffix {
            widget = widget.suffix(suffix.clone());
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
            Event::Key(key) if key.code == KeyCode::Enter => {
                return match &self.on_submit {
                    Some(on_submit) if !key.modifiers.any() => EventResult::Emit(on_submit()),
                    _ => EventResult::Ignored,
                };
            }
            Event::Key(key) => match clipboard_chord(*key) {
                Some(clip) => self.handle_clip(clip, read(state), ctx),
                None => self.handle_key(*key, read(state)),
            },
            Event::Copy => self.handle_clip(Clip::Copy, read(state), ctx),
            Event::Cut => self.handle_clip(Clip::Cut, read(state), ctx),
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

    /// The field's rows, or nothing when its edges and the adornments leave
    /// no text cell: a field that cannot show its text takes no focus,
    /// typing, or clicks.
    fn interaction_area(&self, area: Rect, _state: &S) -> Rect {
        if self.text_rect(area).is_empty() {
            Rect::default()
        } else {
            field_rows(area, self.title.is_some())
        }
    }
}

/// What a copy or cut request asks for.
#[derive(Clone, Copy)]
enum Clip {
    Copy,
    Cut,
}

/// Ctrl+C or Ctrl+X. They act on a selection only, so they are the field's to
/// route rather than the editor's.
fn clipboard_chord(key: KeyEvent) -> Option<Clip> {
    if !key.modifiers.ctrl || key.modifiers.alt {
        return None;
    }
    match key.code {
        KeyCode::Char('c' | 'C') => Some(Clip::Copy),
        KeyCode::Char('x' | 'X') => Some(Clip::Cut),
        _ => None,
    }
}

/// The keys a field leaves alone, whatever the editor binds them to. Tab and
/// `BackTab` belong to focus traversal, Esc and the function keys to enclosing
/// components and the app. One line has no vertical movement to make, so the
/// vertical keys go too, with the chords the editor binds to the same moves.
/// Ctrl+M and a raw line break would insert a newline, and Ctrl+J — a line
/// feed, as a terminal reports it — would delete back to the start of the line.
fn bubbles(key: KeyEvent) -> bool {
    let Modifiers { ctrl, alt, .. } = key.modifiers;
    match key.code {
        KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Esc
        | KeyCode::F(_)
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::PageUp
        | KeyCode::PageDown
        // A raw line break, under any modifiers.
        | KeyCode::Char('\n' | '\r') => true,
        KeyCode::Char(char) => match (ctrl, alt) {
            (false, false) => false,
            (true, false) => matches!(char.to_ascii_lowercase(), 'j' | 'm' | 'n' | 'p' | 'v'),
            (false, true) => matches!(
                char.to_ascii_lowercase(),
                'n' | 'p' | 'v' | '<' | '>' | '[' | ']'
            ),
            (true, true) => matches!(char.to_ascii_lowercase(), 'n' | 'p'),
        },
        _ => false,
    }
}

/// A paste, or a placeholder, as one line: each line break and each tab
/// becomes a space, and every other control character is dropped.
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
    use crate::runtime::{ChildId, FocusState, Ratcn, ScrollDirection};
    use crate::test_support::{Driver, key, key_with, mouse, styled_snapshot};
    use crate::{Dialog, ScrollArea};

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
    const ALT: Modifiers = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    const CTRL_SHIFT: Modifiers = Modifiers {
        ctrl: true,
        alt: false,
        shift: true,
    };
    const CTRL_ALT: Modifiers = Modifiers {
        ctrl: true,
        alt: true,
        shift: false,
    };

    /// The field every test declares unless it says otherwise: eight columns
    /// wide, so a ten-character value has to scroll.
    const FIELD: Rect = Rect::new(0, 0, 8, 1);

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

    /// Route one event that may emit nothing, storing what it does emit.
    fn route(driver: &mut Driver<State, Msg>, state: &mut State, event: Event) {
        match driver.event(event, state) {
            EventResult::Emit(Msg::Name(name)) => state.name = name,
            EventResult::Emit(Msg::Focus(focus)) => state.focus = focus,
            EventResult::Emit(Msg::Submit) => panic!("the mouse never submits"),
            EventResult::Consumed | EventResult::Ignored => {}
        }
    }

    /// Press and release the primary button on one cell.
    fn click(driver: &mut Driver<State, Msg>, state: &mut State, column: u16, row: u16) {
        route(driver, state, mouse(LEFT_DOWN, column, row));
        route(driver, state, mouse(LEFT_UP, column, row));
    }

    /// The selected characters, as `(from, to)` indexes into the value.
    fn selection(state: &State) -> Option<(usize, usize)> {
        state
            .name
            .editor()
            .selection_range()
            .map(|((_, from), (_, to))| (from, to))
    }

    const LEFT_DOWN: MouseKind = MouseKind::Down(MouseButton::Left);
    const LEFT_DRAG: MouseKind = MouseKind::Drag(MouseButton::Left);
    const LEFT_UP: MouseKind = MouseKind::Up(MouseButton::Left);

    /// What the field's text columns show.
    fn field(driver: &Driver<State, Msg>) -> String {
        driver
            .row(0)
            .chars()
            .take(usize::from(FIELD.width))
            .collect()
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
        assert_eq!(field(&driver), "defghij ", "scrolled to the end cursor");

        send(&mut driver, &mut state, key(KeyCode::Left));
        render(&mut driver, &state);
        assert_eq!(field(&driver), "defghij ", "the view must not shift");
        assert_eq!(
            driver.cell(6, 0).bg,
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
        assert_eq!(field(&driver), "defghXij", "the view must not shift");
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
            // A raw line feed arrives as Ctrl+J, which the editor binds to
            // deleting back to the start of the line: typed Enter would wipe
            // the value.
            key_with(KeyCode::Char('j'), CTRL),
            key(KeyCode::Char('\n')),
            key(KeyCode::Char('\r')),
            // However a line break arrives, it must not split the line.
            key_with(KeyCode::Char('\n'), CTRL_ALT),
            key_with(KeyCode::Char('\r'), CTRL_ALT),
            key_with(KeyCode::Char('\n'), ALT),
            key_with(KeyCode::Char('\r'), CTRL),
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
    /// Esc would keep its dialog open. One line has no vertical movement, so
    /// the vertical keys and the editor's chords for them are the app's too.
    /// Undo has no history to act on, and a chord the editor does not bind is
    /// the app's shortcut.
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
            key_with(KeyCode::Char('n'), CTRL),
            key_with(KeyCode::Char('p'), CTRL),
            key_with(KeyCode::Char('v'), CTRL),
            key_with(KeyCode::Char('v'), ALT),
            key_with(KeyCode::Char('<'), ALT),
            key_with(KeyCode::Char('u'), CTRL),
            key_with(KeyCode::Char('r'), CTRL),
            key_with(KeyCode::Char('s'), CTRL),
            key_with(KeyCode::Char('x'), ALT),
        ] {
            assert!(
                matches!(driver.event(event.clone(), &state), EventResult::Ignored),
                "{event:?} must bubble"
            );
        }
    }

    /// Keys route by binding, not by effect. A chord the editor binds is
    /// the field's wherever the cursor is: if Ctrl+K at the end of the text
    /// bubbled, the app's Ctrl+K shortcut would fire or not depending on
    /// where the cursor happened to be.
    #[test]
    fn a_bound_chord_is_consumed_even_where_it_changes_nothing() {
        let mut driver = driver();
        let state = state("name");
        render(&mut driver, &state);

        for event in [
            key_with(KeyCode::Char('k'), CTRL),
            key_with(KeyCode::Char('d'), CTRL),
            key_with(KeyCode::Char('e'), CTRL),
            key_with(KeyCode::Char('f'), ALT),
            key_with(KeyCode::Char('K'), CTRL_SHIFT),
            key_with(KeyCode::Right, CTRL),
        ] {
            assert!(
                matches!(driver.event(event.clone(), &state), EventResult::Consumed),
                "{event:?} must be consumed"
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

    /// A control character typed as text would sit in the value unseen: a
    /// tab character, or an escape for the terminal to act on when the value
    /// is shown again.
    #[test]
    fn a_control_character_is_never_typed() {
        let mut driver = driver();
        let state = state("name");
        render(&mut driver, &state);

        for char in ['\t', '\u{1b}', '\u{7}'] {
            assert!(
                matches!(
                    driver.event(key(KeyCode::Char(char)), &state),
                    EventResult::Ignored
                ),
                "{char:?}"
            );
        }
    }

    /// Copy and cut put the selection on the system clipboard, and in the
    /// editor's yank buffer too, so Ctrl+Y still pastes it back. A copy
    /// leaves the selection as it was, as every editor does. Ctrl+C and
    /// Ctrl+X do it from the keyboard; `Event::Copy` and `Event::Cut` are
    /// the platform's own gesture, a browser's copy say.
    #[test]
    fn copy_and_cut_put_the_selection_on_the_clipboard() {
        for (event, cut) in [
            (key_with(KeyCode::Char('c'), CTRL), false),
            (Event::Copy, false),
            (key_with(KeyCode::Char('x'), CTRL), true),
            (Event::Cut, true),
        ] {
            let mut driver = driver();
            let mut state = state("Ada Lovelace");
            render(&mut driver, &state);
            send(&mut driver, &mut state, key(KeyCode::Home));
            for _ in 0..3 {
                send(&mut driver, &mut state, key_with(KeyCode::Right, SHIFT));
            }
            render(&mut driver, &state);

            send(&mut driver, &mut state, event.clone());

            assert_eq!(
                driver.ratcn.take_clipboard().as_deref(),
                Some("Ada"),
                "{event:?}"
            );
            let left = if cut { " Lovelace" } else { "Ada Lovelace" };
            assert_eq!(state.name.value(), left, "{event:?}");
            // A copy keeps the selection, so a second Ctrl+C copies again
            // rather than bubbling to the app's quit; a cut takes it away.
            let kept = if cut { None } else { Some((0, 3)) };
            assert_eq!(selection(&state), kept, "{event:?}");
            render(&mut driver, &state);
            send(&mut driver, &mut state, key_with(KeyCode::Char('y'), CTRL));
            assert!(
                state.name.value().contains("Ada"),
                "{event:?} fills the yank buffer"
            );
        }
    }

    /// With nothing selected there is nothing to copy, and Ctrl+C is the
    /// app's again: its quit key has to work with a field focused. An empty
    /// selection, a drag that came back to its start, counts as none.
    #[test]
    fn copy_and_cut_without_a_selection_bubble() {
        let mut driver = driver();
        let mut state = state("Ada");
        render(&mut driver, &state);
        send(&mut driver, &mut state, key_with(KeyCode::Left, SHIFT));
        send(&mut driver, &mut state, key_with(KeyCode::Right, SHIFT));
        assert!(state.name.editor().is_selecting());
        render(&mut driver, &state);

        for event in [
            key_with(KeyCode::Char('c'), CTRL),
            key_with(KeyCode::Char('x'), CTRL),
            Event::Copy,
            Event::Cut,
        ] {
            assert!(
                matches!(driver.event(event.clone(), &state), EventResult::Ignored),
                "{event:?} must bubble"
            );
        }
        assert_eq!(driver.ratcn.take_clipboard(), None);
    }

    /// A masked field holds a secret, and copying it would put the secret on
    /// the clipboard: it never copies or cuts, as a browser's password field
    /// does not. A paste still goes in.
    #[test]
    fn a_masked_field_never_copies_or_cuts() {
        let masked = || {
            Input::new()
                .value(|state: &State| &state.name, Msg::Name)
                .mask_char('•')
        };
        let mut driver = driver();
        let mut state = state("secret");
        render_with(&mut driver, &state, masked);
        send(&mut driver, &mut state, key_with(KeyCode::Home, SHIFT));
        render_with(&mut driver, &state, masked);

        for event in [
            key_with(KeyCode::Char('c'), CTRL),
            key_with(KeyCode::Char('x'), CTRL),
            Event::Copy,
            Event::Cut,
        ] {
            assert!(
                matches!(driver.event(event.clone(), &state), EventResult::Ignored),
                "{event:?} must bubble"
            );
        }
        assert_eq!(driver.ratcn.take_clipboard(), None);
        assert_eq!(state.name.value(), "secret");
        send(&mut driver, &mut state, Event::Paste("s".to_owned()));
        assert_eq!(state.name.value(), "s", "the paste replaced the selection");
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

    /// Disabled is the loudest state: no keys, no paste, no mouse, no focus.
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
            mouse(LEFT_DOWN, 1, 0),
            mouse(MouseKind::Click(MouseButton::Left), 1, 0),
            mouse(LEFT_DRAG, 2, 0),
        ] {
            assert!(
                matches!(driver.event(event.clone(), &state), EventResult::Ignored),
                "{event:?}"
            );
        }
        assert_eq!(
            driver.cell(5, 0).bg,
            InputStyle::from_theme(&Theme::default_dark()).background,
            "a disabled field shows no cursor"
        );
    }

    /// A titled field two columns wide is all border: no text can be drawn,
    /// so a field there would edit text nobody sees. Focus passes it by for
    /// the next control until the field is given room again.
    #[test]
    fn a_field_with_no_text_cells_is_skipped_until_it_has_room() {
        let mut driver = Driver::with(
            Ratcn::new()
                .focus(|state: &State| &state.focus, Msg::Focus)
                .tab_wrap(crate::runtime::TabWrap::Wrap),
            12,
            3,
        );
        let mut state = state("");
        let declare = |driver: &mut Driver<State, Msg>, state: &State, field: Rect| {
            driver.render(state, |ctx| {
                ctx.component(
                    ChildId::Static("name"),
                    Input::new()
                        .value(|state: &State| &state.name, Msg::Name)
                        .title("T"),
                    field,
                );
                ctx.component(
                    ChildId::Static("save"),
                    crate::Button::new("Save").on_press(|| Msg::Submit),
                    Rect::new(4, 1, 8, 1),
                );
                ctx.component(
                    ChildId::Static("cancel"),
                    crate::Button::new("Cancel").on_press(|| Msg::Submit),
                    Rect::new(4, 2, 8, 1),
                );
            });
        };

        declare(&mut driver, &state, Rect::new(0, 0, 2, 3));
        assert!(
            driver
                .ratcn
                .focus_path(&[ChildId::Static("name")])
                .is_none()
        );
        send(&mut driver, &mut state, key(KeyCode::Tab));
        assert_eq!(
            state.focus.path(),
            [ChildId::Static("cancel")],
            "startup focus went to Save, so Tab moves on to Cancel"
        );
        declare(&mut driver, &state, Rect::new(0, 0, 2, 3));
        send(&mut driver, &mut state, key(KeyCode::Tab));
        assert_eq!(
            state.focus.path(),
            [ChildId::Static("save")],
            "Tab wraps past the field"
        );
        declare(&mut driver, &state, Rect::new(0, 0, 2, 3));
        for event in [
            key(KeyCode::Char('x')),
            Event::Paste("x".to_owned()),
            mouse(LEFT_DOWN, 0, 0),
        ] {
            assert!(
                !matches!(
                    driver.event(event.clone(), &state),
                    EventResult::Emit(Msg::Name(_))
                ),
                "{event:?}"
            );
        }

        state.focus = FocusState::default();
        declare(&mut driver, &state, Rect::new(0, 0, 4, 3));
        let EventResult::Emit(Msg::Name(typed)) = driver.event(key(KeyCode::Char('x')), &state)
        else {
            panic!("the enlarged field takes startup focus and typing");
        };
        assert_eq!(typed.value(), "x");
        let EventResult::Emit(Msg::Name(pasted)) =
            driver.event(Event::Paste("y".to_owned()), &state)
        else {
            panic!("the enlarged field takes a paste");
        };
        assert_eq!(pasted.value(), "y");
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

    /// A press has one message to give, and an unfocused field needs it for
    /// focus: typing into a field that took the cursor but not the keyboard
    /// would go somewhere else. The cursor follows on the release.
    #[test]
    fn a_click_focuses_the_field_and_places_the_cursor() {
        let mut driver = driver();
        let mut state = State {
            focus: FocusState::none(),
            ..state("name")
        };
        render(&mut driver, &state);

        let EventResult::Emit(Msg::Focus(focus)) = driver.event(mouse(LEFT_DOWN, 1, 0), &state)
        else {
            panic!("a press must move focus to the field");
        };
        assert_eq!(focus.path(), [ChildId::Static("name")]);
        state.focus = focus;
        render(&mut driver, &state);

        send(&mut driver, &mut state, mouse(LEFT_UP, 1, 0));
        assert_eq!(state.name.cursor(), 1);
    }

    /// The cursor goes to the character that was clicked, whatever its
    /// width, and to the end of the text from anywhere past it.
    #[test]
    fn a_click_places_the_cursor_on_the_character_under_it() {
        let mut driver = driver();
        let mut state = state("a日本b");
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(ChildId::Static("name"), input(), Rect::new(0, 0, 12, 1));
            });
        };
        render(&mut driver, &state);

        // Columns: a 0, 日 1–2, 本 3–4, b 5.
        for (column, cursor) in [
            (0, 0),
            (1, 1),
            (2, 1),
            (3, 2),
            (4, 2),
            (5, 3),
            (6, 4),
            (10, 4),
        ] {
            click(&mut driver, &mut state, column, 0);
            assert_eq!(state.name.cursor(), cursor, "a click on column {column}");
            render(&mut driver, &state);
        }
    }

    /// A long value is scrolled, and a click means the character on screen,
    /// not the one that would be there unscrolled.
    #[test]
    fn a_click_in_a_scrolled_field_counts_from_what_is_on_screen() {
        let mut driver = driver();
        let mut state = state("abcdefghij");
        render(&mut driver, &state);
        assert_eq!(field(&driver), "defghij ");

        click(&mut driver, &mut state, 2, 0);
        assert_eq!(state.name.cursor(), 5, "the f");
        render(&mut driver, &state);
        assert_eq!(
            field(&driver),
            "defghij ",
            "a click must not scroll the view"
        );
    }

    /// The text of a titled field starts inside its border, and the border
    /// itself is not text: a press there focuses and moves no cursor.
    #[test]
    fn a_click_in_a_titled_field_counts_from_inside_the_border() {
        let mut driver = driver();
        let mut state = state("hello");
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    ChildId::Static("name"),
                    input().title("Name"),
                    Rect::new(0, 0, 12, 3),
                );
            });
        };
        render(&mut driver, &state);
        assert_eq!(driver.row(1), "│hello     │");

        click(&mut driver, &mut state, 3, 1);
        assert_eq!(state.name.cursor(), 2);

        render(&mut driver, &state);
        click(&mut driver, &mut state, 5, 0);
        assert_eq!(state.name.cursor(), 2, "the border is not text");
    }

    /// A border is edge enough: a titled field's text runs from border to
    /// border, and the cursor sits in the last cell before the right one
    /// without scrolling the text.
    #[test]
    fn a_titled_fields_text_runs_from_border_to_border() {
        let theme = Theme::default_dark();
        let area = Rect::new(0, 0, 12, 3);
        let mut buffer = Buffer::empty(area);
        InputWidget::new(&InputState::new("abcdefghi"))
            .themed(&theme)
            .title("T")
            .focused(true)
            .render(area, &mut buffer);

        let row: String = (0..12).map(|x| buffer[(x, 1)].symbol()).collect();
        assert_eq!(row, "│abcdefghi │");
        assert_eq!(
            buffer[(10, 1)].bg,
            InputStyle::from_theme(&theme).cursor_background
        );
    }

    /// A drag selects from the character pressed to the one under the
    /// pointer, in either direction, and the selection is a real one: the
    /// next character typed replaces it.
    #[test]
    fn a_drag_selects_and_typing_replaces_the_selection() {
        let mut driver = driver();
        let mut state = state("hello");
        render(&mut driver, &state);

        route(&mut driver, &mut state, mouse(LEFT_DOWN, 1, 0));
        assert_eq!(selection(&state), None);
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 4, 0));
        assert_eq!((selection(&state), state.name.cursor()), (Some((1, 4)), 4));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 0, 0));
        assert_eq!((selection(&state), state.name.cursor()), (Some((0, 1)), 0));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 3, 0));
        route(&mut driver, &mut state, mouse(LEFT_UP, 3, 0));
        assert_eq!(selection(&state), Some((1, 3)), "the release keeps it");

        render(&mut driver, &state);
        send(&mut driver, &mut state, key(KeyCode::Char('X')));
        assert_eq!(state.name.value(), "hXlo");
    }

    /// An empty selection is still a selection to the editor: the next
    /// arrow key would extend it. Neither a click nor a drag that returns to
    /// where it began may leave one — nor an older selection standing.
    #[test]
    fn a_click_or_a_drag_back_to_its_start_leaves_no_selection() {
        let mut driver = driver();
        let mut state = state("hello");
        render(&mut driver, &state);

        send(&mut driver, &mut state, key_with(KeyCode::Home, SHIFT));
        assert!(state.name.editor().is_selecting());
        click(&mut driver, &mut state, 2, 0);
        assert!(!state.name.editor().is_selecting(), "a click deselects");
        assert_eq!(state.name.cursor(), 2);

        route(&mut driver, &mut state, mouse(LEFT_DOWN, 2, 0));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 4, 0));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 2, 0));
        route(&mut driver, &mut state, mouse(LEFT_UP, 2, 0));
        assert!(!state.name.editor().is_selecting());
        assert_eq!(state.name.cursor(), 2);
    }

    /// A selection often has to reach text that is scrolled out of sight.
    /// The pointer keeps its hold on the field after leaving it, and every
    /// move it makes out there takes the selection one character further and
    /// the view with it.
    #[test]
    fn a_drag_past_the_edge_keeps_extending_and_scrolls() {
        let area = Rect::new(3, 1, 8, 1);
        let mut driver = driver();
        let mut state = state("abcdefghij");
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(ChildId::Static("name"), input(), area);
            });
        };
        let shown = |driver: &Driver<State, Msg>| driver.row(1)[3..11].to_owned();
        render(&mut driver, &state);
        assert_eq!(shown(&driver), "defghij ");

        route(&mut driver, &mut state, mouse(LEFT_DOWN, 8, 1));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 1, 0));
        assert_eq!(selection(&state), Some((2, 8)), "one past the edge");
        render(&mut driver, &state);
        assert_eq!(shown(&driver), "cdefghij");

        send(&mut driver, &mut state, mouse(LEFT_DRAG, 0, 0));
        assert_eq!(selection(&state), Some((1, 8)), "and one more");
        render(&mut driver, &state);
        assert_eq!(shown(&driver), "bcdefghi");

        route(&mut driver, &mut state, mouse(LEFT_UP, 0, 0));
        assert_eq!(selection(&state), Some((1, 8)), "the release moves nothing");

        // And out the other side, back through the character pressed.
        route(&mut driver, &mut state, mouse(LEFT_DOWN, 3, 1));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 11, 2));
        assert_eq!(selection(&state), Some((1, 9)));
    }

    /// A mask draws one cell per character. A click has to count those
    /// cells: counted by the width of the hidden text, it would land on the
    /// wrong character and give the secret's shape away.
    #[test]
    fn a_click_in_a_masked_field_counts_mask_cells() {
        let mut driver = driver();
        let mut state = state("日本語ab");
        render_with(&mut driver, &state, || input().mask_char('*'));
        assert_eq!(field(&driver), "*****   ");

        for (column, cursor) in [(1, 1), (2, 2), (4, 4)] {
            click(&mut driver, &mut state, column, 0);
            assert_eq!(state.name.cursor(), cursor, "a click on column {column}");
            render_with(&mut driver, &state, || input().mask_char('*'));
        }
    }

    /// One line has nowhere to scroll, and a field that ate the wheel would
    /// stop the form around it from scrolling whenever the pointer crossed
    /// it.
    #[test]
    fn the_wheel_is_left_to_whatever_encloses_the_field() {
        let mut driver = driver();
        let state = state("abcdefghij");
        render(&mut driver, &state);

        for direction in [ScrollDirection::Up, ScrollDirection::Down] {
            assert!(matches!(
                driver.event(mouse(MouseKind::Scroll(direction), 1, 0), &state),
                EventResult::Ignored
            ));
        }
    }

    /// Inside a scroll area the field is declared in content coordinates
    /// and painted somewhere else on screen. The click arrives in the
    /// coordinates the field was declared in, so it still finds its
    /// character.
    #[test]
    fn a_click_finds_its_character_inside_a_scrolled_scroll_area() {
        let mut driver = driver();
        let mut state = state("hello");
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(
                    ChildId::Static("scroll"),
                    ScrollArea::new(8)
                        .scroll(|_: &State| 4, |_| Msg::Submit)
                        .content(|ctx| {
                            let area = ctx.area();
                            ctx.component(
                                ChildId::Static("name"),
                                input().title("Name"),
                                Rect::new(area.x, area.y + 4, 10, 3),
                            );
                        }),
                    Rect::new(1, 0, 11, 3),
                );
            });
        };
        render(&mut driver, &state);
        // Content row 5 is screen row 1, and the text starts at column 2.
        assert!(
            driver.row(1).starts_with(" │hello   │"),
            "{}",
            driver.row(1)
        );

        click(&mut driver, &mut state, 4, 1);
        assert_eq!(state.name.cursor(), 2);
    }

    /// A dialog is a layer: it paints through a clipped buffer of its own,
    /// in screen coordinates. The click has to agree with that paint too.
    #[test]
    fn a_click_finds_its_character_inside_a_dialog() {
        let mut driver = Driver::with(
            Ratcn::new().focus(|state: &State| &state.focus, Msg::Focus),
            40,
            10,
        );
        let mut state = state("hello");
        let area = driver.area();
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.modal(
                    ChildId::Static("dialog"),
                    Dialog::new().title("Rename").content(1, |ctx| {
                        ctx.component(ChildId::Static("name"), input(), ctx.area());
                    }),
                    area,
                );
            });
        };
        render(&mut driver, &state);
        let (row, text) = (0..10)
            .map(|row| (row, driver.row(row)))
            .find(|(_, text)| text.contains("hello"))
            .expect("the field is painted");
        let column = u16::try_from(text.chars().position(|char| char == 'h').expect("found"))
            .expect("on screen");

        click(&mut driver, &mut state, column + 3, row);
        assert_eq!(state.name.cursor(), 3);
    }

    /// A mask must hide the text's shape as well as its characters: every
    /// character is one mask cell, so wide characters scroll by the mask's
    /// width. Measured by their own, this field would scroll past its text
    /// and paint blank.
    #[test]
    fn a_masked_field_paints_only_the_mask_and_scrolls_by_its_width() {
        let mut driver = driver();
        let mut state = state("日本語日本語日本");
        render_with(&mut driver, &state, || input().mask_char('*'));
        assert_eq!(field(&driver), "******* ", "eight masks, scrolled by one");

        send(&mut driver, &mut state, key(KeyCode::Char('字')));
        render_with(&mut driver, &state, || input().mask_char('*'));
        assert_eq!(field(&driver), "******* ");
        assert_eq!(
            state.name.value(),
            "日本語日本語日本字",
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

    /// The placeholder says what belongs in an empty field, and stays while
    /// it is empty: a field that dropped it on focus would lose its label
    /// the moment the user arrived to fill it in. Text replaces it.
    #[test]
    fn the_placeholder_shows_while_the_field_is_empty() {
        let theme = Theme::default_dark();
        let style = InputStyle::from_theme(&theme);
        let area = Rect::new(0, 0, 8, 1);
        let paint = |state: &InputState, focused| {
            let mut buffer = Buffer::empty(area);
            InputWidget::new(state)
                .themed(&theme)
                .placeholder("Name")
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

        let unfocused = paint(&InputState::default(), false);
        assert_eq!(symbols(&unfocused), "Name    ");
        assert_eq!(unfocused[(0, 0)].fg, style.placeholder_foreground);
        let focused = paint(&InputState::default(), true);
        assert_eq!(symbols(&focused), "Name    ");
        assert_eq!(focused[(1, 0)].fg, style.placeholder_foreground);
        assert_eq!(symbols(&paint(&InputState::new("Ada"), true)), "Ada     ");
    }

    /// The placeholder stands where the text will. Started a cell later, the
    /// first key typed would visibly shift the field's contents left.
    #[test]
    fn the_placeholder_starts_in_the_column_the_text_does() {
        let area = Rect::new(0, 0, 12, 3);
        let column_of = |state: &InputState, titled, focused, symbol| {
            let widget = InputWidget::new(state).placeholder("Name").focused(focused);
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
                assert_eq!(
                    column_of(&InputState::default(), titled, focused, "N"),
                    column_of(&InputState::new("Ada"), titled, focused, "A"),
                    "titled {titled}, focused {focused}"
                );
            }
        }
    }

    /// A placeholder is one row, like the text it stands in for: a line
    /// break or a tab in it shows as a space rather than whatever the
    /// terminal makes of a control character.
    #[test]
    fn the_placeholder_is_one_line_of_plain_text() {
        let area = Rect::new(0, 0, 12, 1);
        let mut buffer = Buffer::empty(area);
        InputWidget::new(&InputState::default())
            .placeholder("a\nb\r\nc\td\u{1b}e")
            .render(area, &mut buffer);
        let symbols: String = buffer
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert_eq!(symbols, "a b c de    ");
    }

    /// A layout sizes the field from what it will draw: one row, or three
    /// with a titled border.
    #[test]
    fn the_height_counts_the_border() {
        let state = InputState::default();
        assert_eq!(InputWidget::new(&state).height(), 1);
        assert_eq!(InputWidget::new(&state).title("Name").height(), 3);
        assert_eq!(input().height(), 1);
        assert_eq!(input().title("Name").height(), 3);
    }

    /// A focused empty field still shows where typing goes: the cursor
    /// rests on the placeholder's first character, the way a block cursor
    /// rests on the character under it, and the rest stays muted.
    #[test]
    fn a_focused_empty_field_shows_the_cursor_on_the_placeholder() {
        let theme = Theme::default_dark();
        let style = InputStyle::from_theme(&theme);
        let area = Rect::new(0, 0, 8, 1);
        let mut buffer = Buffer::empty(area);
        InputWidget::new(&InputState::default())
            .themed(&theme)
            .placeholder("Name")
            .focused(true)
            .render(area, &mut buffer);

        let first = &buffer[(0, 0)];
        assert_eq!(
            (first.symbol(), first.fg, first.bg),
            ("N", style.cursor_foreground, style.cursor_background)
        );
        assert_eq!(buffer[(1, 0)].fg, style.placeholder_foreground);
    }

    /// A paint-only field driven by hand keeps its view only if the loop
    /// edits the editor that was painted, which alone knows how far the view
    /// is scrolled. `paint` hands it back for exactly that.
    #[test]
    fn a_standalone_loop_edits_the_painted_editor_and_keeps_its_view() {
        let area = FIELD;
        let shown = |buffer: &Buffer| {
            buffer
                .content
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
        };
        let mut buffer = Buffer::empty(area);
        let mut editor = InputWidget::new(&InputState::new("abcdefghij"))
            .focused(true)
            .paint(area, &mut buffer);
        assert_eq!(shown(&buffer), "defghij ");

        editor.input(editor_input(&KeyEvent::new(KeyCode::Left)).expect("an editor key"));
        let state = InputState::from_editor(editor);
        let mut buffer = Buffer::empty(area);
        InputWidget::new(&state)
            .focused(true)
            .render(area, &mut buffer);
        assert_eq!(shown(&buffer), "defghij ", "the view must not shift");
    }

    /// A state can be built from any editor, configured any way. The field
    /// paints its own look over whatever it was given: a border, line
    /// numbers, or right alignment left in place would move the text away
    /// from where a click is read, and the click would land on the wrong
    /// character.
    #[test]
    fn a_foreign_editors_settings_do_not_reach_the_paint() {
        let mut foreign = Editor::from(["hello"]);
        foreign.set_block(Block::bordered());
        foreign.set_line_number_style(Style::default());
        foreign.set_alignment(ratatui::layout::Alignment::Right);
        foreign.set_wrap_mode(crate::text_edit::WrapMode::WordOrGlyph);
        foreign.set_mask_char('*');
        let mut driver = driver();
        let mut state = State {
            name: InputState::from_editor(foreign),
            ..State::default()
        };
        render(&mut driver, &state);
        assert_eq!(field(&driver), "hello   ");

        click(&mut driver, &mut state, 3, 0);
        assert_eq!(state.name.cursor(), 3);
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

    /// Disabled wins over invalid everywhere, the border included: a field
    /// the user cannot edit must not ask them to fix it.
    #[test]
    fn a_disabled_field_draws_no_invalid_border() {
        let theme = Theme::default_dark();
        let style = InputStyle::from_theme(&theme);
        let area = Rect::new(0, 0, 8, 3);
        let state = InputState::new("x");
        let border = |disabled| {
            let mut buffer = Buffer::empty(area);
            InputWidget::new(&state)
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

    /// Paint one adorned widget into a fresh buffer of `area` and return the
    /// buffer.
    fn paint_adorned(area: Rect, widget: InputWidget<'_>) -> Buffer {
        let mut buffer = Buffer::empty(area);
        widget.render(area, &mut buffer);
        buffer
    }

    fn symbols(buffer: &Buffer) -> String {
        buffer
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    /// An adornment sits inside the field, a cell apart from the text, and
    /// the placeholder starts where the text will: after the prefix, not on
    /// top of it.
    #[test]
    fn adornments_sit_a_cell_apart_from_the_text_and_the_placeholder() {
        let theme = Theme::default_dark();
        let style = InputStyle::from_theme(&theme);
        let area = Rect::new(0, 0, 16, 1);
        let empty = InputState::default();
        let ada = InputState::new("ada");
        let widget = |state| InputWidget::new(state).themed(&theme);

        let prefixed = paint_adorned(area, widget(&ada).prefix("https://"));
        assert_eq!(symbols(&prefixed), "https:// ada    ");
        assert_eq!(prefixed[(0, 0)].fg, style.adornment_foreground);
        assert_eq!(prefixed[(9, 0)].fg, style.foreground);
        assert_eq!(
            symbols(&paint_adorned(area, widget(&ada).suffix(".com"))),
            "ada         .com"
        );
        assert_eq!(
            symbols(&paint_adorned(
                area,
                widget(&empty).prefix("$").suffix("kg").placeholder("Name")
            )),
            "$ Name        kg"
        );
        let placeholder = paint_adorned(area, widget(&empty).prefix("$").placeholder("Name"));
        assert_eq!(placeholder[(2, 0)].fg, style.placeholder_foreground);
    }

    /// An emoji or a CJK prefix takes two cells, and the text has to start
    /// after both: measured by characters it would paint over the second.
    #[test]
    fn a_wide_prefix_measures_in_cells() {
        let area = Rect::new(0, 0, 10, 1);
        let state = InputState::new("ab");
        let buffer = paint_adorned(area, InputWidget::new(&state).prefix("🔍"));
        assert_eq!(buffer[(0, 0)].symbol(), "🔍");
        assert_eq!(
            (buffer[(3, 0)].symbol(), buffer[(4, 0)].symbol()),
            ("a", "b")
        );
    }

    /// A span's own style paints over the muted one, and disabled mutes it
    /// all again, as it does the text.
    #[test]
    fn a_styled_span_shows_unless_the_field_is_disabled() {
        let theme = Theme::default_dark();
        let style = InputStyle::from_theme(&theme);
        let area = Rect::new(0, 0, 10, 1);
        let state = InputState::new("ab");
        let prefix = || {
            Line::styled(
                "$",
                Style::default()
                    .fg(Color::Green)
                    .bg(Color::Blue)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            )
        };
        let paint = |disabled| {
            let cell = paint_adorned(
                area,
                InputWidget::new(&state)
                    .themed(&theme)
                    .prefix(prefix())
                    .disabled(disabled),
            )[(0, 0)]
                .clone();
            (cell.fg, cell.bg, cell.modifier.is_empty())
        };
        assert_eq!(paint(false), (Color::Green, Color::Blue, false));
        assert_eq!(
            paint(true),
            (style.disabled_foreground, style.background, true),
            "nothing of the span's own look survives"
        );
    }

    /// A mask hides the secret, not the field's furniture: a unit after a
    /// PIN still reads as itself.
    #[test]
    fn a_mask_never_reaches_the_adornments() {
        let mut driver = driver();
        let state = state("1234");
        render_with(&mut driver, &state, || input().mask_char('*').suffix("#"));
        assert_eq!(driver.row(0), "****   #    ");
    }

    fn adorned() -> Input<State, Msg> {
        input().prefix("$").suffix("kg")
    }

    /// The adornments are part of the field: a press on one focuses it and
    /// places the cursor at that end of the text.
    #[test]
    fn a_click_on_an_adornment_places_the_cursor_at_that_end() {
        let mut driver = driver();
        let mut state = State {
            focus: FocusState::none(),
            ..state("ab")
        };
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(ChildId::Static("name"), adorned(), Rect::new(0, 0, 12, 1));
            });
        };
        render(&mut driver, &state);
        assert_eq!(driver.row(0), "$ ab      kg");

        let EventResult::Emit(Msg::Focus(focus)) = driver.event(mouse(LEFT_DOWN, 0, 0), &state)
        else {
            panic!("a press on the prefix must focus the field");
        };
        state.focus = focus;
        render(&mut driver, &state);
        send(&mut driver, &mut state, mouse(LEFT_UP, 0, 0));
        assert_eq!(state.name.cursor(), 0, "the prefix");

        render(&mut driver, &state);
        click(&mut driver, &mut state, 11, 0);
        assert_eq!(state.name.cursor(), 2, "the suffix");
        render(&mut driver, &state);
        click(&mut driver, &mut state, 3, 0);
        assert_eq!(state.name.cursor(), 1, "the text starts after the prefix");
    }

    /// A click on an adornment means that end of the text even when it is
    /// scrolled out of sight, where a drag across the same cell only steps
    /// one character past the edge, so it can keep scrolling.
    #[test]
    fn a_click_on_an_adornment_reaches_that_end_of_scrolled_text() {
        let mut driver = driver();
        let mut state = state("abcdefghijklmnop");
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(ChildId::Static("name"), adorned(), Rect::new(0, 0, 12, 1));
            });
        };
        render(&mut driver, &state);
        assert_eq!(driver.row(0), "$ klmnop  kg", "scrolled to the end");

        click(&mut driver, &mut state, 0, 0);
        assert_eq!(state.name.cursor(), 0, "the prefix: the very start");
        render(&mut driver, &state);
        assert_eq!(driver.row(0), "$ abcdefg kg");

        click(&mut driver, &mut state, 11, 0);
        assert_eq!(state.name.cursor(), 16, "the suffix: the very end");
        render(&mut driver, &state);
        assert_eq!(driver.row(0), "$ klmnop  kg");

        route(&mut driver, &mut state, mouse(LEFT_DOWN, 4, 0));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 0, 0));
        assert_eq!(state.name.cursor(), 9, "a drag steps one past the edge");
    }

    /// A press is a press wherever it ends up: one on an adornment anchors
    /// a drag at that end of the text, as a click there would place it.
    #[test]
    fn a_drag_from_an_adornment_selects_from_that_end() {
        let mut driver = driver();
        let mut state = state("abcdefghijklmnop");
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(ChildId::Static("name"), adorned(), Rect::new(0, 0, 12, 1));
            });
        };
        render(&mut driver, &state);

        route(&mut driver, &mut state, mouse(LEFT_DOWN, 0, 0));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 3, 0));
        assert_eq!(selection(&state), Some((0, 11)), "from the very start");

        render(&mut driver, &state);
        route(&mut driver, &mut state, mouse(LEFT_UP, 3, 0));
        send(&mut driver, &mut state, key(KeyCode::End));
        render(&mut driver, &state);
        route(&mut driver, &mut state, mouse(LEFT_DOWN, 11, 0));
        send(&mut driver, &mut state, mouse(LEFT_DRAG, 4, 0));
        assert_eq!(selection(&state), Some((12, 16)), "from the very end");
    }

    /// Long text scrolls inside the cells between the adornments, and never
    /// paints over either.
    #[test]
    fn long_text_scrolls_between_the_adornments() {
        let mut driver = driver();
        let mut state = state("abcdefghij");
        let render = |driver: &mut Driver<State, Msg>, state: &State| {
            driver.render(state, |ctx| {
                ctx.component(ChildId::Static("name"), adorned(), Rect::new(0, 0, 12, 1));
            });
        };
        render(&mut driver, &state);
        assert_eq!(
            driver.row(0),
            "$ efghij  kg",
            "seven text cells, one the cursor's"
        );

        send(&mut driver, &mut state, key(KeyCode::Home));
        render(&mut driver, &state);
        assert_eq!(driver.row(0), "$ abcdefg kg");
    }

    /// Adornments that leave no cell for the text leave a field that edits
    /// text nobody sees: it takes no focus, typing, or clicks.
    #[test]
    fn a_field_the_adornments_fill_takes_no_focus() {
        let mut driver = driver();
        let state = state("");
        driver.render(&state, |ctx| {
            ctx.component(ChildId::Static("name"), adorned(), Rect::new(0, 0, 5, 1));
        });
        assert!(
            driver
                .ratcn
                .focus_path(&[ChildId::Static("name")])
                .is_none()
        );
        for event in [key(KeyCode::Char('x')), mouse(LEFT_DOWN, 2, 0)] {
            assert!(
                matches!(driver.event(event.clone(), &state), EventResult::Ignored),
                "{event:?}"
            );
        }
    }
}
