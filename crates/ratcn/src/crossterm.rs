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
//!     .mouse()
//!     .paste()
//!     .enable()?;
//! // Run the event loop; `_modes` must stay alive for the whole of it.
//! # Ok(())
//! # }
//! ```

use std::io;

use ratatui::crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
};

/// Which optional terminal input modes to switch on.
///
/// Build one, call [`enable`](Self::enable), and keep the returned
/// [`InputModeGuard`] alive for as long as the app reads terminal events —
/// dropping it switches the modes back off.
///
/// Binding it to `_` rather than `_guard` is the classic mistake: `let _ =
/// modes.enable()?;` drops the guard immediately and the modes never take
/// effect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub struct InputModes {
    mouse: bool,
    paste: bool,
}

impl InputModes {
    /// Start with no optional terminal input modes enabled.
    pub const fn new() -> Self {
        Self {
            mouse: false,
            paste: false,
        }
    }

    /// Report mouse movement, clicks, and scrolling as events.
    ///
    /// Every part of ratcn's mouse handling needs it — hover, clicking a
    /// button, dragging a dialog. While it is on, the terminal's own text
    /// selection usually stops working, and users hold Shift to select.
    pub const fn mouse(mut self) -> Self {
        self.mouse = true;
        self
    }

    /// Deliver a paste as one [`Event::Paste`](crate::runtime::Event::Paste)
    /// that components insert verbatim, so a pasted newline stays distinct
    /// from the user hitting Enter.
    pub const fn paste(mut self) -> Self {
        self.paste = true;
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
        if self.mouse {
            execute!(io::stdout(), EnableMouseCapture)?;
            guard.mouse_capture = true;
        }
        if self.paste {
            execute!(io::stdout(), EnableBracketedPaste)?;
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
            let _ = execute!(io::stdout(), DisableBracketedPaste);
        }
        if self.mouse_capture {
            let _ = execute!(io::stdout(), DisableMouseCapture);
        }
    }
}
