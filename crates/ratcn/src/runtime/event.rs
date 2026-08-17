//! Normalized input events.
//!
//! The interaction runtime defines its own event vocabulary so components match
//! one set of types across backends. Conversions from crossterm and ratzilla are
//! feature-gated (`crossterm`, `ratzilla`).

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
/// Deliberately small: it covers the keys terminal UIs actually bind, not
/// everything a backend can report. A backend key with no variant here does not
/// convert into an [`Event`] and is ignored.
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
/// `column`/`row` are terminal cells, 0-based and screen-absolute — the same
/// coordinate space as a component's rendered [`Rect`](ratatui::layout::Rect),
/// so the runtime can hit-test events against children directly. (crossterm
/// already speaks cells; the browser backend converts pixels to cells before
/// constructing this.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseEvent {
    /// What the pointer did.
    pub kind: MouseKind,
    /// Screen-absolute cell column, 0-based.
    pub column: u16,
    /// Screen-absolute cell row, 0-based.
    pub row: u16,
    /// Ctrl/Alt/Shift state during the event.
    pub modifiers: Modifiers,
}

/// What a [`MouseEvent`] is.
///
/// [`Click`](MouseKind::Click) and [`DragEnd`](MouseKind::DragEnd) are
/// synthesized by [`Ratcn`](super::Ratcn)'s internal mouse tracker.
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

/// Internal normalizer owned by [`Ratcn`](super::Ratcn). It turns a backend's
/// raw `Down`/`Up`/`Moved` stream into the dispatched sequence, synthesizing the
/// [`Click`](MouseKind::Click), [`Drag`](MouseKind::Drag), and
/// [`DragEnd`](MouseKind::DragEnd) gestures a backend cannot provide uniformly.
/// Apps pass raw mouse events to [`Ratcn::handle_event`](super::Ratcn::handle_event)
/// and do not own a separate tracker.
///
/// A release follows up as at most one gesture. The tracker knows where the
/// pointer went but not what is under it, so the caller decides the one
/// question that needs the surface — whether the release lands on the
/// component the press landed on, unclaimed by any capture — and passes it to
/// [`feed`](MouseTracker::feed) as `releases_on_press_target`. That answer
/// makes the release a `Click`; otherwise a press that moved emits `DragEnd`,
/// and a press that did not emits nothing. So a click never fires on a
/// different target, and a claimed drag never also clicks. An
/// [`Exited`](MouseKind::Exited) clears tracked presses, so a release outside
/// the interactive grid cannot become a drag after the pointer returns.
#[derive(Debug, Default, Clone)]
pub(crate) struct MouseTracker {
    /// One entry per held button, oldest press first — so the last is the one
    /// movement belongs to. Kept as a list rather than a map plus an order,
    /// because the order *is* the state and a hand-held pointer holds at most
    /// a few buttons.
    presses: Vec<Press>,
}

#[derive(Debug, Clone, Copy)]
struct Press {
    button: MouseButton,
    column: u16,
    row: u16,
    /// The pointer has left the pressed cell, so this press has emitted at
    /// least one [`Drag`](MouseKind::Drag). Whether that makes the release a
    /// drag end is decided by the caller, not here.
    moved: bool,
}

impl MouseTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Feed one raw mouse event; return the events to dispatch, in order.
    ///
    /// `releases_on_press_target` answers the surface-dependent half of a
    /// release: is this `Up` on the same component its press hit, and not the
    /// end of a drag some component claimed? It is read only for
    /// [`Up`](MouseKind::Up) and ignored for every other kind.
    pub(crate) fn feed(
        &mut self,
        event: MouseEvent,
        releases_on_press_target: bool,
    ) -> Vec<MouseEvent> {
        match event.kind {
            MouseKind::Down(button) => {
                self.presses.retain(|press| press.button != button);
                self.presses.push(Press {
                    button,
                    column: event.column,
                    row: event.row,
                    moved: false,
                });
                vec![event]
            }
            MouseKind::Up(button) => {
                let press = self
                    .presses
                    .iter()
                    .position(|press| press.button == button)
                    .map(|index| self.presses.remove(index));
                let follow = match press {
                    // Landing back on the press target wins over having moved:
                    // movement nobody claimed as a drag is drift within the
                    // control, and drift must not eat the click. The caller
                    // folds captures into the flag, so a claimed gesture
                    // reaches this arm as `false` and ends as `DragEnd`.
                    Some(_) if releases_on_press_target => Some(MouseKind::Click(button)),
                    Some(press) if press.moved => Some(MouseKind::DragEnd(button)),
                    _ => None,
                };
                let mut out = vec![event];
                out.extend(follow.map(|kind| MouseEvent { kind, ..event }));
                out
            }
            // Movement belongs to the most recent press: the last entry.
            MouseKind::Moved => match self.presses.last_mut() {
                Some(press) => {
                    if !press.moved && press.column == event.column && press.row == event.row {
                        return Vec::new();
                    }
                    press.moved = true;
                    vec![MouseEvent {
                        kind: MouseKind::Drag(press.button),
                        ..event
                    }]
                }
                None => vec![event],
            },
            // Some backends (crossterm) deliver `Drag` natively; it still marks
            // the press as moved so its release ends as a `DragEnd`.
            MouseKind::Drag(button) => {
                if let Some(press) = self.press_mut(button) {
                    press.moved = true;
                }
                vec![event]
            }
            MouseKind::Exited => {
                self.clear();
                vec![event]
            }
            // Scroll (and any already-synthesized kind) pass through.
            _ => vec![event],
        }
    }

    pub(crate) fn pressed_buttons(&self) -> Vec<MouseButton> {
        self.presses.iter().map(|press| press.button).collect()
    }

    pub(crate) fn has_pressed_button(&self) -> bool {
        !self.presses.is_empty()
    }

    /// Has the press held by `button` produced motion? The caller needs this
    /// to tell a claimed *drag* from a claimed press that never moved, which
    /// decides whether its release can still be a click.
    pub(crate) fn press_moved(&self, button: MouseButton) -> bool {
        self.press(button).is_some_and(|press| press.moved)
    }

    pub(crate) fn clear(&mut self) {
        self.presses.clear();
    }

    fn press(&self, button: MouseButton) -> Option<&Press> {
        self.presses.iter().find(|press| press.button == button)
    }

    fn press_mut(&mut self, button: MouseButton) -> Option<&mut Press> {
        self.presses.iter_mut().find(|press| press.button == button)
    }
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
    /// `Moved`/`ButtonDown`/`ButtonUp` transitions: the runtime's `MouseTracker`
    /// synthesizes `Click` and `Drag` uniformly across backends, so ratzilla's
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
                modifiers: Modifiers {
                    ctrl: event.modifiers.contains(ct::KeyModifiers::CONTROL),
                    alt: event.modifiers.contains(ct::KeyModifiers::ALT),
                    shift: event.modifiers.contains(ct::KeyModifiers::SHIFT),
                },
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

#[cfg(test)]
mod tracker_tests {
    use super::{Modifiers, MouseButton, MouseEvent, MouseKind, MouseTracker, Unsupported};

    #[test]
    fn unsupported_is_a_standard_error_with_a_message() {
        fn assert_error<T: std::error::Error>() {}

        assert_error::<Unsupported>();
        assert_eq!(Unsupported.to_string(), "unsupported input event");
    }

    fn mouse(kind: MouseKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: Modifiers::NONE,
        }
    }

    fn kinds(events: Vec<MouseEvent>) -> Vec<MouseKind> {
        events.into_iter().map(|event| event.kind).collect()
    }

    /// Feed an event whose release — if it is one — the caller reports as
    /// landing somewhere other than the press target, or as claimed by a
    /// capture. Every kind but `Up` ignores the flag, so this is also the
    /// plain "feed this" helper.
    fn feed(tracker: &mut MouseTracker, event: MouseEvent) -> Vec<MouseKind> {
        kinds(tracker.feed(event, false))
    }

    /// Feed a release the caller reports as landing back on the component its
    /// press hit, unclaimed by any capture.
    fn feed_on_target(tracker: &mut MouseTracker, event: MouseEvent) -> Vec<MouseKind> {
        kinds(tracker.feed(event, true))
    }

    #[test]
    fn a_press_and_release_on_one_component_clicks() {
        let mut tracker = MouseTracker::new();

        feed(
            &mut tracker,
            mouse(MouseKind::Down(MouseButton::Left), 2, 2),
        );
        assert_eq!(
            feed_on_target(&mut tracker, mouse(MouseKind::Up(MouseButton::Left), 2, 2)),
            vec![
                MouseKind::Up(MouseButton::Left),
                MouseKind::Click(MouseButton::Left)
            ]
        );
    }

    #[test]
    fn same_cell_motion_emits_nothing_at_all() {
        let mut tracker = MouseTracker::new();

        feed(
            &mut tracker,
            mouse(MouseKind::Down(MouseButton::Left), 2, 2),
        );
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Moved, 2, 2)),
            Vec::<MouseKind>::new()
        );
        assert_eq!(
            feed_on_target(&mut tracker, mouse(MouseKind::Up(MouseButton::Left), 2, 2)),
            vec![
                MouseKind::Up(MouseButton::Left),
                MouseKind::Click(MouseButton::Left)
            ]
        );
    }

    #[test]
    fn drift_within_one_component_still_clicks_it() {
        // The pointer left its starting cell, so `Drag` fired — but nothing
        // claimed the gesture and the release is still on the press target, so
        // it is a click, not a drag end. Losing this click is the whole reason
        // the caller answers the target question.
        let mut tracker = MouseTracker::new();

        feed(
            &mut tracker,
            mouse(MouseKind::Down(MouseButton::Left), 2, 2),
        );
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Moved, 3, 2)),
            vec![MouseKind::Drag(MouseButton::Left)]
        );
        assert_eq!(
            feed_on_target(&mut tracker, mouse(MouseKind::Up(MouseButton::Left), 3, 2)),
            vec![
                MouseKind::Up(MouseButton::Left),
                MouseKind::Click(MouseButton::Left)
            ]
        );
    }

    #[test]
    fn a_release_off_the_press_target_without_motion_is_only_an_up() {
        let mut tracker = MouseTracker::new();

        feed(
            &mut tracker,
            mouse(MouseKind::Down(MouseButton::Left), 2, 2),
        );
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Up(MouseButton::Left), 3, 2)),
            vec![MouseKind::Up(MouseButton::Left)]
        );
    }

    #[test]
    fn a_moved_press_released_off_target_ends_as_drag_end() {
        let mut tracker = MouseTracker::new();

        feed(
            &mut tracker,
            mouse(MouseKind::Down(MouseButton::Left), 2, 2),
        );
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Moved, 5, 3)),
            vec![MouseKind::Drag(MouseButton::Left)]
        );
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Up(MouseButton::Left), 5, 3)),
            vec![
                MouseKind::Up(MouseButton::Left),
                MouseKind::DragEnd(MouseButton::Left)
            ]
        );
    }

    #[test]
    fn a_claimed_drag_released_where_it_started_is_not_a_click() {
        // Returning to the starting cell does not undo a drag: the caller
        // reports a captured gesture as off-target, so it ends as `DragEnd`.
        let mut tracker = MouseTracker::new();

        feed(
            &mut tracker,
            mouse(MouseKind::Down(MouseButton::Left), 2, 2),
        );
        feed(&mut tracker, mouse(MouseKind::Moved, 5, 3));
        feed(&mut tracker, mouse(MouseKind::Moved, 2, 2));
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Up(MouseButton::Left), 2, 2)),
            vec![
                MouseKind::Up(MouseButton::Left),
                MouseKind::DragEnd(MouseButton::Left)
            ]
        );
    }

    #[test]
    fn a_native_backend_drag_also_ends_as_drag_end() {
        // crossterm delivers `Drag` itself rather than held `Moved`s.
        let mut tracker = MouseTracker::new();

        feed(
            &mut tracker,
            mouse(MouseKind::Down(MouseButton::Left), 2, 2),
        );
        feed(
            &mut tracker,
            mouse(MouseKind::Drag(MouseButton::Left), 5, 3),
        );
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Up(MouseButton::Left), 5, 3)),
            vec![
                MouseKind::Up(MouseButton::Left),
                MouseKind::DragEnd(MouseButton::Left)
            ]
        );
    }

    #[test]
    fn releasing_the_latest_button_preserves_an_earlier_held_press() {
        let mut tracker = MouseTracker::new();

        feed(
            &mut tracker,
            mouse(MouseKind::Down(MouseButton::Left), 1, 1),
        );
        feed(
            &mut tracker,
            mouse(MouseKind::Down(MouseButton::Right), 2, 2),
        );
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Moved, 4, 2)),
            vec![MouseKind::Drag(MouseButton::Right)]
        );
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Up(MouseButton::Right), 4, 2)),
            vec![
                MouseKind::Up(MouseButton::Right),
                MouseKind::DragEnd(MouseButton::Right)
            ]
        );
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Moved, 3, 1)),
            vec![MouseKind::Drag(MouseButton::Left)]
        );
    }

    #[test]
    fn interleaved_release_clicks_only_its_matching_press() {
        let mut tracker = MouseTracker::new();

        feed(
            &mut tracker,
            mouse(MouseKind::Down(MouseButton::Left), 1, 1),
        );
        feed(
            &mut tracker,
            mouse(MouseKind::Down(MouseButton::Right), 2, 2),
        );
        assert_eq!(
            feed_on_target(&mut tracker, mouse(MouseKind::Up(MouseButton::Left), 1, 1)),
            vec![
                MouseKind::Up(MouseButton::Left),
                MouseKind::Click(MouseButton::Left)
            ]
        );
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Moved, 4, 2)),
            vec![MouseKind::Drag(MouseButton::Right)]
        );
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Up(MouseButton::Right), 4, 2)),
            vec![
                MouseKind::Up(MouseButton::Right),
                MouseKind::DragEnd(MouseButton::Right)
            ]
        );
    }

    #[test]
    fn exit_clears_presses_so_reentry_motion_is_not_a_drag() {
        let mut tracker = MouseTracker::new();

        feed(
            &mut tracker,
            mouse(MouseKind::Down(MouseButton::Left), 1, 1),
        );
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Exited, 1, 1)),
            vec![MouseKind::Exited]
        );
        assert!(tracker.pressed_buttons().is_empty());
        assert_eq!(
            feed(&mut tracker, mouse(MouseKind::Moved, 1, 1)),
            vec![MouseKind::Moved]
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
