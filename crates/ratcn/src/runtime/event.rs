//! The event vocabulary: what arrives, and what a component answers with.
//!
//! The interaction runtime defines its own event types so components match one
//! set of them across backends. Conversions from crossterm, termina, and
//! ratzilla are feature-gated (`crossterm`, `termina`, `ratzilla`).
//! [`EventResult`] is the answering half, carried back up the same chain the
//! event travelled down.

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
    /// The user asked to copy, through the platform's own gesture — a
    /// browser's `copy` event. A component that copies writes the text with
    /// [`EventCtx::set_clipboard`](super::EventCtx::set_clipboard).
    Copy,
    /// The user asked to cut, as [`Copy`](Self::Copy) does to copy.
    Cut,
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

/// What a component did with an event, and what should happen next.
///
/// Events travel from a component up through its ancestors. This value decides
/// whether that continues: `Ignored` passes the event to the parent, while both
/// `Consumed` and `Emit` stop it there.
///
/// The same enum comes back out of
/// [`Ratcn::handle_event`](super::Ratcn::handle_event), so the app sees the
/// final outcome after bubbling has finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventResult<Msg> {
    /// Not handled here. The parent gets a chance at it, and if nothing in the
    /// chain handles it the app sees `Ignored` and can treat it as a global
    /// hotkey.
    Ignored,
    /// Handled, with nothing for the app to do — a keypress that only moved an
    /// internal cursor, for example.
    Consumed,
    /// Handled, and the app should apply this message. Implies consumed.
    ///
    /// Components never write app state themselves; this message is how a
    /// change reaches the app's update function.
    Emit(Msg),
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

    /// The focus-traversal step represented by this key press.
    ///
    /// Shift+Tab is accepted in either backend representation: `Tab` with
    /// Shift held, or the normalized `BackTab` code (with or without Shift
    /// still present). Ctrl and Alt variants are not traversal keys.
    #[must_use]
    pub const fn traversal_step(self) -> Option<super::Step> {
        if self.modifiers.ctrl || self.modifiers.alt {
            return None;
        }
        match self.code {
            KeyCode::Tab if self.modifiers.shift => Some(super::Step::Backward),
            KeyCode::Tab => Some(super::Step::Forward),
            KeyCode::BackTab => Some(super::Step::Backward),
            _ => None,
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
/// [`viewport`](super::DeclareCtx::viewport). Every backend conversion
/// delivers cells.
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
/// Exhaustive: these are the four axes a terminal or browser reports.
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

#[cfg(all(target_arch = "wasm32", feature = "ratzilla"))]
mod browser_clipboard {
    use std::{cell::RefCell, io, rc::Rc};

    use wasm_bindgen_futures::JsFuture;
    use web_sys::{
        AddEventListenerOptions, ClipboardEvent, Document, Element, HtmlElement, KeyboardEvent,
        MouseEventInit, WheelEvent, Window,
        wasm_bindgen::{JsCast, JsValue, prelude::Closure},
    };

    use super::super::PointerShape;
    use super::ratzilla_conv::{Wheel, non_ascii_key};
    use super::{Event, Modifiers, MouseEvent, MouseKind, ScrollDirection};

    /// The browser's clipboard, wired to the runtime for as long as the guard
    /// lives. Ratzilla has no callback for the clipboard, so this listens on
    /// the page itself:
    ///
    /// - The document's `paste`, `copy`, and `cut` events arrive as
    ///   [`Event::Paste`], [`Event::Copy`], and [`Event::Cut`]. Its
    ///   `beforepaste`, `beforecopy`, and `beforecut` are cancelled, which is
    ///   how Safari enables those commands on a canvas with no text selected.
    /// - The clipboard chords are stopped before they reach ratzilla as keys,
    ///   so the browser turns them into those events instead: every
    ///   <kbd>Cmd</kbd> (or <kbd>Super</kbd>) chord, which ratzilla would
    ///   report as the bare letter, and on all but a Mac <kbd>Ctrl+C</kbd>,
    ///   <kbd>Ctrl+X</kbd>, <kbd>Ctrl+V</kbd>, and <kbd>Shift+Delete</kbd>. A Mac is told by
    ///   `navigator.platform`. <kbd>Cmd+A</kbd> is also kept from selecting
    ///   the page's text.
    ///
    /// Focus decides whose an event is: it is the app's while focus is on the
    /// app's element or inside it (`:focus-within`), and the page's otherwise.
    /// A copy or cut the app answers with nothing copies nothing, unless text
    /// inside the app's element is selected, which the browser then copies.
    ///
    /// Install one per app, given its element. Text an app writes outside
    /// those events — Ctrl+C on a Mac, a "Copy" button — goes out through
    /// [`write`](Self::write).
    #[must_use = "dropping the guard removes its browser listeners"]
    pub struct BrowserClipboard {
        window: Window,
        document: Document,
        on_clipboard: Closure<dyn FnMut(ClipboardEvent)>,
        on_before: Closure<dyn FnMut(web_sys::Event)>,
        on_keydown: Closure<dyn FnMut(KeyboardEvent)>,
    }

    impl std::fmt::Debug for BrowserClipboard {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("BrowserClipboard").finish_non_exhaustive()
        }
    }

    /// The document events this routes.
    const EVENTS: [&str; 3] = ["paste", "copy", "cut"];

    /// The document events this cancels, so Safari offers the ones above.
    const BEFORE: [&str; 3] = ["beforepaste", "beforecopy", "beforecut"];

    impl BrowserClipboard {
        /// Install the listeners for the app drawn in `app`: the canvas a
        /// canvas backend draws on, or the element holding it; for a DOM
        /// backend, the element whose id was given as its `grid_id`.
        ///
        /// `on_event` routes one clipboard event and reports whether the app
        /// took it — as a host does by mapping anything but
        /// [`EventResult::Ignored`](super::EventResult::Ignored) to `true`.
        /// `take_clipboard` hands back what the app wrote meanwhile, as
        /// [`Ratcn::take_clipboard`](super::super::Ratcn::take_clipboard)
        /// does. A paste the app took, and a copy or cut that wrote text, are
        /// the app's; anything else is left to the page. An event before the
        /// first render is ignored by the runtime, so the page keeps it.
        ///
        /// # Errors
        ///
        /// Returns an I/O error if there is no window or document, or if either
        /// refuses a listener.
        pub fn install(
            app: &Element,
            mut on_event: impl FnMut(Event) -> bool + 'static,
            mut take_clipboard: impl FnMut() -> Option<String> + 'static,
        ) -> io::Result<Self> {
            let window = web_sys::window().ok_or_else(|| io::Error::other("no window"))?;
            let document = window
                .document()
                .ok_or_else(|| io::Error::other("no document"))?;
            let keyboard_app = app.clone();
            let on_clipboard = Closure::new({
                let app = app.clone();
                move |event: ClipboardEvent| {
                    let Some(data) = event.clipboard_data() else {
                        return;
                    };
                    if !focused(&app) {
                        return;
                    }
                    if event.type_() == "paste" {
                        let Ok(text) = data.get_data("text/plain") else {
                            return;
                        };
                        if on_event(Event::Paste(text)) {
                            event.prevent_default();
                        }
                        // A paste's write goes out as any other event's does.
                        if let Some(text) = take_clipboard() {
                            Self::write(&text);
                        }
                        return;
                    }
                    let _ = on_event(if event.type_() == "cut" {
                        Event::Cut
                    } else {
                        Event::Copy
                    });
                    if let Some(text) = take_clipboard() {
                        let _ = data.set_data("text/plain", &text);
                    } else if selected_within(&app) {
                        // Text of the app's own DOM, which the browser copies.
                        return;
                    }
                    // The app's copy, even of nothing: the browser's own would
                    // copy whatever the page still has selected.
                    event.prevent_default();
                }
            });
            let on_before = Closure::new({
                let app = app.clone();
                move |event: web_sys::Event| {
                    if focused(&app) {
                        event.prevent_default();
                    }
                }
            });
            let mac = window
                .navigator()
                .platform()
                .is_ok_and(|platform| platform.starts_with("Mac"));
            let on_keydown = Closure::new(move |event: KeyboardEvent| {
                if !focused(&keyboard_app) {
                    return;
                }
                let letter = chord_letter(&event);
                let clipboard = matches!(letter, Some('c' | 'x' | 'v'));
                if event.meta_key() {
                    // Not `prevent_default`: the browser still has to fire
                    // its copy, cut, or paste event.
                    event.stop_propagation();
                    if letter == Some('a') {
                        event.prevent_default();
                    }
                } else if !mac
                    && (clipboard && event.ctrl_key() && !event.alt_key()
                        || is_shift_delete(&event))
                {
                    event.stop_propagation();
                }
            });
            let listen = |kinds: [&str; 3], callback: &JsValue| {
                kinds.into_iter().try_for_each(|kind| {
                    document
                        .add_event_listener_with_callback(kind, callback.unchecked_ref())
                        .map_err(|error| io::Error::other(format!("{kind} listener: {error:?}")))
                })
            };
            listen(EVENTS, on_clipboard.as_ref())?;
            listen(BEFORE, on_before.as_ref())?;
            // The capture phase on the window runs before ratzilla's
            // listener on the canvas, which then never sees the key.
            window
                .add_event_listener_with_callback_and_bool(
                    "keydown",
                    on_keydown.as_ref().unchecked_ref(),
                    true,
                )
                .map_err(|error| io::Error::other(format!("keydown listener: {error:?}")))?;
            Ok(Self {
                window,
                document,
                on_clipboard,
                on_before,
                on_keydown,
            })
        }

        /// Put `text` on the clipboard from outside a copy or cut event, with
        /// `navigator.clipboard.writeText`. The browser allows it while
        /// handling a user's key press or click, on a secure page (https, or
        /// localhost), and in a cross-origin iframe only with
        /// `allow="clipboard-write"`; otherwise the write fails silently.
        pub fn write(text: &str) {
            let Some(window) = web_sys::window() else {
                return;
            };
            let clipboard = window.navigator().clipboard();
            // Missing on an insecure page, where calling it would throw.
            if JsValue::from(&clipboard).is_undefined() {
                return;
            }
            // A refused write rejects the promise: wait it out, so the
            // rejection is handled and the failure stays silent.
            let written = JsFuture::from(clipboard.write_text(text));
            wasm_bindgen_futures::spawn_local(async move {
                let _ = written.await;
            });
        }
    }

    /// Whether focus is on `app` or inside it, which makes an event the
    /// app's.
    fn focused(app: &Element) -> bool {
        app.matches(":focus-within").unwrap_or(false)
    }

    /// Whether text inside `app` is selected: the app's own DOM, for a
    /// backend that draws into it.
    fn selected_within(app: &Element) -> bool {
        web_sys::window()
            .and_then(|window| window.get_selection().ok().flatten())
            .filter(|selection| !selection.is_collapsed())
            .and_then(|selection| selection.anchor_node())
            .is_some_and(|node| app.contains(Some(&node)))
    }

    /// Shift+Delete, which a browser off a Mac reads as cut; ratzilla would
    /// delete the selection without copying it.
    fn is_shift_delete(event: &KeyboardEvent) -> bool {
        event.key() == "Delete" && event.shift_key() && !event.ctrl_key() && !event.alt_key()
    }

    /// The letter a chord was typed with, lowercased: the key's own when it is
    /// an ASCII letter, and the physical key's on a layout that types another
    /// script, where Ctrl+С is still copy. Not the physical key alone: on
    /// Dvorak the key in C's place types `j`.
    fn chord_letter(event: &KeyboardEvent) -> Option<char> {
        let key = event.key();
        let mut chars = key.chars();
        match (chars.next(), chars.next()) {
            (Some(char), None) if char.is_ascii_alphabetic() => Some(char.to_ascii_lowercase()),
            _ if !key.is_ascii() => event
                .code()
                .strip_prefix("Key")
                .and_then(|letter| letter.chars().next())
                .map(|char| char.to_ascii_lowercase()),
            _ => None,
        }
    }

    impl Drop for BrowserClipboard {
        fn drop(&mut self) {
            for (kinds, callback) in [
                (EVENTS, self.on_clipboard.as_ref()),
                (BEFORE, self.on_before.as_ref()),
            ] {
                for kind in kinds {
                    let _ = self
                        .document
                        .remove_event_listener_with_callback(kind, callback.unchecked_ref());
                }
            }
            let _ = self.window.remove_event_listener_with_callback_and_bool(
                "keydown",
                self.on_keydown.as_ref().unchecked_ref(),
                true,
            );
        }
    }

    /// The input Ratzilla does not deliver, wired to the runtime for as long as
    /// the guard lives:
    ///
    /// - A key that types one non-ASCII character, `ä` or `ж`, arrives as an
    ///   [`Event::Key`] for it, with its modifiers. Ratzilla 0.3.1 reports such
    ///   a key as unidentified, so without this a text field could not type
    ///   it. A character composed with <kbd>Option</kbd> on a Mac still
    ///   arrives with Alt held, and one composed with `AltGr` on
    ///   Windows with Ctrl+Alt held. A text field types Ctrl+Alt with any
    ///   character but an ASCII letter or digit, but takes Alt with a
    ///   character as a chord: typing Option compositions needs the page, or
    ///   the app's own key capture, to forward them without the modifiers, as
    ///   the demos' keyboard script does.
    /// - The mouse wheel arrives as [`MouseKind::Scroll`] at the cell under
    ///   the pointer, once a notch (100 pixels, or three lines), however
    ///   finely a trackpad slices the travel. Only Ratzilla knows the cell,
    ///   so route its mouse events through
    ///   [`mouse_handler`](Self::mouse_handler): before scrolling, the
    ///   listener sends the grid a `mousemove` where the wheel turned, and
    ///   Ratzilla reports the cell through it. A turn's first event is a
    ///   whole step, so the app decides at once whether the turn is its; a
    ///   turn the app takes does not also scroll the page. A turn ends when
    ///   the wheel rests for a quarter second. <kbd>Ctrl</kbd>+wheel is left
    ///   to the browser, which zooms.
    ///
    /// Install one per app, given its element as for [`BrowserClipboard`].
    #[must_use = "dropping the guard removes its browser listeners"]
    pub struct BrowserInput {
        app: Element,
        route: Rc<RefCell<dyn FnMut(Event) -> bool>>,
        wheel: Rc<RefCell<Wheel>>,
        on_key: Closure<dyn FnMut(KeyboardEvent)>,
        on_wheel: Closure<dyn FnMut(WheelEvent)>,
    }

    impl std::fmt::Debug for BrowserInput {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("BrowserInput").finish_non_exhaustive()
        }
    }

    impl BrowserInput {
        /// Install the listeners for the app drawn in `app`: the canvas a
        /// canvas backend draws on, or the element holding it; for a DOM
        /// backend, the element whose id was given as its `grid_id`.
        ///
        /// `on_event` routes one event and reports whether the app took it,
        /// as for [`BrowserClipboard::install`]. Write what the app put on
        /// the clipboard from inside it, as after any other event.
        ///
        /// # Errors
        ///
        /// Returns an I/O error if `app` refuses a listener.
        pub fn install(
            app: &Element,
            on_event: impl FnMut(Event) -> bool + 'static,
        ) -> io::Result<Self> {
            let route: Rc<RefCell<dyn FnMut(Event) -> bool>> = Rc::new(RefCell::new(on_event));
            let wheel = Rc::new(RefCell::new(Wheel::default()));
            // On the way up from the canvas, after Ratzilla's listener has
            // dropped the key, and after a page script that forwards keys to
            // the canvas, as the demos' does, has had its say.
            let on_key = Closure::new({
                let route = Rc::clone(&route);
                move |event: KeyboardEvent| {
                    let modifiers = Modifiers {
                        ctrl: event.ctrl_key(),
                        alt: event.alt_key(),
                        shift: event.shift_key(),
                    };
                    if let Some(key) = non_ascii_key(&event.key(), modifiers)
                        && send(&route, Event::Key(key))
                    {
                        event.prevent_default();
                    }
                }
            });
            let on_wheel = Closure::new({
                let route = Rc::clone(&route);
                let wheel = Rc::clone(&wheel);
                move |event: WheelEvent| scroll(&route, &wheel, &event)
            });
            app.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref())
                .map_err(|error| io::Error::other(format!("keydown listener: {error:?}")))?;
            // A passive wheel listener could not keep the page from scrolling.
            let options = AddEventListenerOptions::new();
            options.set_passive(false);
            app.add_event_listener_with_callback_and_add_event_listener_options(
                "wheel",
                on_wheel.as_ref().unchecked_ref(),
                &options,
            )
            .map_err(|error| io::Error::other(format!("wheel listener: {error:?}")))?;
            Ok(Self {
                app: app.clone(),
                route,
                wheel,
                on_key,
                on_wheel,
            })
        }

        /// A callback for Ratzilla's `on_mouse_event` that routes each mouse
        /// event through `on_event`, and remembers the pointer's cell for the
        /// wheel. It is Ratzilla's to keep: it routes for as long as the
        /// backend holds it, guard or not.
        pub fn mouse_handler(&self) -> impl FnMut(ratzilla::event::MouseEvent) + 'static {
            let route = Rc::clone(&self.route);
            let wheel = Rc::clone(&self.wheel);
            move |mouse| {
                let Ok(mouse) = MouseEvent::try_from(mouse) else {
                    return;
                };
                wheel.borrow_mut().point(match mouse.kind {
                    MouseKind::Exited => None,
                    _ => Some((mouse.column, mouse.row)),
                });
                let _ = send(&route, Event::Mouse(mouse));
            }
        }
    }

    /// Route `event`, unless the route is already busy routing another, which
    /// only an app dispatching browser events from its handler would cause.
    fn send(route: &RefCell<dyn FnMut(Event) -> bool>, event: Event) -> bool {
        route.try_borrow_mut().is_ok_and(|mut route| route(event))
    }

    /// Turn one wheel event into scroll events at the pointer's cell.
    fn scroll(
        route: &RefCell<dyn FnMut(Event) -> bool>,
        wheel: &RefCell<Wheel>,
        event: &WheelEvent,
    ) {
        // Ctrl+wheel zooms, and a horizontal wheel is left to the page.
        if event.ctrl_key() || event.delta_y() == 0.0 {
            return;
        }
        // Ratzilla reports the pointer's cell only as the mouse moves, so a
        // wheel before any move, or after the page scrolled the grid under a
        // still pointer, would scroll at no cell or a stale one. A move where
        // the wheel turned has Ratzilla report the cell under it now.
        if let Some(target) = event.target() {
            let init = MouseEventInit::new();
            init.set_bubbles(true);
            init.set_client_x(event.client_x());
            init.set_client_y(event.client_y());
            if let Ok(moved) =
                web_sys::MouseEvent::new_with_mouse_event_init_dict("mousemove", &init)
            {
                let _ = target.dispatch_event(&moved);
            }
        }
        let Some((column, row)) = wheel.borrow().cell() else {
            return;
        };
        let notches = match event.delta_mode() {
            WheelEvent::DOM_DELTA_PIXEL => event.delta_y() / 100.0,
            WheelEvent::DOM_DELTA_LINE => event.delta_y() / 3.0,
            _ => event.delta_y(),
        };
        let steps = wheel.borrow_mut().turn(notches, event.time_stamp());
        let direction = if steps < 0 {
            ScrollDirection::Up
        } else {
            ScrollDirection::Down
        };
        let mut took_any = false;
        for _ in 0..steps.unsigned_abs() {
            let took = send(
                route,
                Event::Mouse(MouseEvent {
                    kind: MouseKind::Scroll(direction),
                    column,
                    row,
                    modifiers: Modifiers {
                        ctrl: false,
                        alt: event.alt_key(),
                        shift: event.shift_key(),
                    },
                }),
            );
            wheel.borrow_mut().took(took);
            took_any |= took;
        }
        if took_any || wheel.borrow().taking() {
            event.prevent_default();
        }
    }

    impl Drop for BrowserInput {
        fn drop(&mut self) {
            let _ = self.app.remove_event_listener_with_callback(
                "keydown",
                self.on_key.as_ref().unchecked_ref(),
            );
            let _ = self.app.remove_event_listener_with_callback(
                "wheel",
                self.on_wheel.as_ref().unchecked_ref(),
            );
        }
    }

    /// Show `shape` as the mouse pointer over `app`, the element the app is
    /// drawn in: the canvas a canvas backend draws on, or the element holding
    /// it; for a `DomBackend`, the element whose id was given as its
    /// `grid_id`. A canvas found by id is an `Element`; `dyn_into` makes it
    /// the `HtmlElement` this takes.
    ///
    /// It sets the element's CSS `cursor`, and only when that changes, so hand
    /// it [`Ratcn::pointer_shape`](super::super::Ratcn::pointer_shape) after
    /// every frame. [`PointerShape::Default`] is the arrow (`cursor:
    /// default`), not the browser's automatic cursor, which over a
    /// `DomBackend`'s text would be a text beam. Nothing is installed: unlike
    /// [`BrowserClipboard`], there is no guard to keep.
    pub fn set_browser_pointer_shape(app: &HtmlElement, shape: PointerShape) {
        let style = app.style();
        let wanted = shape.css_name();
        if style.get_property_value("cursor").ok().as_deref() == Some(wanted) {
            return;
        }
        // Only a read-only declaration refuses, and an element's own style is
        // not one.
        let _ = style.set_property("cursor", wanted);
    }
}

#[cfg(all(target_arch = "wasm32", feature = "ratzilla"))]
pub use browser_clipboard::{BrowserClipboard, BrowserInput, set_browser_pointer_shape};

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

    /// The key a browser `keydown` named `key` types when Ratzilla cannot say:
    /// one non-ASCII character, `ä` or `€`. Ratzilla 0.3.1 takes a key name
    /// as a character only when it is one byte long, and reports the rest as
    /// unidentified. An ASCII character is Ratzilla's to convert, so it is
    /// left alone here, or it would arrive twice.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        allow(dead_code, reason = "BrowserInput is wasm-only; tested natively")
    )]
    pub(super) fn non_ascii_key(key: &str, modifiers: Modifiers) -> Option<KeyEvent> {
        let mut chars = key.chars();
        match (chars.next(), chars.next()) {
            (Some(char), None) if !char.is_ascii() => Some(KeyEvent {
                code: KeyCode::Char(char),
                modifiers,
            }),
            _ => None,
        }
    }

    /// How long the wheel rests before its next event starts a new turn,
    /// in milliseconds. A flick's events, and a trackpad's, follow each other
    /// a frame or so apart, even through a trackpad's coasting, so a quarter
    /// second without one means the hand let go.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        allow(dead_code, reason = "BrowserInput is wasm-only; tested natively")
    )]
    const TURN_GAP_MS: f64 = 250.0;

    /// The browser wheel's state between events: where the pointer is, when
    /// the wheel last moved, the travel short of a notch, and whether the app
    /// took the last step of the turn.
    #[derive(Debug, Default)]
    #[cfg_attr(
        not(target_arch = "wasm32"),
        allow(dead_code, reason = "BrowserInput is wasm-only; tested natively")
    )]
    pub(super) struct Wheel {
        cell: Option<(u16, u16)>,
        last: f64,
        pending: f64,
        /// `None` until the turn's first step has been routed.
        taken: Option<bool>,
    }

    #[cfg_attr(
        not(target_arch = "wasm32"),
        allow(dead_code, reason = "BrowserInput is wasm-only; tested natively")
    )]
    impl Wheel {
        /// The pointer is over `cell`, or off the grid. Over another cell,
        /// the turn starts afresh.
        pub(super) fn point(&mut self, cell: Option<(u16, u16)>) {
            if cell != self.cell {
                *self = Self {
                    cell,
                    ..Self::default()
                };
            }
        }

        pub(super) const fn cell(&self) -> Option<(u16, u16)> {
            self.cell
        }

        /// Add `notches` of travel at `now` (milliseconds), negative upward,
        /// and say how many whole steps it makes: one a notch, as a terminal
        /// reports a wheel, however finely a trackpad slices the travel.
        ///
        /// A turn's first slice is a whole step however small, so that the
        /// app decides at once whether the turn is its: a browser may stop a
        /// trackpad's events being cancelable once their first one went to
        /// the page. The step's overshoot is forgiven rather than owed, or
        /// the first slice back the other way would step straight back.
        pub(super) fn turn(&mut self, notches: f64, now: f64) -> i32 {
            if notches == 0.0 {
                return 0;
            }
            if now - self.last >= TURN_GAP_MS {
                self.pending = 0.0;
                self.taken = None;
            }
            self.last = now;
            let travel = self.pending + notches;
            let mut steps = travel.trunc();
            if steps == 0.0 && self.taken.is_none() {
                steps = notches.signum();
                self.pending = 0.0;
            } else {
                self.pending = travel - steps;
            }
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a wheel event's notches are few"
            )]
            let steps = steps as i32;
            steps
        }

        /// Whether the app took the step just routed.
        pub(super) const fn took(&mut self, taken: bool) {
            self.taken = Some(taken);
        }

        /// Whether the app is taking this turn: the last step of it was the
        /// app's, so travel short of a step is the app's too, not the page's.
        pub(super) fn taking(&self) -> bool {
            self.taken == Some(true)
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
    fn termina_mouse_cells_are_not_shifted() {
        let event = MouseEvent::from(tn::MouseEvent {
            kind: tn::MouseEventKind::Drag(tn::MouseButton::Left),
            column: 3,
            row: 4,
            modifiers: tn::Modifiers::CONTROL,
        });

        assert_eq!(
            event,
            MouseEvent {
                kind: MouseKind::Drag(MouseButton::Left),
                column: 3,
                row: 4,
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::NONE
                },
            }
        );
    }

    #[test]
    fn termina_presses_and_releases_keep_their_button() {
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
    fn termina_scroll_axes_map_to_their_directions() {
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
    use super::ratzilla_conv::{Wheel, non_ascii_key};
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

    /// Ratzilla drops every key whose name is longer than one byte, so the
    /// browser listener converts those itself. Counting bytes, as Ratzilla
    /// does, would drop `ä` again; taking ASCII too would type it twice.
    #[test]
    fn a_browser_key_typing_one_non_ascii_character_converts() {
        let alt = Modifiers {
            alt: true,
            ..Modifiers::NONE
        };
        for key in ["ä", "é", "€", "ж", "字", "😀"] {
            let char = key.chars().next().unwrap();
            assert_eq!(
                non_ascii_key(key, alt),
                Some(KeyEvent {
                    code: KeyCode::Char(char),
                    modifiers: alt,
                }),
                "{key:?}"
            );
        }
        for key in ["a", "@", "{", "Enter", "Dead", "Unidentified", "ä̈", ""] {
            assert_eq!(non_ascii_key(key, Modifiers::NONE), None, "{key:?}");
        }
    }

    /// A wheel's events at `ms` apart from `start`, each `notches` long:
    /// the steps each one makes.
    fn turn(wheel: &mut Wheel, start: f64, ms: f64, slices: &[f64]) -> Vec<i32> {
        let mut now = start;
        slices
            .iter()
            .map(|&notches| {
                now += ms;
                wheel.turn(notches, now)
            })
            .collect()
    }

    /// A mouse wheel scrolls once a notch, and a trackpad sends a stream of
    /// small deltas that must add up to the same rate: scrolling once per
    /// event, or dropping the remainder, would make a trackpad far too fast
    /// or never scroll at all.
    #[test]
    fn wheel_travel_scrolls_once_a_notch() {
        let mut wheel = Wheel::default();
        wheel.point(Some((3, 4)));
        assert_eq!(turn(&mut wheel, 0.0, 20.0, &[1.0, -2.0]), [1, -2]);
        wheel.took(true);

        let steps = turn(&mut wheel, 100.0, 10.0, &[0.125; 25]);
        assert_eq!(steps.iter().sum::<i32>(), 3, "three notches and a bit");
        assert_eq!(steps.iter().filter(|&&step| step != 0).count(), 3);
    }

    /// Most of a trackpad's events are short of a step. While the app takes
    /// the steps of a turn, the travel between them is the app's too, or the
    /// page around a demo would scroll along with the field. Once a step is
    /// ignored, or the pointer moves to another cell, the page gets it back.
    #[test]
    fn a_turn_the_app_takes_keeps_the_page_still_between_steps() {
        let mut wheel = Wheel::default();
        wheel.point(Some((3, 4)));
        assert_eq!(wheel.turn(1.0, 10.0), 1);
        wheel.took(true);
        assert_eq!(wheel.turn(0.25, 20.0), 0);
        assert!(wheel.taking(), "a slice between steps is the app's");

        wheel.point(Some((3, 4)));
        assert!(wheel.taking(), "the same cell keeps the turn");
        wheel.point(Some((3, 5)));
        assert!(!wheel.taking(), "another cell starts afresh");

        assert_eq!(wheel.turn(0.25, 30.0), 1);
        wheel.took(false);
        assert!(!wheel.taking(), "an ignored step hands the page back");
        assert_eq!(wheel.turn(0.25, 40.0), 0, "and slices after it stay slices");
    }

    /// A turn's first slice decides whose the turn is, at once: left short of
    /// a step, the first slices of a trackpad turn would go to the page, and
    /// a browser may then stop the rest being cancelable. That holds for a
    /// second flick on the same cell too, once the wheel has rested, even if
    /// the app ignored the first.
    #[test]
    fn every_turn_starts_with_a_whole_step() {
        let mut wheel = Wheel::default();
        wheel.point(Some((3, 4)));
        assert_eq!(wheel.turn(-0.25, 10.0), -1, "the first slice is a step");
        wheel.took(false);
        let rest = turn(&mut wheel, 10.0, 10.0, &[-0.25; 4]);
        assert_eq!(rest, [0, 0, 0, -1], "ignored, the turn goes by notches");
        wheel.took(false);

        assert_eq!(wheel.turn(-0.25, 500.0), -1, "a flick after a rest");
    }

    /// A trackpad held still jitters back and forth by a hair. Within a turn
    /// that must scroll nothing: forcing a step on each reversal, or owing
    /// the first step's overshoot, would flicker a list up and down.
    #[test]
    fn jitter_within_a_turn_scrolls_nothing() {
        let mut wheel = Wheel::default();
        wheel.point(Some((3, 4)));
        assert_eq!(wheel.turn(0.1, 10.0), 1, "the turn's first slice");
        wheel.took(true);
        let jitter = turn(&mut wheel, 10.0, 10.0, &[-0.1, 0.1, -0.1, 0.1, -0.1, 0.1]);
        assert_eq!(jitter, [0; 6]);
        assert!(wheel.taking(), "and the turn stays the app's");
    }
}
