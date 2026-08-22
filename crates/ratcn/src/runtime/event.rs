//! Normalized input events.
//!
//! The interaction runtime defines its own event vocabulary so components match
//! one set of types across backends. Conversions from crossterm, termina, and
//! ratzilla are feature-gated (`crossterm`, `termina`, `ratzilla`).

/// An input event, in the runtime's own vocabulary rather than a backend's.
///
/// Backend events convert into this with `TryFrom`, which is why
/// [`Ratcn::handle_event`](super::Ratcn::handle_event) takes anything
/// convertible: input a backend reports but this vocabulary has no place for
/// (a key release, a resize) fails the conversion and is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A key was pressed. Key *releases* are not represented.
    Key(KeyEvent),
    /// Text arrived in one piece from the terminal or browser clipboard, rather
    /// than as individual key presses.
    Paste(String),
    /// The pointer moved, a button changed, or the wheel turned.
    Mouse(MouseEvent),
}

impl From<KeyEvent> for Event {
    fn from(key: KeyEvent) -> Self {
        Self::Key(key)
    }
}

impl From<MouseEvent> for Event {
    fn from(mouse: MouseEvent) -> Self {
        Self::Mouse(mouse)
    }
}

impl From<KeyCode> for Event {
    fn from(code: KeyCode) -> Self {
        Self::Key(KeyEvent::new(code))
    }
}

/// A key press, with whichever modifiers were held at the time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    /// Which key was pressed.
    pub code: KeyCode,
    /// Ctrl/Alt/Shift state during the press.
    pub modifiers: Modifiers,
}

impl KeyEvent {
    /// A press of `code` with no modifiers held.
    #[must_use]
    pub const fn new(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: Modifiers::NONE,
        }
    }
}

impl From<KeyCode> for KeyEvent {
    fn from(code: KeyCode) -> Self {
        Self::new(code)
    }
}

/// Which key a [`KeyEvent`] refers to.
///
/// It covers the keys terminal UIs bind. A backend key with no variant here
/// does not convert into an [`Event`] and is ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyCode {
    /// A printable character, as the backend reports it: a shifted `a` arrives
    /// as `Char('A')`, usually with `modifiers.shift` set as well. Match on the
    /// character rather than on Shift.
    Char(char),
    /// Return. The commit key for buttons, list rows, and dialog actions.
    Enter,
    /// Escape. The dismiss key for modal layers.
    Esc,
    /// Tab. Moves focus to the next focusable component.
    Tab,
    /// Shift+Tab, normalized. Runtime traversal accepts Shift but ignores this
    /// code when Ctrl or Alt is also held.
    BackTab,
    /// Delete the character before the cursor.
    Backspace,
    /// Delete the character after the cursor.
    Delete,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Home. Jumps to the first item of a control with items.
    Home,
    /// End. Jumps to the last item of a control with items.
    End,
    /// Page Up. Moves a viewport's worth of items toward the start.
    PageUp,
    /// Page Down. Moves a viewport's worth of items toward the end.
    PageDown,
    /// A function key, numbered from 1: `F(1)` is F1. How many exist depends on
    /// the terminal.
    F(u8),
}

/// Which modifier keys were held during an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Control was held.
    pub ctrl: bool,
    /// Alt (Option on macOS) was held.
    pub alt: bool,
    /// Shift was held. For character keys this is usually redundant — the
    /// [`KeyCode::Char`] already carries the shifted character.
    pub shift: bool,
}

impl Modifiers {
    /// No modifiers held. Same as `Modifiers::default()`, but usable in `const`
    /// context.
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
    };

    /// True if any modifier at all was held.
    #[must_use]
    pub const fn any(self) -> bool {
        self.ctrl || self.alt || self.shift
    }
}

/// A key chord: a [`KeyCode`] plus the Ctrl/Alt that must accompany it, for
/// declaration-time bindings such as [`Ratcn::focus_key`](super::Ratcn::focus_key)
/// and [`ScopeOptions::focus_key`](super::ScopeOptions::focus_key).
///
/// Matching is lenient for ASCII letters: a [`Char`](KeyCode::Char) chord
/// ignores ASCII case and Shift, so `'a'` matches both `a` and `A`. Non-ASCII
/// characters and non-character keys match exactly. Ctrl and Alt must also
/// match exactly. Build one from a `char` or [`KeyCode`] and add modifiers:
///
/// ```
/// use ratcn::runtime::{KeyChord, KeyCode};
///
/// let _letter = KeyChord::from('a'); // the letter a, no Ctrl/Alt
/// let _shortcut = KeyChord::from('1').alt(); // Alt+1
/// let _submit = KeyChord::from(KeyCode::Enter).ctrl();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyChord {
    code: KeyCode,
    ctrl: bool,
    alt: bool,
}

impl KeyChord {
    /// Require Ctrl to be held.
    #[must_use]
    pub const fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    /// Require Alt to be held.
    #[must_use]
    pub const fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// Whether a key press satisfies this chord.
    ///
    /// Ctrl and Alt must match exactly. Shift is ignored, and ASCII
    /// [`Char`](KeyCode::Char) values match without regard to case, so a chord
    /// built from `'d'` matches both `d` and `D`; non-ASCII characters and
    /// non-character codes match exactly. This is the same matching the
    /// runtime applies to [`focus_key`](super::Ratcn::focus_key) bindings, so
    /// an app can reuse it for its own global hotkeys — checking an event
    /// against a chord before handing it to
    /// [`Ratcn::handle_event`](super::Ratcn::handle_event).
    #[must_use]
    pub fn matches(self, key: &KeyEvent) -> bool {
        self.ctrl == key.modifiers.ctrl
            && self.alt == key.modifiers.alt
            && match (self.code, key.code) {
                (KeyCode::Char(a), KeyCode::Char(b)) => a.eq_ignore_ascii_case(&b),
                (a, b) => a == b,
            }
    }
}

impl From<KeyCode> for KeyChord {
    fn from(code: KeyCode) -> Self {
        Self {
            code,
            ctrl: false,
            alt: false,
        }
    }
}

impl From<char> for KeyChord {
    fn from(c: char) -> Self {
        Self::from(KeyCode::Char(c))
    }
}

#[cfg(test)]
mod key_chord_tests {
    use super::{KeyChord, KeyCode, KeyEvent, Modifiers};

    #[test]
    fn character_chords_fold_ascii_case_and_ignore_shift() {
        let chord = KeyChord::from('a');

        assert!(chord.matches(&KeyEvent {
            code: KeyCode::Char('A'),
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        }));
    }

    #[test]
    fn character_chords_do_not_fold_non_ascii_case() {
        let chord = KeyChord::from('\u{e5}');

        assert!(chord.matches(&KeyEvent::new(KeyCode::Char('\u{e5}'))));
        assert!(!chord.matches(&KeyEvent::new(KeyCode::Char('\u{c5}'))));
    }

    #[test]
    fn character_chords_match_ctrl_and_alt_exactly() {
        let chord = KeyChord::from('k').ctrl().alt();

        for modifiers in [
            Modifiers::NONE,
            Modifiers {
                ctrl: true,
                ..Modifiers::NONE
            },
            Modifiers {
                alt: true,
                ..Modifiers::NONE
            },
        ] {
            assert!(!chord.matches(&KeyEvent {
                code: KeyCode::Char('k'),
                modifiers,
            }));
        }
        assert!(chord.matches(&KeyEvent {
            code: KeyCode::Char('K'),
            modifiers: Modifiers {
                ctrl: true,
                alt: true,
                shift: true,
            },
        }));
    }
}

/// A mouse event, normalized across backends.
///
/// `column`/`row` enter [`Ratcn`](super::Ratcn) as 0-based, screen-absolute
/// terminal cells. A component receives them in the coordinate space it was
/// declared with, which matches its
/// [`EventCtx::area`](super::EventCtx::area): the same cells at the top
/// level, and content coordinates inside a
/// [`viewport`](super::DeclareCtx::viewport). (crossterm already speaks cells;
/// the browser backend converts pixels to cells before constructing this.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    /// What the pointer did.
    pub kind: MouseKind,
    /// Cell column, 0-based, in the receiving component's declaration space.
    pub column: u16,
    /// Cell row, 0-based, in the receiving component's declaration space.
    pub row: u16,
    /// Ctrl/Alt/Shift state during the event.
    pub modifiers: Modifiers,
}

/// What a [`MouseEvent`] is.
///
/// [`Click`](MouseKind::Click) and [`DragEnd`](MouseKind::DragEnd) are
/// synthesized by [`Ratcn`](super::Ratcn)'s gesture tracking.
/// [`Exited`](MouseKind::Exited) is a browser backend notification; the other
/// kinds are backend input or normalized drag motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MouseKind {
    /// A button went down. This is where components take focus and where
    /// [`capture_pointer`](super::EventCtx::capture_pointer) must be called.
    Down(MouseButton),
    /// A button was released, wherever the pointer happens to be.
    Up(MouseButton),
    /// A press and release on the same component, synthesized after the `Up`.
    /// This is what "the button was clicked" means; `Down` alone is not a
    /// click.
    ///
    /// The pointer may move between the press and the release: what must match
    /// is the hit path, not the cell. Drifting a column while pressing a button
    /// still clicks it, the same way it does everywhere else. Leaving the
    /// component ends the click — returning to it before releasing revives it.
    Click(MouseButton),
    /// The pointer moved with a button held. Routed to the component that
    /// captured the gesture, or hit-tested if none did.
    Drag(MouseButton),
    /// The release that ends a drag (synthesized): follows the `Up` of a press
    /// that produced at least one [`Drag`](MouseKind::Drag) and did not end as
    /// a [`Click`](MouseKind::Click). The event's `column`/`row` are where the
    /// drag was released, so a drop target can hit-test them.
    ///
    /// Movement a component claimed with
    /// [`capture_pointer`](super::EventCtx::capture_pointer) always ends here,
    /// never as a click — claiming it is what declares the movement meaningful.
    /// Movement nobody claimed is pointer drift, and its release on the press
    /// target is a click. A claimed press that never moved is a click too, so
    /// one component can both drag and be clicked.
    DragEnd(MouseButton),
    /// The pointer moved with no button held. Drives hover.
    Moved,
    /// The pointer left the backend's interactive grid. The runtime cancels
    /// tracked pointer gestures and does not route this to components.
    Exited,
    /// The wheel or trackpad scrolled at the reported cell.
    Scroll(ScrollDirection),
}

/// The three mouse buttons the runtime tracks. Extra buttons a backend may
/// report (back, forward) do not convert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MouseButton {
    /// The primary button. Presses, clicks, and drags on it drive focus and
    /// activation.
    Left,
    /// The secondary button. The runtime routes it but binds nothing to it.
    Right,
    /// The wheel button. Routed, with nothing bound to it.
    Middle,
}

/// Which way the wheel or trackpad scrolled.
///
/// Exhaustive: these are the four axes a terminal or browser reports. A
/// row-oriented control that only cares about the vertical pair converts to
/// [`ScrollStep`](crate::linear_nav::ScrollStep).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    /// Scrolled toward the top of the content.
    Up,
    /// Scrolled toward the bottom of the content.
    Down,
    /// Scrolled toward the left of the content.
    Left,
    /// Scrolled toward the right of the content.
    Right,
}

/// Conversion error: this backend event has no equivalent in the normalized
/// vocabulary — an unidentified browser key, a key release, a resize.
///
/// It is the `TryFrom` error type for backend events, and
/// [`Ratcn::handle_event`](super::Ratcn::handle_event) treats it as
/// [`EventResult::Ignored`](super::EventResult::Ignored) rather than an error to
/// handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsupported;

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unsupported input event")
    }
}

impl std::error::Error for Unsupported {}

#[cfg(test)]
mod unsupported_tests {
    use super::Unsupported;

    #[test]
    fn unsupported_is_a_standard_error_with_a_message() {
        fn assert_error<T: std::error::Error>() {}

        assert_error::<Unsupported>();
        assert_eq!(Unsupported.to_string(), "unsupported input event");
    }
}

/// Why a typed browser paste event could not be normalized.
#[cfg(all(target_arch = "wasm32", feature = "ratzilla"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BrowserEventError {
    /// The paste event did not expose clipboard data.
    MissingClipboardData,
    /// Reading `text/plain` from the clipboard failed.
    ClipboardRead,
}

#[cfg(all(target_arch = "wasm32", feature = "ratzilla"))]
impl std::fmt::Display for BrowserEventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::MissingClipboardData => "browser paste has no clipboard data",
            Self::ClipboardRead => "could not read text/plain from browser clipboard",
        };
        f.write_str(message)
    }
}

#[cfg(all(target_arch = "wasm32", feature = "ratzilla"))]
impl std::error::Error for BrowserEventError {}

#[cfg(all(target_arch = "wasm32", feature = "ratzilla"))]
mod browser_conv {
    use super::{BrowserEventError, Event};
    use web_sys::ClipboardEvent;

    impl TryFrom<&ClipboardEvent> for Event {
        type Error = BrowserEventError;

        fn try_from(event: &ClipboardEvent) -> Result<Self, Self::Error> {
            let data = event
                .clipboard_data()
                .ok_or(BrowserEventError::MissingClipboardData)?;
            data.get_data("text/plain")
                .map(Event::Paste)
                .map_err(|_error| BrowserEventError::ClipboardRead)
        }
    }

    impl TryFrom<ClipboardEvent> for Event {
        type Error = BrowserEventError;

        fn try_from(event: ClipboardEvent) -> Result<Self, Self::Error> {
            Self::try_from(&event)
        }
    }
}

#[cfg(feature = "ratzilla")]
mod ratzilla_conv {
    use super::{
        Event, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind, Unsupported,
    };

    fn mouse_button(button: ratzilla::event::MouseButton) -> Result<MouseButton, Unsupported> {
        use ratzilla::event::MouseButton as R;
        match button {
            R::Left => Ok(MouseButton::Left),
            R::Right => Ok(MouseButton::Right),
            R::Middle => Ok(MouseButton::Middle),
            R::Back | R::Forward | R::Unidentified => Err(Unsupported),
        }
    }

    impl TryFrom<ratzilla::event::KeyEvent> for KeyEvent {
        type Error = Unsupported;

        fn try_from(event: ratzilla::event::KeyEvent) -> Result<Self, Self::Error> {
            use ratzilla::event::KeyCode as R;
            let code = match event.code {
                R::Char(c) => KeyCode::Char(c),
                R::F(n) => KeyCode::F(n),
                R::Backspace => KeyCode::Backspace,
                R::Enter => KeyCode::Enter,
                R::Left => KeyCode::Left,
                R::Right => KeyCode::Right,
                R::Up => KeyCode::Up,
                R::Down => KeyCode::Down,
                R::Tab if event.shift => KeyCode::BackTab,
                R::Tab => KeyCode::Tab,
                R::Delete => KeyCode::Delete,
                R::Home => KeyCode::Home,
                R::End => KeyCode::End,
                R::PageUp => KeyCode::PageUp,
                R::PageDown => KeyCode::PageDown,
                R::Esc => KeyCode::Esc,
                R::Unidentified => return Err(Unsupported),
            };
            Ok(Self {
                code,
                modifiers: Modifiers {
                    ctrl: event.ctrl,
                    alt: event.alt,
                    shift: event.shift,
                },
            })
        }
    }

    impl TryFrom<ratzilla::event::KeyEvent> for Event {
        type Error = Unsupported;

        fn try_from(event: ratzilla::event::KeyEvent) -> Result<Self, Self::Error> {
            KeyEvent::try_from(event).map(Event::Key)
        }
    }

    /// ratzilla reports browser cell coordinates directly, alongside its own
    /// `SingleClick`/`DoubleClick` recognition. We take only the raw
    /// `Moved`/`ButtonDown`/`ButtonUp` transitions: the runtime synthesizes
    /// `Click` and `Drag` uniformly across backends, so ratzilla's
    /// pre-recognized clicks are dropped rather than become a second, divergent
    /// source of click events. `Entered` has no runtime equivalent; `Exited`
    /// cancels the runtime's tracked pointer gesture.
    impl TryFrom<ratzilla::event::MouseEvent> for MouseEvent {
        type Error = Unsupported;

        fn try_from(event: ratzilla::event::MouseEvent) -> Result<Self, Self::Error> {
            use ratzilla::event::MouseEventKind as R;
            let kind = match event.kind {
                R::Moved => MouseKind::Moved,
                R::ButtonDown(button) => MouseKind::Down(mouse_button(button)?),
                R::ButtonUp(button) => MouseKind::Up(mouse_button(button)?),
                R::SingleClick(_) | R::DoubleClick(_) | R::Entered | R::Unidentified => {
                    return Err(Unsupported);
                }
                R::Exited => MouseKind::Exited,
            };
            Ok(Self {
                kind,
                column: event.col,
                row: event.row,
                modifiers: Modifiers {
                    ctrl: event.ctrl,
                    alt: event.alt,
                    shift: event.shift,
                },
            })
        }
    }

    impl TryFrom<ratzilla::event::MouseEvent> for Event {
        type Error = Unsupported;

        fn try_from(event: ratzilla::event::MouseEvent) -> Result<Self, Self::Error> {
            MouseEvent::try_from(event).map(Event::Mouse)
        }
    }
}

#[cfg(feature = "crossterm")]
mod crossterm_conv {
    use super::{
        Event, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind, ScrollDirection,
        Unsupported,
    };
    use ratatui::crossterm::event as ct;

    fn modifiers(held: ct::KeyModifiers) -> Modifiers {
        Modifiers {
            ctrl: held.contains(ct::KeyModifiers::CONTROL),
            alt: held.contains(ct::KeyModifiers::ALT),
            shift: held.contains(ct::KeyModifiers::SHIFT),
        }
    }

    fn mouse_button(button: ct::MouseButton) -> MouseButton {
        match button {
            ct::MouseButton::Left => MouseButton::Left,
            ct::MouseButton::Right => MouseButton::Right,
            ct::MouseButton::Middle => MouseButton::Middle,
        }
    }

    impl From<ct::MouseEvent> for MouseEvent {
        fn from(event: ct::MouseEvent) -> Self {
            use ct::MouseEventKind as K;
            let kind = match event.kind {
                K::Down(b) => MouseKind::Down(mouse_button(b)),
                K::Up(b) => MouseKind::Up(mouse_button(b)),
                K::Drag(b) => MouseKind::Drag(mouse_button(b)),
                K::Moved => MouseKind::Moved,
                K::ScrollUp => MouseKind::Scroll(ScrollDirection::Up),
                K::ScrollDown => MouseKind::Scroll(ScrollDirection::Down),
                K::ScrollLeft => MouseKind::Scroll(ScrollDirection::Left),
                K::ScrollRight => MouseKind::Scroll(ScrollDirection::Right),
            };
            Self {
                kind,
                column: event.column,
                row: event.row,
                modifiers: modifiers(event.modifiers),
            }
        }
    }

    impl TryFrom<ct::KeyEvent> for KeyEvent {
        type Error = Unsupported;

        fn try_from(event: ct::KeyEvent) -> Result<Self, Self::Error> {
            if event.kind == ct::KeyEventKind::Release {
                return Err(Unsupported);
            }
            let code = match event.code {
                ct::KeyCode::Char(c) => KeyCode::Char(c),
                ct::KeyCode::F(n) => KeyCode::F(n),
                ct::KeyCode::Backspace => KeyCode::Backspace,
                ct::KeyCode::Enter => KeyCode::Enter,
                ct::KeyCode::Left => KeyCode::Left,
                ct::KeyCode::Right => KeyCode::Right,
                ct::KeyCode::Up => KeyCode::Up,
                ct::KeyCode::Down => KeyCode::Down,
                ct::KeyCode::Tab => KeyCode::Tab,
                ct::KeyCode::BackTab => KeyCode::BackTab,
                ct::KeyCode::Delete => KeyCode::Delete,
                ct::KeyCode::Home => KeyCode::Home,
                ct::KeyCode::End => KeyCode::End,
                ct::KeyCode::PageUp => KeyCode::PageUp,
                ct::KeyCode::PageDown => KeyCode::PageDown,
                ct::KeyCode::Esc => KeyCode::Esc,
                _ => return Err(Unsupported),
            };
            Ok(Self {
                code,
                modifiers: modifiers(event.modifiers),
            })
        }
    }

    impl TryFrom<ct::Event> for Event {
        type Error = Unsupported;

        fn try_from(event: ct::Event) -> Result<Self, Self::Error> {
            match event {
                ct::Event::Key(key) => KeyEvent::try_from(key).map(Event::Key),
                ct::Event::Mouse(mouse) => Ok(Event::Mouse(mouse.into())),
                ct::Event::Paste(s) => Ok(Event::Paste(s)),
                _ => Err(Unsupported),
            }
        }
    }
}

#[cfg(feature = "termina")]
mod termina_conv {
    use super::{
        Event, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind, ScrollDirection,
        Unsupported,
    };
    use termina::event as tn;

    fn modifiers(held: tn::Modifiers) -> Modifiers {
        Modifiers {
            ctrl: held.contains(tn::Modifiers::CONTROL),
            alt: held.contains(tn::Modifiers::ALT),
            shift: held.contains(tn::Modifiers::SHIFT),
        }
    }

    fn mouse_button(button: tn::MouseButton) -> MouseButton {
        match button {
            tn::MouseButton::Left => MouseButton::Left,
            tn::MouseButton::Right => MouseButton::Right,
            tn::MouseButton::Middle => MouseButton::Middle,
        }
    }

    /// Termina reports zero-based cells, the same space a rendered
    /// [`Rect`](ratatui::layout::Rect) is in, so the coordinates pass through.
    impl From<tn::MouseEvent> for MouseEvent {
        fn from(event: tn::MouseEvent) -> Self {
            use tn::MouseEventKind as K;
            let kind = match event.kind {
                K::Down(b) => MouseKind::Down(mouse_button(b)),
                K::Up(b) => MouseKind::Up(mouse_button(b)),
                K::Drag(b) => MouseKind::Drag(mouse_button(b)),
                K::Moved => MouseKind::Moved,
                K::ScrollUp => MouseKind::Scroll(ScrollDirection::Up),
                K::ScrollDown => MouseKind::Scroll(ScrollDirection::Down),
                K::ScrollLeft => MouseKind::Scroll(ScrollDirection::Left),
                K::ScrollRight => MouseKind::Scroll(ScrollDirection::Right),
            };
            Self {
                kind,
                column: event.column,
                row: event.row,
                modifiers: modifiers(event.modifiers),
            }
        }
    }

    impl TryFrom<tn::KeyEvent> for KeyEvent {
        type Error = Unsupported;

        fn try_from(event: tn::KeyEvent) -> Result<Self, Self::Error> {
            // A repeat is a press the user is still making, so it routes; a
            // release is the one kind this vocabulary has no place for.
            if event.kind == tn::KeyEventKind::Release {
                return Err(Unsupported);
            }
            let code = match event.code {
                tn::KeyCode::Char(c) => KeyCode::Char(c),
                tn::KeyCode::Function(n) => KeyCode::F(n),
                tn::KeyCode::Backspace => KeyCode::Backspace,
                tn::KeyCode::Enter => KeyCode::Enter,
                tn::KeyCode::Left => KeyCode::Left,
                tn::KeyCode::Right => KeyCode::Right,
                tn::KeyCode::Up => KeyCode::Up,
                tn::KeyCode::Down => KeyCode::Down,
                tn::KeyCode::Tab => KeyCode::Tab,
                tn::KeyCode::BackTab => KeyCode::BackTab,
                tn::KeyCode::Delete => KeyCode::Delete,
                tn::KeyCode::Home => KeyCode::Home,
                tn::KeyCode::End => KeyCode::End,
                tn::KeyCode::PageUp => KeyCode::PageUp,
                tn::KeyCode::PageDown => KeyCode::PageDown,
                tn::KeyCode::Escape => KeyCode::Esc,
                _ => return Err(Unsupported),
            };
            Ok(Self {
                code,
                modifiers: modifiers(event.modifiers),
            })
        }
    }

    /// Termina keeps terminal *responses* — CSI, OSC, DCS — in the same enum as
    /// input, so a query reply reaching the app's event loop is an ordinary
    /// unsupported event rather than the fake keystrokes crossterm invents for
    /// it.
    impl TryFrom<termina::Event> for Event {
        type Error = Unsupported;

        fn try_from(event: termina::Event) -> Result<Self, Self::Error> {
            match event {
                termina::Event::Key(key) => KeyEvent::try_from(key).map(Event::Key),
                termina::Event::Mouse(mouse) => Ok(Event::Mouse(mouse.into())),
                termina::Event::Paste(s) => Ok(Event::Paste(s)),
                _ => Err(Unsupported),
            }
        }
    }
}

#[cfg(all(test, feature = "termina"))]
mod termina_tests {
    use super::{
        Event, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind, ScrollDirection,
        Unsupported,
    };
    use termina::event as tn;

    fn key(code: tn::KeyCode, modifiers: tn::Modifiers) -> tn::KeyEvent {
        tn::KeyEvent::new(code, modifiers)
    }

    #[test]
    fn termina_shift_backtab_keeps_normalized_code_and_modifiers() {
        let event = KeyEvent::try_from(key(tn::KeyCode::BackTab, tn::Modifiers::SHIFT));

        assert_eq!(
            event,
            Ok(KeyEvent {
                code: KeyCode::BackTab,
                modifiers: Modifiers {
                    shift: true,
                    ..Modifiers::NONE
                },
            })
        );
    }

    #[test]
    fn terminas_own_names_for_escape_and_function_keys_map_across() {
        assert_eq!(
            KeyEvent::try_from(key(tn::KeyCode::Escape, tn::Modifiers::NONE)),
            Ok(KeyEvent::new(KeyCode::Esc))
        );
        assert_eq!(
            KeyEvent::try_from(key(tn::KeyCode::Function(7), tn::Modifiers::NONE)),
            Ok(KeyEvent::new(KeyCode::F(7)))
        );
    }

    #[test]
    fn every_modifier_the_vocabulary_names_survives_at_once() {
        let held = tn::Modifiers::CONTROL | tn::Modifiers::ALT | tn::Modifiers::SHIFT;

        assert_eq!(
            KeyEvent::try_from(key(tn::KeyCode::Char('K'), held)),
            Ok(KeyEvent {
                code: KeyCode::Char('K'),
                modifiers: Modifiers {
                    ctrl: true,
                    alt: true,
                    shift: true,
                },
            })
        );
    }

    #[test]
    fn a_key_release_does_not_convert_but_a_repeat_does() {
        let held = tn::KeyEvent {
            code: tn::KeyCode::Char('j'),
            kind: tn::KeyEventKind::Release,
            modifiers: tn::Modifiers::NONE,
            state: tn::KeyEventState::NONE,
        };

        assert_eq!(KeyEvent::try_from(held), Err(Unsupported));
        assert_eq!(
            KeyEvent::try_from(tn::KeyEvent {
                kind: tn::KeyEventKind::Repeat,
                ..held
            }),
            Ok(KeyEvent::new(KeyCode::Char('j')))
        );
    }

    #[test]
    fn keys_outside_the_vocabulary_stay_unsupported() {
        for code in [
            tn::KeyCode::Insert,
            tn::KeyCode::CapsLock,
            tn::KeyCode::Menu,
            tn::KeyCode::Null,
            tn::KeyCode::Modifier(tn::ModifierKeyCode::LeftShift),
            tn::KeyCode::Media(tn::MediaKeyCode::Play),
        ] {
            assert_eq!(
                KeyEvent::try_from(key(code, tn::Modifiers::NONE)),
                Err(Unsupported),
                "{code:?} has no place in the runtime vocabulary"
            );
        }
    }

    #[test]
    fn termina_mouse_cells_and_kinds_pass_through_unshifted() {
        // Every button, because a transposed pair reads correctly from any one
        // of them: the middle button is its own opposite.
        let buttons = [
            (tn::MouseButton::Left, MouseButton::Left),
            (tn::MouseButton::Right, MouseButton::Right),
            (tn::MouseButton::Middle, MouseButton::Middle),
        ];

        for (reported, button) in buttons {
            let event = MouseEvent::from(tn::MouseEvent {
                kind: tn::MouseEventKind::Drag(reported),
                column: 3,
                row: 4,
                modifiers: tn::Modifiers::CONTROL,
            });

            assert_eq!(
                event,
                MouseEvent {
                    kind: MouseKind::Drag(button),
                    column: 3,
                    row: 4,
                    modifiers: Modifiers {
                        ctrl: true,
                        ..Modifiers::NONE
                    },
                },
                "termina's {reported:?} is the runtime's {button:?}"
            );
        }
    }

    #[test]
    fn presses_and_releases_keep_their_button_too() {
        for (reported, button) in [
            (tn::MouseButton::Left, MouseButton::Left),
            (tn::MouseButton::Right, MouseButton::Right),
            (tn::MouseButton::Middle, MouseButton::Middle),
        ] {
            let down = MouseEvent::from(tn::MouseEvent {
                kind: tn::MouseEventKind::Down(reported),
                column: 0,
                row: 0,
                modifiers: tn::Modifiers::NONE,
            });
            let up = MouseEvent::from(tn::MouseEvent {
                kind: tn::MouseEventKind::Up(reported),
                column: 0,
                row: 0,
                modifiers: tn::Modifiers::NONE,
            });

            assert_eq!(down.kind, MouseKind::Down(button), "{reported:?} press");
            assert_eq!(up.kind, MouseKind::Up(button), "{reported:?} release");
        }
    }

    #[test]
    fn all_four_scroll_axes_map_to_their_directions() {
        let axes = [
            (tn::MouseEventKind::ScrollUp, ScrollDirection::Up),
            (tn::MouseEventKind::ScrollDown, ScrollDirection::Down),
            (tn::MouseEventKind::ScrollLeft, ScrollDirection::Left),
            (tn::MouseEventKind::ScrollRight, ScrollDirection::Right),
        ];

        for (reported, direction) in axes {
            let event = MouseEvent::from(tn::MouseEvent {
                kind: reported,
                column: 0,
                row: 0,
                modifiers: tn::Modifiers::NONE,
            });
            assert_eq!(
                event.kind,
                MouseKind::Scroll(direction),
                "termina's {reported:?} is the runtime's {direction:?}"
            );
        }
    }

    #[test]
    fn a_bracketed_paste_arrives_whole() {
        let event = Event::try_from(termina::Event::Paste("two\nlines".to_owned()));

        assert_eq!(event, Ok(Event::Paste("two\nlines".to_owned())));
    }

    #[test]
    fn escape_replies_and_focus_notices_are_not_app_events() {
        use termina::escape::csi::{Csi, Device};

        // The reply to a startup query, arriving after the query window has
        // closed: an ignored event, not the burst of fake keystrokes crossterm
        // turns the same bytes into.
        let reply = Event::try_from(termina::Event::Csi(Csi::Device(Device::DeviceAttributes(
            (),
        ))));

        assert_eq!(reply, Err(Unsupported));
        assert_eq!(Event::try_from(termina::Event::FocusIn), Err(Unsupported));
    }
}

#[cfg(all(test, feature = "crossterm"))]
mod crossterm_tests {
    use super::{KeyCode, KeyEvent, Modifiers};
    use ratatui::crossterm::event as ct;

    #[test]
    fn crossterm_shift_backtab_keeps_normalized_code_and_modifiers() {
        let event = KeyEvent::try_from(ct::KeyEvent::new(
            ct::KeyCode::BackTab,
            ct::KeyModifiers::SHIFT,
        ));

        assert_eq!(
            event,
            Ok(KeyEvent {
                code: KeyCode::BackTab,
                modifiers: Modifiers {
                    shift: true,
                    ..Modifiers::NONE
                },
            })
        );
    }
}

#[cfg(all(test, feature = "ratzilla"))]
mod ratzilla_tests {
    use super::{
        Event, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind, Unsupported,
    };
    use ratzilla::event::{
        KeyCode as RatzillaKeyCode, KeyEvent as RatzillaKeyEvent, MouseButton as RatzillaButton,
        MouseEvent as RatzillaMouseEvent, MouseEventKind as RatzillaMouseKind,
    };

    #[test]
    fn ratzilla_shift_tab_normalizes_to_backtab_with_shift() {
        let event = KeyEvent::try_from(RatzillaKeyEvent {
            code: RatzillaKeyCode::Tab,
            ctrl: false,
            alt: false,
            shift: true,
        });

        assert_eq!(
            event,
            Ok(KeyEvent {
                code: KeyCode::BackTab,
                modifiers: Modifiers {
                    shift: true,
                    ..Modifiers::NONE
                },
            })
        );
    }

    fn ratzilla_mouse(kind: RatzillaMouseKind) -> RatzillaMouseEvent {
        RatzillaMouseEvent {
            kind,
            col: 3,
            row: 4,
            ctrl: true,
            alt: false,
            shift: true,
        }
    }

    #[test]
    fn ratzilla_raw_mouse_event_maps_to_runtime_mouse_event() {
        let event = Event::try_from(ratzilla_mouse(RatzillaMouseKind::ButtonDown(
            RatzillaButton::Left,
        )));

        assert_eq!(
            event,
            Ok(Event::Mouse(MouseEvent {
                kind: MouseKind::Down(MouseButton::Left),
                column: 3,
                row: 4,
                modifiers: Modifiers {
                    ctrl: true,
                    alt: false,
                    shift: true,
                },
            }))
        );
    }

    #[test]
    fn ratzilla_button_up_and_moved_map_to_runtime_kinds() {
        let up = MouseEvent::try_from(ratzilla_mouse(RatzillaMouseKind::ButtonUp(
            RatzillaButton::Left,
        )))
        .map(|event| event.kind);
        assert_eq!(up, Ok(MouseKind::Up(MouseButton::Left)));

        let moved =
            MouseEvent::try_from(ratzilla_mouse(RatzillaMouseKind::Moved)).map(|event| event.kind);
        assert_eq!(moved, Ok(MouseKind::Moved));
    }

    #[test]
    fn ratzilla_click_events_stay_unsupported() {
        let event = MouseEvent::try_from(ratzilla_mouse(RatzillaMouseKind::SingleClick(
            RatzillaButton::Left,
        )));

        assert_eq!(event, Err(Unsupported));
    }

    #[test]
    fn ratzilla_exit_maps_to_runtime_exit() {
        let event = MouseEvent::try_from(ratzilla_mouse(RatzillaMouseKind::Exited));

        assert_eq!(event.map(|event| event.kind), Ok(MouseKind::Exited));
    }

    #[test]
    fn ratzilla_back_button_stays_unsupported() {
        let event = MouseEvent::try_from(ratzilla_mouse(RatzillaMouseKind::ButtonDown(
            RatzillaButton::Back,
        )));

        assert_eq!(event, Err(Unsupported));
    }
}

#[cfg(all(test, target_arch = "wasm32", feature = "ratzilla"))]
mod browser_tests {
    use super::Event;
    use crate::runtime::BrowserEventError;
    use web_sys::ClipboardEvent;

    #[test]
    fn typed_browser_event_conversions_are_available() {
        fn assert_try_from_ref<T>()
        where
            for<'a> Event: TryFrom<&'a T, Error = BrowserEventError>,
        {
        }

        assert_try_from_ref::<ClipboardEvent>();
    }
}
