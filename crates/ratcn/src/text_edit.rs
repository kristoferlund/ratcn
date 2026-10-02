//! The text your app stores for an [`Input`](crate::Input) or a
//! [`TextArea`](crate::TextArea), and the pieces a text component is built
//! from.
//!
//! Text fields are controlled like everything else: the content lives in app
//! state as an [`InputState`] or a [`TextAreaState`]. A keystroke takes the
//! current state, applies the edit, and emits a *new* state for the app to
//! store.
//!
//! Editing, cursor movement, selection, and scrolling belong to the
//! [`ratatui_textarea`] editor each state wraps. This module re-exports the
//! editor types a component needs, so a component copied into your project
//! depends on `ratcn` and `ratatui` alone, and holds the mechanics both fields
//! share: the key conversion, the editor's binding table, and the mapping from
//! a pointer to a place in the text.
//!
//! Undo history is switched off: a state is replaced on every keystroke, which
//! is the wrong place to keep a stack of past states. An app that wants undo
//! keeps its own history of states, at the cost of a whole editor, text
//! included, for each one.

use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

pub use ratatui_textarea::{
    CursorMove, DataCursor, Input as EditorInput, Key as EditorKey, TextArea as Editor, WrapMode,
};

use ratatui::layout::{Position, Rect};

use crate::runtime::{KeyCode, KeyEvent, Modifiers};

/// A version no other state holds: drawn on every construction and every edit.
fn next_version() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The text, cursor, and selection of a single-line field, stored in app
/// state.
///
/// What [`Input`](crate::Input) reads and emits. It stays one line: a line
/// break becomes a space at construction, and the component never inserts one.
///
/// There is no `PartialEq`, because the editor has none. Two states with the
/// same [`version`](Self::version) are the same state.
#[derive(Clone)]
pub struct InputState {
    // Boxed so the state stays small: it travels inside the app's message.
    editor: Box<Editor<'static>>,
    version: u64,
}

impl Default for InputState {
    fn default() -> Self {
        Self::new("")
    }
}

impl InputState {
    /// A state holding `value`, with the cursor at its end. Each line break in
    /// `value` — `\r\n`, `\n`, or `\r` — becomes a space.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into().replace("\r\n", " ").replace(['\n', '\r'], " ");
        let mut editor = Editor::new(vec![value]);
        editor.move_cursor(CursorMove::End);
        Self::from_editor(editor)
    }

    /// The state an edit produced: `editor` adopted as it is, under a fresh
    /// version.
    ///
    /// This is how a component hands back the editor it changed — the one it
    /// painted, or a clone of [`editor`](Self::editor), with a key or a paste
    /// applied. Nothing is rebuilt from the text, so the editor's selection
    /// and scroll position carry over. Undo history is switched off.
    ///
    /// # Panics
    ///
    /// Panics unless `editor` holds exactly one line: a multi-line editor
    /// here is a programming error, not data.
    #[must_use]
    pub fn from_editor(mut editor: Editor<'static>) -> Self {
        assert!(
            editor.lines().len() == 1,
            "an InputState holds one line; the editor holds {}",
            editor.lines().len()
        );
        editor.set_max_histories(0);
        Self {
            editor: Box::new(editor),
            version: next_version(),
        }
    }

    /// The current text.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.editor.lines()[0]
    }

    /// The cursor position, as a character index into [`value`](Self::value).
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.editor.cursor().1
    }

    /// The editor behind this state. A component clones it to paint or edit.
    #[must_use]
    pub fn editor(&self) -> &Editor<'static> {
        &self.editor
    }

    /// What identifies this state: unique across the process, and new after
    /// every edit. A component compares it to tell whether the state it
    /// painted is still the one the app holds.
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }
}

/// The text, cursor, and selection of a multi-line field, stored in app state.
///
/// The multi-line counterpart of [`InputState`], with the same contract: no
/// `PartialEq`, and a [`version`](Self::version) that identifies the state.
#[derive(Clone)]
pub struct TextAreaState {
    // Boxed so the state stays small: it travels inside the app's message.
    editor: Box<Editor<'static>>,
    version: u64,
}

impl Default for TextAreaState {
    fn default() -> Self {
        Self::new("")
    }
}

impl TextAreaState {
    /// A state holding `value`, split into lines at each `\n` or `\r\n`, with
    /// the cursor at the end of the text.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let mut editor = Editor::from(
            value
                .split('\n')
                .map(|line| line.strip_suffix('\r').unwrap_or(line)),
        );
        editor.move_cursor(CursorMove::Bottom);
        editor.move_cursor(CursorMove::End);
        Self::from_editor(editor)
    }

    /// The state an edit produced: `editor` adopted as it is, under a fresh
    /// version. See [`InputState::from_editor`].
    #[must_use]
    pub fn from_editor(mut editor: Editor<'static>) -> Self {
        editor.set_max_histories(0);
        Self {
            editor: Box::new(editor),
            version: next_version(),
        }
    }

    /// The current text, its lines joined with `\n`.
    #[must_use]
    pub fn value(&self) -> String {
        self.editor.lines().join("\n")
    }

    /// The current lines.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        self.editor.lines()
    }

    /// The cursor position: the line, then the character index within it.
    #[must_use]
    pub fn cursor(&self) -> (usize, usize) {
        let DataCursor(row, column) = self.editor.cursor();
        (row, column)
    }

    /// The editor behind this state. A component clones it to paint or edit.
    #[must_use]
    pub fn editor(&self) -> &Editor<'static> {
        &self.editor
    }

    /// What identifies this state. See [`InputState::version`].
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }
}

impl fmt::Debug for InputState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputState")
            .field("value", &self.value())
            .field("cursor", &self.cursor())
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for TextAreaState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextAreaState")
            .field("value", &self.value())
            .field("cursor", &self.cursor())
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

/// Convert a key press into the editor's own input type.
///
/// The conversion is mechanical and carries no policy: which keys a field
/// takes, and which it leaves to bubble, is the component's decision. Two
/// adjustments make the editor see what the user meant:
///
/// - **`AltGr`.** Some terminals report `AltGr` as Ctrl+Alt, so `@`, `{`, or
///   `€` arrive looking like a chord. Ctrl+Alt with anything but an ASCII
///   letter or digit converts to the plain character. The trade-off: a real
///   Ctrl+Alt chord on punctuation, Ctrl+Alt+`[` say, is typed rather than
///   delivered as a shortcut.
/// - **Chord letters.** The editor matches chords by lowercase letter, while
///   a backend reports Ctrl+Shift+A as `A`. A letter with Ctrl or Alt held
///   is lowercased; Shift stays set.
///
/// `None` only for [`KeyCode::BackTab`], which the editor has no key for.
#[must_use]
pub fn editor_input(event: &KeyEvent) -> Option<EditorInput> {
    let Modifiers {
        mut ctrl,
        mut alt,
        shift,
    } = event.modifiers;
    let key = match event.code {
        KeyCode::Char(char) if ctrl && alt && !char.is_ascii_alphanumeric() => {
            ctrl = false;
            alt = false;
            EditorKey::Char(char)
        }
        KeyCode::Char(char) if ctrl || alt => EditorKey::Char(char.to_ascii_lowercase()),
        KeyCode::Char(char) => EditorKey::Char(char),
        KeyCode::Enter => EditorKey::Enter,
        KeyCode::Esc => EditorKey::Esc,
        KeyCode::Tab => EditorKey::Tab,
        KeyCode::BackTab => return None,
        KeyCode::Backspace => EditorKey::Backspace,
        KeyCode::Delete => EditorKey::Delete,
        KeyCode::Left => EditorKey::Left,
        KeyCode::Right => EditorKey::Right,
        KeyCode::Up => EditorKey::Up,
        KeyCode::Down => EditorKey::Down,
        KeyCode::Home => EditorKey::Home,
        KeyCode::End => EditorKey::End,
        KeyCode::PageUp => EditorKey::PageUp,
        KeyCode::PageDown => EditorKey::PageDown,
        KeyCode::F(number) => EditorKey::F(number),
    };
    Some(EditorInput {
        key,
        ctrl,
        alt,
        shift,
    })
}

/// Whether the editor binds `input` to something: an edit, a movement, a
/// selection, the yank buffer, or a page of scrolling.
///
/// This mirrors the keymap in upstream's [`Editor::input`], and a test holds
/// the two together. A component routes keys by it: a binding is consumed
/// even where it changes nothing — Ctrl+K at the end of the text — so that
/// whether a chord reaches the app never depends on where the cursor is. Shift
/// never decides a binding; the editor reads it as "select".
///
/// Undo and redo (Ctrl+U, Ctrl+R) are left out: a state keeps no history, so
/// in a state they do nothing.
#[must_use]
pub fn is_editor_binding(input: &EditorInput) -> bool {
    let EditorInput { key, ctrl, alt, .. } = *input;
    match key {
        EditorKey::Char(char) => match (ctrl, alt) {
            // Typing.
            (false, false) => true,
            (true, false) => matches!(
                char,
                'a' | 'b'
                    | 'c'
                    | 'd'
                    | 'e'
                    | 'f'
                    | 'h'
                    | 'j'
                    | 'k'
                    | 'm'
                    | 'n'
                    | 'p'
                    | 'v'
                    | 'w'
                    | 'x'
                    | 'y'
            ),
            (false, true) => matches!(
                char,
                'b' | 'd' | 'f' | 'h' | 'n' | 'p' | 'v' | '<' | '>' | '[' | ']'
            ),
            (true, true) => matches!(char, 'b' | 'f' | 'n' | 'p'),
        },
        EditorKey::Enter
        | EditorKey::Home
        | EditorKey::End
        | EditorKey::PageUp
        | EditorKey::PageDown
        | EditorKey::Copy
        | EditorKey::Cut
        | EditorKey::Paste
        | EditorKey::MouseScrollUp
        | EditorKey::MouseScrollDown => true,
        EditorKey::Tab => !ctrl && !alt,
        EditorKey::Backspace | EditorKey::Delete => !ctrl,
        // Alone, by character; with Ctrl, by word or paragraph; with
        // Ctrl+Alt, to the start or end of the line or the text.
        EditorKey::Left | EditorKey::Right | EditorKey::Up | EditorKey::Down => ctrl || !alt,
        _ => false,
    }
}

/// The move that puts the cursor on the character drawn under `pointer`.
///
/// `text` is where `editor` was painted, and the editor knows how far that
/// view is scrolled. A pointer outside `text` counts as one cell past the edge
/// it left by: that cell's character is the next one out of sight, so the
/// cursor steps onto it and the next paint scrolls it into view — a drag held
/// past an edge keeps extending as the pointer moves. Past the end of a line
/// the editor clamps to its end, and below the text to the last line.
#[must_use]
pub fn cursor_at(editor: &Editor<'_>, text: Rect, pointer: Position) -> CursorMove {
    let (top_row, top_column) = editor.scroll_offset();
    let along = |pointer: u16, start: u16, length: u16, scrolled: u16| {
        let offset = (i32::from(pointer) - i32::from(start))
            .min(i32::from(length))
            .max(-1);
        usize::try_from(i32::from(scrolled) + offset).unwrap_or(0)
    };
    let DataCursor(row, column) = editor.screen_to_data(
        along(pointer.y, text.y, text.height, top_row),
        along(pointer.x, text.x, text.width, top_column),
    );
    CursorMove::Jump(
        u16::try_from(row).unwrap_or(u16::MAX),
        u16::try_from(column).unwrap_or(u16::MAX),
    )
}

/// Select from `anchor` to `pointer`: the selection a drag makes, replacing
/// whatever was selected before. A drag that comes back to where it began
/// selects nothing — an empty selection is still one to the editor, and the
/// next arrow key would extend it.
pub fn select_dragged(editor: &mut Editor<'_>, anchor: CursorMove, pointer: CursorMove) {
    editor.cancel_selection();
    editor.move_cursor(anchor);
    let anchored = editor.cursor();
    editor.start_selection();
    editor.move_cursor(pointer);
    if editor.cursor() == anchored {
        editor.cancel_selection();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    const ALT: Modifiers = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    const CTRL_ALT: Modifiers = Modifiers {
        ctrl: true,
        alt: true,
        shift: false,
    };

    fn convert(code: KeyCode, modifiers: Modifiers) -> EditorInput {
        editor_input(&KeyEvent { code, modifiers }).expect("the editor has this key")
    }

    /// What the editor does with a converted key, which is the point of
    /// converting it: `(text, cursor column, selection)` afterwards.
    fn apply(text: &str, code: KeyCode, modifiers: Modifiers) -> (String, usize, bool) {
        let mut editor = InputState::new(text).editor().clone();
        editor.input(convert(code, modifiers));
        (
            editor.lines()[0].clone(),
            editor.cursor().1,
            editor.is_selecting(),
        )
    }

    #[test]
    fn states_start_with_the_cursor_at_the_end() {
        let input = InputState::new("hello");
        assert_eq!((input.value(), input.cursor()), ("hello", 5));

        let area = TextAreaState::new("one\ntwo");
        assert_eq!(area.lines(), ["one", "two"]);
        assert_eq!(area.value(), "one\ntwo");
        assert_eq!(area.cursor(), (1, 3));

        assert_eq!(InputState::default().value(), "");
        assert_eq!(TextAreaState::default().lines(), [""]);
        assert_eq!(
            TextAreaState::new("one\r\ntwo\n").lines(),
            ["one", "two", ""],
            "a Windows line ending must not leave a carriage return in the line"
        );
    }

    /// The version is how a component tells "the state I painted" from "a
    /// state the app replaced it with", so no two distinct states may share
    /// one — not even two built from the same text — while a clone, which is
    /// the same state, must keep it.
    #[test]
    fn every_construction_and_edit_takes_a_fresh_version() {
        let first = InputState::new("same");
        let second = InputState::new("same");
        assert_ne!(first.version(), second.version());
        assert_eq!(first.clone().version(), first.version());

        let edited = InputState::from_editor(first.editor().clone());
        assert_ne!(edited.version(), first.version());

        let area = TextAreaState::new("same");
        assert_ne!(area.version(), TextAreaState::new("same").version());
        assert_ne!(
            TextAreaState::from_editor(area.editor().clone()).version(),
            area.version()
        );
        assert_ne!(InputState::default().version(), area.version());
    }

    /// A state is replaced on every keystroke, so an undo stack inside it
    /// would be copied with each one. `from_editor` is the only way in, and it
    /// switches history off whatever the editor arrived with.
    #[test]
    fn history_is_off_in_every_state() {
        assert_eq!(InputState::new("a").editor().max_histories(), 0);
        assert_eq!(TextAreaState::new("a").editor().max_histories(), 0);
        assert_eq!(
            InputState::from_editor(Editor::default())
                .editor()
                .max_histories(),
            0
        );
        assert_eq!(
            TextAreaState::from_editor(Editor::default())
                .editor()
                .max_histories(),
            0
        );
    }

    /// A value often comes from data the app does not control — a record,
    /// a file, a paste through some other route — and a line break in it is
    /// no reason to crash. Each one becomes a space, as a paste's would.
    #[test]
    fn an_input_state_flattens_line_breaks_to_spaces() {
        let state = InputState::new("one\r\ntwo\nthree\rfour");
        assert_eq!(state.value(), "one two three four");
        assert_eq!(state.cursor(), 18);
    }

    /// A state's debug output is for reading: the text, the cursor, and the
    /// version, not the editor's whole configuration.
    #[test]
    fn debug_shows_the_value_cursor_and_version() {
        let input = InputState::new("ab");
        assert_eq!(
            format!("{input:?}"),
            format!(
                "InputState {{ value: \"ab\", cursor: 2, version: {}, .. }}",
                input.version()
            )
        );
        let area = TextAreaState::new("a\nb");
        assert_eq!(
            format!("{area:?}"),
            format!(
                "TextAreaState {{ value: \"a\\nb\", cursor: (1, 1), version: {}, .. }}",
                area.version()
            )
        );
    }

    #[test]
    #[should_panic(expected = "an InputState holds one line")]
    fn an_input_state_rejects_a_multi_line_editor() {
        let _ = InputState::from_editor(Editor::from(["one", "two"]));
    }

    #[test]
    fn every_key_maps_to_the_editors_key_of_the_same_name() {
        for (code, key) in [
            (KeyCode::Char('a'), EditorKey::Char('a')),
            (KeyCode::Char('A'), EditorKey::Char('A')),
            (KeyCode::Char('é'), EditorKey::Char('é')),
            (KeyCode::Enter, EditorKey::Enter),
            (KeyCode::Esc, EditorKey::Esc),
            (KeyCode::Tab, EditorKey::Tab),
            (KeyCode::Backspace, EditorKey::Backspace),
            (KeyCode::Delete, EditorKey::Delete),
            (KeyCode::Left, EditorKey::Left),
            (KeyCode::Right, EditorKey::Right),
            (KeyCode::Up, EditorKey::Up),
            (KeyCode::Down, EditorKey::Down),
            (KeyCode::Home, EditorKey::Home),
            (KeyCode::End, EditorKey::End),
            (KeyCode::PageUp, EditorKey::PageUp),
            (KeyCode::PageDown, EditorKey::PageDown),
            (KeyCode::F(5), EditorKey::F(5)),
        ] {
            assert_eq!(
                convert(code, Modifiers::NONE),
                EditorInput {
                    key,
                    ctrl: false,
                    alt: false,
                    shift: false,
                },
                "{code:?}"
            );
        }
    }

    #[test]
    fn back_tab_has_no_editor_key() {
        assert_eq!(editor_input(&KeyEvent::new(KeyCode::BackTab)), None);
    }

    #[test]
    fn modifiers_are_carried_through() {
        for modifiers in [CTRL, ALT, SHIFT, CTRL_ALT] {
            let input = convert(KeyCode::Left, modifiers);
            assert_eq!(
                (input.key, input.ctrl, input.alt, input.shift),
                (
                    EditorKey::Left,
                    modifiers.ctrl,
                    modifiers.alt,
                    modifiers.shift
                )
            );
        }
    }

    /// A shifted letter is text: the backend already delivers the capital,
    /// and it must be inserted as such, Shift flag or not.
    #[test]
    fn shift_types_the_shifted_character() {
        assert_eq!(
            apply("ab", KeyCode::Char('C'), SHIFT),
            ("abC".to_owned(), 3, false)
        );
        assert_eq!(
            apply("ab", KeyCode::Char('!'), SHIFT),
            ("ab!".to_owned(), 3, false)
        );
    }

    /// Shift with a movement key is how the editor selects, so the flag must
    /// arrive on the converted key.
    #[test]
    fn shift_with_a_movement_key_selects() {
        assert_eq!(
            apply("abc", KeyCode::Left, SHIFT),
            ("abc".to_owned(), 2, true)
        );
        assert_eq!(
            apply("abc", KeyCode::Left, Modifiers::NONE),
            ("abc".to_owned(), 2, false)
        );
        assert_eq!(
            apply("abc", KeyCode::Home, SHIFT),
            ("abc".to_owned(), 0, true)
        );
    }

    /// The editor's readline chords must still be chords after conversion,
    /// not inserted letters.
    #[test]
    fn ctrl_chords_reach_the_editor_as_chords() {
        let ctrl = |char| apply("one two", KeyCode::Char(char), CTRL);
        assert_eq!(ctrl('a'), ("one two".to_owned(), 0, false), "head");
        assert_eq!(ctrl('b'), ("one two".to_owned(), 6, false), "back");
        assert_eq!(ctrl('h'), ("one tw".to_owned(), 6, false), "backspace");
        assert_eq!(ctrl('w'), ("one ".to_owned(), 4, false), "delete word");
        assert_eq!(ctrl('j'), (String::new(), 0, false), "delete to head");
        assert_eq!(
            apply("one two", KeyCode::Left, CTRL),
            ("one two".to_owned(), 4, false),
            "word back"
        );
    }

    #[test]
    fn alt_chords_reach_the_editor_as_chords() {
        assert_eq!(
            apply("one two", KeyCode::Char('b'), ALT),
            ("one two".to_owned(), 4, false),
            "word back"
        );
        assert_eq!(
            apply("one two", KeyCode::Backspace, ALT),
            ("one ".to_owned(), 4, false),
            "delete word"
        );
    }

    /// A backend reports Ctrl+Shift+A as `A`, and the editor matches chords
    /// by lowercase letter: without the lowercasing the chord would do
    /// nothing, and selecting to the head of the line would be unreachable.
    #[test]
    fn a_shifted_chord_letter_is_lowercased_and_keeps_shift() {
        let input = convert(
            KeyCode::Char('A'),
            Modifiers {
                ctrl: true,
                alt: false,
                shift: true,
            },
        );
        assert_eq!(
            input,
            EditorInput {
                key: EditorKey::Char('a'),
                ctrl: true,
                alt: false,
                shift: true,
            }
        );
        assert_eq!(
            apply(
                "abc",
                KeyCode::Char('A'),
                Modifiers {
                    ctrl: true,
                    alt: false,
                    shift: true,
                }
            ),
            ("abc".to_owned(), 0, true)
        );
        assert_eq!(convert(KeyCode::Char('B'), ALT).key, EditorKey::Char('b'));
    }

    /// `AltGr` characters arrive as Ctrl+Alt on some terminals. They are text
    /// the user typed, and must be inserted rather than dropped as an unknown
    /// chord.
    #[test]
    fn altgr_characters_are_typed_as_plain_text() {
        for char in ['@', '{', '}', '[', ']', '\\', '|', '~', '€', 'ł', 'µ'] {
            assert_eq!(
                convert(KeyCode::Char(char), CTRL_ALT),
                EditorInput {
                    key: EditorKey::Char(char),
                    ctrl: false,
                    alt: false,
                    shift: false,
                },
                "{char:?}"
            );
            assert_eq!(
                apply("a", KeyCode::Char(char), CTRL_ALT),
                (format!("a{char}"), 2, false),
                "{char:?}"
            );
        }
    }

    /// An ASCII letter or digit with Ctrl+Alt is a real chord — the editor
    /// binds Ctrl+Alt+B/F/N/P, and the rest are the app's shortcuts — so it
    /// must never be typed.
    #[test]
    fn ctrl_alt_letters_and_digits_stay_chords() {
        for char in ['b', 'f', 's', 'S', '1'] {
            let input = convert(KeyCode::Char(char), CTRL_ALT);
            assert_eq!(
                (input.key, input.ctrl, input.alt),
                (EditorKey::Char(char.to_ascii_lowercase()), true, true),
                "{char:?}"
            );
        }
        assert_eq!(
            apply("abc", KeyCode::Char('b'), CTRL_ALT),
            ("abc".to_owned(), 0, false),
            "the editor's Ctrl+Alt+B moves to the head"
        );
        assert_eq!(
            apply("abc", KeyCode::Char('s'), CTRL_ALT),
            ("abc".to_owned(), 3, false),
            "an unbound chord changes nothing"
        );
    }

    /// The binding table has to be the editor's keymap, or a key would be
    /// consumed while doing nothing, or bubble to the app after the editor
    /// acted on it. Every key, under every modifier, is tried on editors
    /// where each binding has something to do: a key is a binding exactly
    /// when it changes the text, the cursor, the selection, the yank buffer,
    /// or the view in one of them. Undo and redo change nothing, because a
    /// state keeps no history, so they are not bindings.
    #[test]
    fn the_binding_table_is_the_editors_keymap() {
        let text = (0..30)
            .map(|line| if line % 10 == 9 { "" } else { "one two three" })
            .collect::<Vec<_>>()
            .join("\n");
        let mut plain = TextAreaState::new(text).editor().clone();
        plain.move_cursor(CursorMove::Jump(15, 5));
        plain.set_yank_text("yanked");
        let area = ratatui::layout::Rect::new(0, 0, 20, 5);
        ratatui::widgets::Widget::render(&plain, area, &mut ratatui::buffer::Buffer::empty(area));
        let mut selecting = plain.clone();
        selecting.start_selection();
        selecting.move_cursor(CursorMove::Forward);
        let observe = |editor: &Editor<'_>| {
            (
                editor.lines().to_vec(),
                editor.cursor(),
                editor.selection_range(),
                editor.yank_text(),
                editor.scroll_offset(),
            )
        };

        let mut keys = vec![
            EditorKey::Enter,
            EditorKey::Esc,
            EditorKey::Tab,
            EditorKey::Backspace,
            EditorKey::Delete,
            EditorKey::Left,
            EditorKey::Right,
            EditorKey::Up,
            EditorKey::Down,
            EditorKey::Home,
            EditorKey::End,
            EditorKey::PageUp,
            EditorKey::PageDown,
            EditorKey::F(1),
            EditorKey::Copy,
            EditorKey::Cut,
            EditorKey::Paste,
            EditorKey::MouseScrollUp,
            EditorKey::MouseScrollDown,
            EditorKey::Null,
        ];
        keys.extend(
            ('a'..='z')
                .chain(['A', '1', ' ', '<', '>', '[', ']', '\n', '\r'])
                .map(EditorKey::Char),
        );
        for key in keys {
            for bits in 0..8 {
                let input = EditorInput {
                    key,
                    ctrl: bits & 1 != 0,
                    alt: bits & 2 != 0,
                    shift: bits & 4 != 0,
                };
                let acts = [&plain, &selecting].into_iter().any(|editor| {
                    let mut edited = editor.clone();
                    edited.input(input.clone());
                    observe(&edited) != observe(editor)
                });
                assert_eq!(is_editor_binding(&input), acts, "{input:?}");
            }
        }
    }

    /// Alt alone is not `AltGr`: Alt+`<` is the editor's jump-to-top chord and
    /// must stay one.
    #[test]
    fn a_single_modifier_never_becomes_plain_text() {
        for modifiers in [CTRL, ALT] {
            let input = convert(KeyCode::Char('<'), modifiers);
            assert_eq!(
                (input.key, input.ctrl, input.alt),
                (EditorKey::Char('<'), modifiers.ctrl, modifiers.alt)
            );
        }
    }
}
