//! Turning on the terminal input modes ratcn's mouse and paste handling need.
//!
//! A terminal does not report mouse movement or paste as a unit unless asked.
//! [`InputModes`] asks, and hands back a guard that turns the modes off again
//! when dropped — leaving them on would leave the user's shell in a strange
//! state after the app exits.
//!
//! Nothing is enabled implicitly. Ratcn will not change the user's terminal
//! behind their back, so an app that wants mouse support opts in:
//!
//! ```no_run
//! use ratcn::crossterm::InputModes;
//!
//! # fn main() -> std::io::Result<()> {
//! let _modes = InputModes::new()
//!     .mouse_capture()
//!     .bracketed_paste()
//!     .enable()?;
//! // Run the event loop; `_modes` must stay alive for the whole of it.
//! # Ok(())
//! # }
//! ```

use std::{
    io,
    sync::{Condvar, Mutex, OnceLock},
};

use ratatui::crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
};

#[derive(Debug, Clone, Copy)]
enum Mode {
    MouseCapture,
    BracketedPaste,
}

#[derive(Debug, Default)]
struct ModeState {
    leases: usize,
    transitioning: bool,
}

#[derive(Debug, Default)]
struct ModeStates {
    mouse_capture: ModeState,
    bracketed_paste: ModeState,
}

impl ModeStates {
    const fn get_mut(&mut self, mode: Mode) -> &mut ModeState {
        match mode {
            Mode::MouseCapture => &mut self.mouse_capture,
            Mode::BracketedPaste => &mut self.bracketed_paste,
        }
    }
}

#[derive(Debug, Default)]
struct ModeRegistry {
    states: Mutex<ModeStates>,
    changed: Condvar,
}

impl ModeRegistry {
    fn acquire(&self, mode: Mode, enable: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
        let mut states = self.lock();
        loop {
            let state = states.get_mut(mode);
            if state.transitioning {
                states = self
                    .changed
                    .wait(states)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            } else if state.leases > 0 {
                state.leases = state.leases.checked_add(1).ok_or_else(|| {
                    io::Error::other("terminal input mode reference count overflow")
                })?;
                return Ok(());
            } else {
                state.transitioning = true;
                break;
            }
        }
        drop(states);

        let result = enable();
        let mut states = self.lock();
        let state = states.get_mut(mode);
        state.transitioning = false;
        if result.is_ok() {
            state.leases = 1;
        }
        self.changed.notify_all();
        result
    }

    fn release(&self, mode: Mode, disable: impl FnOnce() -> io::Result<()>) {
        let mut states = self.lock();
        loop {
            let state = states.get_mut(mode);
            if state.transitioning {
                states = self
                    .changed
                    .wait(states)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            } else if state.leases > 1 {
                state.leases -= 1;
                return;
            } else if state.leases == 1 {
                state.transitioning = true;
                break;
            } else {
                return;
            }
        }
        drop(states);

        let _ = disable();
        let mut states = self.lock();
        let state = states.get_mut(mode);
        state.leases = 0;
        state.transitioning = false;
        self.changed.notify_all();
    }

    #[cfg(test)]
    fn leases(&self, mode: Mode) -> usize {
        self.lock().get_mut(mode).leases
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ModeStates> {
        self.states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn mode_registry() -> &'static ModeRegistry {
    static REGISTRY: OnceLock<ModeRegistry> = OnceLock::new();
    REGISTRY.get_or_init(ModeRegistry::default)
}

/// Which optional terminal input modes to switch on.
///
/// Build one, call [`enable`](Self::enable), and keep the returned
/// [`InputModeGuard`] alive for as long as the app reads terminal events —
/// dropping it switches the modes back off.
/// Multiple guards created through this API compose: a mode stays enabled until
/// the last guard that requested it is dropped.
///
/// Binding it to `_` rather than `_guard` is the classic mistake: `let _ =
/// modes.enable()?;` drops the guard immediately and the modes never take
/// effect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub struct InputModes {
    mouse_capture: bool,
    bracketed_paste: bool,
}

impl InputModes {
    /// Start with no optional terminal input modes enabled.
    pub const fn new() -> Self {
        Self {
            mouse_capture: false,
            bracketed_paste: false,
        }
    }

    /// Report mouse movement, clicks, and scrolling as events.
    ///
    /// Required for any of ratcn's mouse handling — hover, clicking a button,
    /// dragging a dialog. Without it the terminal handles the mouse itself and
    /// no mouse events reach the app.
    ///
    /// The trade-off: while it is on, the terminal's own text selection usually
    /// stops working, and users fall back to holding Shift to select.
    pub const fn mouse_capture(mut self) -> Self {
        self.mouse_capture = true;
        self
    }

    /// Report a paste as one event rather than as fake typing.
    ///
    /// Without it the terminal delivers pasted text as a burst of individual key
    /// presses, so a pasted newline looks exactly like the user hitting Enter —
    /// which submits the form. With it, the paste arrives as a single
    /// [`Event::Paste`](crate::runtime::Event::Paste) that components insert
    /// verbatim.
    pub const fn bracketed_paste(mut self) -> Self {
        self.bracketed_paste = true;
        self
    }

    /// Switch the selected modes on and return the guard that switches them off.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the terminal will not accept one of the modes.
    /// Anything already enabled is switched back off first, so a failed call
    /// leaves the terminal as it found it.
    pub fn enable(self) -> io::Result<InputModeGuard> {
        let mut guard = InputModeGuard::default();
        if self.mouse_capture {
            mode_registry().acquire(Mode::MouseCapture, || {
                execute!(io::stdout(), EnableMouseCapture)
            })?;
            guard.mouse_capture = true;
        }
        if self.bracketed_paste {
            mode_registry().acquire(Mode::BracketedPaste, || {
                execute!(io::stdout(), EnableBracketedPaste)
            })?;
            guard.bracketed_paste = true;
        }
        Ok(guard)
    }
}

/// Keeps [`InputModes`]' modes on, and switches them off when dropped.
///
/// Hold it for the lifetime of the event loop. Errors while restoring are
/// ignored, since `Drop` cannot report them and the process is usually exiting
/// anyway.
#[derive(Debug, Default)]
#[must_use = "dropping the guard restores the selected crossterm input modes"]
pub struct InputModeGuard {
    mouse_capture: bool,
    bracketed_paste: bool,
}

impl Drop for InputModeGuard {
    fn drop(&mut self) {
        if self.bracketed_paste {
            mode_registry().release(Mode::BracketedPaste, || {
                execute!(io::stdout(), DisableBracketedPaste)
            });
        }
        if self.mouse_capture {
            mode_registry().release(Mode::MouseCapture, || {
                execute!(io::stdout(), DisableMouseCapture)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{InputModes, Mode, ModeRegistry};

    #[test]
    fn mode_builders_select_only_the_requested_modes() {
        assert_eq!(InputModes::new(), InputModes::default());
        assert!(InputModes::new().mouse_capture().mouse_capture);
        assert!(InputModes::new().bracketed_paste().bracketed_paste);
        let both = InputModes::new().mouse_capture().bracketed_paste();
        assert!(both.mouse_capture && both.bracketed_paste);
    }

    #[test]
    fn overlapping_mode_leases_enable_once_and_disable_after_the_last_release() {
        let enables = Cell::new(0);
        let disables = Cell::new(0);
        let registry = ModeRegistry::default();

        registry
            .acquire(Mode::MouseCapture, || {
                enables.set(enables.get() + 1);
                Ok(())
            })
            .expect("first lease");
        registry
            .acquire(Mode::MouseCapture, || {
                enables.set(enables.get() + 1);
                Ok(())
            })
            .expect("second lease");
        registry.release(Mode::MouseCapture, || {
            disables.set(disables.get() + 1);
            Ok(())
        });

        assert_eq!(
            (
                registry.leases(Mode::MouseCapture),
                enables.get(),
                disables.get()
            ),
            (1, 1, 0)
        );

        registry.release(Mode::MouseCapture, || {
            disables.set(disables.get() + 1);
            Ok(())
        });
        assert_eq!(
            (
                registry.leases(Mode::MouseCapture),
                enables.get(),
                disables.get()
            ),
            (0, 1, 1)
        );
    }
}
