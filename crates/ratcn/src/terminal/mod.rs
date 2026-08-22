//! Running a ratatui app on a terminal that can be asked about itself.
//!
//! [`Session`] opens the terminal — raw mode, the alternate screen, the input
//! modes an app asked for — and puts every one of them back on the way out, on
//! a `?`, a quit, or a panic.
//!
//! # Choosing a theme
//!
//! Paint with a preset:
//!
//! ```no_run
//! use ratcn::{Theme, terminal::{Session, SessionOptions}};
//!
//! # fn main() -> std::io::Result<()> {
//! let mut session = Session::open(SessionOptions::new().mouse())?;
//! let theme = Theme::gruvbox();
//!
//! loop {
//!     session.terminal_mut().draw(|frame| {
//!         let _paint_with = theme;
//!     })?;
//!     let _event = session.next(None)?;
//!     # break;
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Or paint with the terminal's own colors. [`SessionOptions::adaptive`] asks
//! the terminal what it looks like and follows it as the user changes it: it
//! asks again when the window regains focus, shortly after every change signal,
//! and when input resumes after a pause. Read
//! [`theme_with_fallback`](Session::theme_with_fallback) each frame and paint
//! from what it says.
//!
//! ```no_run
//! use ratcn::{Theme, terminal::{Session, SessionEvent, SessionOptions}};
//!
//! # fn main() -> std::io::Result<()> {
//! let mut session = Session::open(SessionOptions::new().mouse().adaptive())?;
//!
//! loop {
//!     let theme = session.theme_with_fallback(Theme::gruvbox());
//!     session.terminal_mut().draw(|frame| {
//!         let _paint_with = theme;
//!     })?;
//!
//!     match session.next(None)? {
//!         Some(SessionEvent::Input(event)) => {
//!             // Route it: quit keys first, then ratcn's components.
//!             let _ = event;
//!         }
//!         Some(SessionEvent::ThemeChanged(theme)) => {
//!             // React to the change — persist it, animate it.
//!             let _ = theme;
//!         }
//!         None => {}
//!     }
//!     # break;
//! }
//! # Ok(())
//! # }
//! ```

#[cfg(test)]
mod fake;
mod query;
mod watch;

use std::{
    io,
    time::{Duration, Instant},
};

use ratatui::Terminal;
use ratatui_termina::TerminaBackend;
use termina::{
    EventReader, PlatformTerminal, Terminal as _,
    escape::csi::{Csi, DecPrivateMode, DecPrivateModeCode, Mode},
};

use crate::Theme;
use watch::Watch;

use query::{TerminalColors, query};
/// The terminal library this module is built on, re-exported so an integrator
/// names the types inside [`SessionEvent::Input`] through ratcn, at the version
/// ratcn builds against.
pub use termina;

/// The backend a [`Session`] draws through.
pub type SessionBackend = TerminaBackend<PlatformTerminal>;

/// What came out of a [`Session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// Something the user did. Convert it with
    /// [`Event::try_from`](crate::runtime::Event) to route it at components.
    Input(termina::Event),
    /// The terminal re-themed, and this is the theme to paint with now.
    ///
    /// A session opened with [`SessionOptions::adaptive`] reports this each
    /// time the theme changes. Match it to react to a change — persist it,
    /// animate it; [`Session::theme`] already carries it.
    ThemeChanged(Theme),
}

/// The terminal an app runs on, and the modes it switched on to get there.
///
/// Dropping it switches them back off, on every path out — a `?`, a quit, or an
/// unwinding panic.
#[derive(Debug)]
pub struct Session {
    terminal: Terminal<SessionBackend>,
    /// Taken before the terminal was wrapped: the backend keeps the handle to
    /// itself behind an unstable feature.
    events: EventReader,
    /// Present where the terminal answered the startup query, which is where
    /// mode 2031 is switched on for it.
    watch: Option<Watch>,
    /// What the terminal last said it looks like, once it has said anything.
    theme: Option<Theme>,
    modes: Vec<DecPrivateModeCode>,
}

/// What an app wants of its [`Session`]. Each builder switches on the terminal
/// mode it names.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct SessionOptions {
    mouse: bool,
    paste: bool,
    adaptive: bool,
}

impl SessionOptions {
    /// The alternate screen, and nothing else.
    pub const fn new() -> Self {
        Self {
            mouse: false,
            paste: false,
            adaptive: false,
        }
    }

    /// Report mouse movement, clicks, and scrolling as events. Required for
    /// any of ratcn's mouse handling.
    pub const fn mouse(mut self) -> Self {
        self.mouse = true;
        self
    }

    /// Deliver pasted text as a single [`SessionEvent::Input`] carrying the
    /// whole paste.
    pub const fn paste(mut self) -> Self {
        self.paste = true;
        self
    }

    /// Paint with the terminal's own colors, and follow them as they change.
    ///
    /// The session asks the terminal what it looks like while opening,
    /// subscribes to changes it reports, and asks again when the window regains
    /// focus, shortly after every change signal, and when input resumes after a
    /// pause. [`Session::theme`] answers with what it said.
    pub const fn adaptive(mut self) -> Self {
        self.adaptive = true;
        self
    }
}

impl Session {
    /// Open the terminal and switch on the modes `options` asked for.
    ///
    /// Under [`SessionOptions::adaptive`] the terminal is asked what colors it
    /// uses while the session opens, subscribed to for changes it reports, and
    /// asked again when the window regains focus, shortly after every change
    /// signal, and when input resumes after a pause; [`theme`](Self::theme)
    /// answers with what it said.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the terminal cannot be opened, cannot be put
    /// into raw mode, cannot be measured, or will not accept the modes or the
    /// query. A terminal that stays silent leaves [`theme`](Self::theme) at its
    /// fallback.
    pub fn open(options: SessionOptions) -> io::Result<Self> {
        // The wiring in this function needs a real terminal, so no test in the
        // suite reaches it; it is exercised by hand against a pty.
        let mut output = PlatformTerminal::new()?;
        // Raw mode first: nothing below reads a reply that the line discipline
        // would otherwise hold back until Enter.
        output.enter_raw_mode()?;

        // The query's one window: raw mode is on and no event reader has taken
        // a byte yet, so the terminal's answers are the only thing in the
        // stream that is not the user typing. It runs before the alternate
        // screen, so a slow terminal is answered against the shell the user can
        // still see.
        let answer = if options.adaptive {
            query(&mut output)?
        } else {
            None
        };
        let (theme, watch) = opening(answer, Instant::now());

        let modes = modes(options, watch.is_some());
        // Termina restores raw mode after this hook runs, so the hook owes only
        // the modes below. It writes through a handle of its own, so it works
        // while the session is being unwound.
        output.set_panic_hook({
            let modes = modes.clone();
            move |handle| {
                let _ = restore_modes(handle, &modes);
            }
        });

        // `Terminal::new` measures the grid and can fail. The modes are still
        // off here, so a failure has nothing to restore.
        let events = output.event_reader();
        let session = Self {
            terminal: Terminal::new(TerminaBackend::new(output))?,
            events,
            watch,
            theme,
            modes,
        };
        session.enable()
    }

    /// What the terminal says it looks like, or [`Theme::default_dark`].
    ///
    /// Read it each frame and paint from what it says: under
    /// [`SessionOptions::adaptive`] it changes as the user changes their
    /// terminal.
    #[must_use]
    pub const fn theme(&self) -> Theme {
        self.theme_with_fallback(Theme::default_dark())
    }

    /// What the terminal says it looks like, or `fallback` — the app's own
    /// preset for the terminals that stay silent.
    #[must_use]
    pub const fn theme_with_fallback(&self, fallback: Theme) -> Theme {
        stored_or(self.theme, fallback)
    }

    /// The ratatui terminal to draw on.
    pub const fn terminal_mut(&mut self) -> &mut Terminal<SessionBackend> {
        &mut self.terminal
    }

    /// Wait for the next thing worth telling the app about, for at most
    /// `timeout` — indefinitely when it is [`None`]. `Ok(None)` means the wait
    /// ran out with nothing to report.
    ///
    /// On a session that follows the terminal this also keeps the theme
    /// current, shortening the wait as needed to meet its deadlines and
    /// reporting each change as
    /// [`ThemeChanged`](SessionEvent::ThemeChanged).
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the terminal cannot be read or a re-query
    /// cannot be written.
    pub fn next(&mut self, timeout: Option<Duration>) -> io::Result<Option<SessionEvent>> {
        pump_and_remember(
            &self.events,
            self.terminal.backend_mut(),
            self.watch.as_mut(),
            &mut self.theme,
            timeout,
        )
    }

    /// Switch the modes on. A failure part-way through still restores, because
    /// the session already owns them: resetting a mode that never went on is
    /// what a terminal does with any mode it does not know.
    fn enable(mut self) -> io::Result<Self> {
        set_modes(self.terminal.backend_mut(), &self.modes)?;
        Ok(self)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = restore_modes(self.terminal.backend_mut(), &self.modes);
    }
}

/// The terminal modes a session switches on, in the order it switches them on.
///
/// Termina manages none of these: they are protocol, and the application
/// decides. Restoring walks the list back off in reverse, so the
/// theme subscription goes on last and comes off first — a terminal being put
/// back reports no change into a session that is closing.
fn modes(options: SessionOptions, subscribe: bool) -> Vec<DecPrivateModeCode> {
    use DecPrivateModeCode as M;

    let mut modes = vec![M::ClearAndEnableAlternateScreen];
    if options.mouse {
        // Presses, motion with a button held, motion without one, and SGR
        // coordinates — without the last, a click past column 223 cannot be
        // encoded at all.
        modes.extend([
            M::MouseTracking,
            M::ButtonEventMouse,
            M::AnyEventMouse,
            M::SGRMouse,
        ]);
    }
    if options.paste {
        modes.push(M::BracketedPaste);
    }
    if subscribe {
        // Focus tracking is the second trigger for a re-query: a terminal
        // recoloured from outside its own theme selector reports nothing, and
        // some terminals have no mode 2031 to report through.
        modes.extend([M::FocusTracking, M::Theme]);
    }
    modes
}

/// Where a session's events come from. [`EventReader`] is the one that matters;
/// a scripted one is how the routing below is checked without a terminal.
trait Source {
    fn poll(&self, timeout: Option<Duration>) -> io::Result<bool>;
    fn read(&self) -> io::Result<termina::Event>;
}

impl Source for EventReader {
    fn poll(&self, timeout: Option<Duration>) -> io::Result<bool> {
        Self::poll(self, timeout, |_| true)
    }

    fn read(&self) -> io::Result<termina::Event> {
        Self::read(self, |_| true)
    }
}

/// Wait for the next thing worth telling the app about, keeping `watch` fed on
/// the way.
fn pump<S: Source, W: io::Write>(
    source: &S,
    out: &mut W,
    mut watch: Option<&mut Watch>,
    timeout: Option<Duration>,
) -> io::Result<Option<SessionEvent>> {
    let deadline = timeout.map(|timeout| watch::deadline(Instant::now(), timeout));
    loop {
        let now = Instant::now();
        let left = deadline.map(|at| at.saturating_duration_since(now));
        if left == Some(Duration::ZERO) {
            return Ok(None);
        }
        let wake = watch.as_ref().and_then(|watch| watch.wake(now));
        let ready = source.poll(soonest(left, wake))?;
        let event = if ready { Some(source.read()?) } else { None };

        let now = Instant::now();
        if let Some(watch) = watch.as_mut() {
            if let Some(event) = &event {
                watch.absorb(event, now);
            }
            // A re-query that will not go out costs the theme an update, not
            // the app its life: the next notification tries again, and the
            // frames in between are the ones already on screen.
            if let Some(colors) = watch.step(out, now).unwrap_or(None) {
                return Ok(Some(SessionEvent::ThemeChanged(colors.theme())));
            }
        }
        match event {
            // A terminal's answer is never input, subscription or not: the app
            // cannot read it and would only have to discard it.
            Some(event) if event.is_escape() => {}
            Some(event) => return Ok(Some(SessionEvent::Input(event))),
            // A deadline fired. If it was the caller's, say so; if it was the
            // watch's, it has just been served.
            None if deadline.is_some_and(|at| Instant::now() >= at) => return Ok(None),
            None => {}
        }
    }
}

/// Wait for the next event, and keep `stored` in step with it.
///
/// Reporting a change and remembering it are one step: a caller that read
/// [`Session::theme`] after a [`SessionEvent::ThemeChanged`] would otherwise
/// see the theme the change replaced.
fn pump_and_remember<S: Source, W: io::Write>(
    source: &S,
    out: &mut W,
    watch: Option<&mut Watch>,
    stored: &mut Option<Theme>,
    timeout: Option<Duration>,
) -> io::Result<Option<SessionEvent>> {
    let event = pump(source, out, watch, timeout)?;
    remember(stored, event.as_ref());
    Ok(event)
}

/// Keep the stored theme in step with what the session just reported.
///
/// Only a change moves it: everything else the terminal says leaves the app
/// wearing what it already had.
fn remember(stored: &mut Option<Theme>, event: Option<&SessionEvent>) {
    if let Some(SessionEvent::ThemeChanged(theme)) = event {
        *stored = Some(*theme);
    }
}

/// What a session answers when asked for its theme: what the terminal said, or
/// the fallback the caller chose.
const fn stored_or(stored: Option<Theme>, fallback: Theme) -> Theme {
    match stored {
        Some(theme) => theme,
        None => fallback,
    }
}

/// What the opening query decided: the theme the terminal reported, and the
/// watch that follows it.
///
/// A terminal that answered is followed; one that would not say — or was never
/// asked — leaves both empty, so the watch exists exactly where mode 2031 is
/// switched on.
///
/// `now` is when the session opened, which is the quiet the first event is
/// measured against.
fn opening(answer: Option<TerminalColors>, now: Instant) -> (Option<Theme>, Option<Watch>) {
    match answer {
        Some(colors) => (Some(colors.theme()), Some(Watch::new(Some(colors), now))),
        None => (None, None),
    }
}

/// The nearer of two waits, where [`None`] is "no deadline of my own".
fn soonest(one: Option<Duration>, other: Option<Duration>) -> Option<Duration> {
    match (one, other) {
        (Some(one), Some(other)) => Some(one.min(other)),
        (only, None) | (None, only) => only,
    }
}

fn set_modes(out: &mut impl io::Write, modes: &[DecPrivateModeCode]) -> io::Result<()> {
    for &code in modes {
        let mode = DecPrivateMode::Code(code);
        write!(out, "{}", Csi::Mode(Mode::SetDecPrivateMode(mode)))?;
    }
    out.flush()
}

/// Switch the modes back off, newest first, and show the cursor.
///
/// A frame interrupted between hiding the cursor and showing it again leaves it
/// hidden, and a hidden cursor is invisible in the shell the user comes back to.
fn restore_modes(out: &mut impl io::Write, modes: &[DecPrivateModeCode]) -> io::Result<()> {
    for &code in modes.iter().rev() {
        let mode = DecPrivateMode::Code(code);
        write!(out, "{}", Csi::Mode(Mode::ResetDecPrivateMode(mode)))?;
    }
    let cursor = DecPrivateMode::Code(DecPrivateModeCode::ShowCursor);
    write!(out, "{}", Csi::Mode(Mode::SetDecPrivateMode(cursor)))?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::{
        DecPrivateModeCode as M, Duration, Instant, SessionEvent, SessionOptions, Source,
        TerminalColors, Watch, io, modes, opening, pump, pump_and_remember, remember,
        restore_modes, stored_or,
    };
    use crate::Theme;
    use ratatui::style::Color;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use termina::Parser;

    /// An event source the test writes the script for: each step is either an
    /// event to hand over or a wait to really sit out. A spent script is a
    /// terminal with nothing more to say, and [`POLLS`] is what keeps a loop
    /// that would never leave it from running forever.
    struct Scripted {
        steps: RefCell<VecDeque<Option<termina::Event>>>,
        /// The timeout each poll was given, in order.
        polls: RefCell<Vec<Option<Duration>>>,
    }

    /// How many times one `pump` call may ask the source before the loop it is
    /// in counts as a spin.
    ///
    /// Every script here is a handful of steps, and every pass through the loop
    /// spends one: a pump still asking after this many has stopped making
    /// progress through anything.
    const POLLS: usize = 64;

    impl Scripted {
        /// `script` parses to the events; `waits` marks, by index, the points
        /// where the source should instead sit out the wait it was given.
        fn new(script: &str, waits: &[usize]) -> Self {
            let mut parser = Parser::default();
            parser.parse(script.as_bytes(), false);
            let mut events = VecDeque::new();
            while let Some(event) = parser.pop() {
                events.push_back(event);
            }
            let mut steps: VecDeque<Option<termina::Event>> = VecDeque::new();
            for (index, event) in events.into_iter().enumerate() {
                if waits.contains(&index) {
                    steps.push_back(None);
                }
                steps.push_back(Some(event));
            }
            if waits.contains(&steps.len()) {
                steps.push_back(None);
            }
            Self {
                steps: RefCell::new(steps),
                polls: RefCell::new(Vec::new()),
            }
        }

        fn waiting() -> Self {
            Self {
                steps: RefCell::new(VecDeque::from([None])),
                polls: RefCell::new(Vec::new()),
            }
        }

        fn poll_at(&self, index: usize) -> Option<Duration> {
            self.polls.borrow().get(index).copied().flatten()
        }
    }

    impl Source for Scripted {
        fn poll(&self, timeout: Option<Duration>) -> io::Result<bool> {
            self.polls.borrow_mut().push(timeout);
            let asked = self.polls.borrow().len();
            if asked > POLLS {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    format!("the source was asked {asked} times: the loop is spinning"),
                ));
            }
            match self.steps.borrow().front() {
                Some(Some(_)) => return Ok(true),
                Some(None) => {}
                // A spent script is a terminal with nothing more to say: it
                // sits out whatever wait it was given, and answers an unbounded
                // one the way a silent terminal does, by staying silent.
                None => {
                    if let Some(timeout) = timeout {
                        std::thread::sleep(timeout);
                    }
                    return Ok(false);
                }
            }
            self.steps.borrow_mut().pop_front();
            if let Some(timeout) = timeout {
                std::thread::sleep(timeout);
            }
            Ok(false)
        }

        fn read(&self) -> io::Result<termina::Event> {
            self.steps
                .borrow_mut()
                .pop_front()
                .flatten()
                .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "nothing pending"))
        }
    }

    const NOTIFIED: &str = "\x1b[?997;2n";

    /// Long enough for a collapse window to close and its answer to arrive.
    const PATIENT: Duration = Duration::from_millis(500);

    /// Long enough for a collapse window to close, which is what a re-query on
    /// its way out would need.
    const PAST_THE_WINDOW: Duration = Duration::from_millis(200);
    const LIGHT: &str = "\x1b]10;rgb:6565/7b7b/8383\x07\x1b]11;rgb:fdfd/f6f6/e3e3\x07";

    fn typed(code: char) -> termina::Event {
        termina::Event::Key(termina::event::KeyEvent::from(
            termina::event::KeyCode::Char(code),
        ))
    }

    /// A watch on a session nobody has touched since it opened, which is how
    /// the quiet the input trigger answers to is arranged without sitting one
    /// out.
    fn quiet_watch() -> Watch {
        let opened = Instant::now()
            .checked_sub(super::watch::IDLE + Duration::from_secs(1))
            .expect("the process clock has six seconds behind it");
        Watch::new(None, opened)
    }

    #[test]
    fn typing_reaches_the_app_whether_or_not_the_terminal_is_watched() {
        for watched in [false, true] {
            let source = Scripted::new("q", &[]);
            let mut watch = Watch::new(None, Instant::now());
            let mut out = Vec::new();

            let event = pump(
                &source,
                &mut out,
                watched.then_some(&mut watch),
                Some(Duration::from_secs(5)),
            )
            .expect("a scripted source cannot fail");

            assert_eq!(
                event,
                Some(SessionEvent::Input(typed('q'))),
                "watched: {watched}"
            );
        }
    }

    #[test]
    fn focus_reaches_the_app_whether_or_not_the_terminal_is_watched() {
        for watched in [false, true] {
            let source = Scripted::new("\x1b[I", &[]);
            let mut watch = Watch::new(None, Instant::now());
            let mut out = Vec::new();

            let event = pump(
                &source,
                &mut out,
                watched.then_some(&mut watch),
                Some(PATIENT),
            )
            .expect("a scripted source cannot fail");

            assert_eq!(
                event,
                Some(SessionEvent::Input(termina::Event::FocusIn)),
                "the watch observes focus without taking it (watched: {watched})"
            );
            assert!(
                out.is_empty(),
                "focus opens a window; the re-query it triggers goes out when \
                 that window closes (watched: {watched})"
            );
        }
    }

    #[test]
    fn a_session_that_follows_nothing_writes_nothing_when_focus_returns() {
        // A session with no re-query to trigger has nothing for focus to do,
        // and the wait in the middle carries it long past any collapse window.
        let source = Scripted::new("\x1b[Iq", &[1]);
        let mut out = Vec::new();

        let focus =
            pump(&source, &mut out, None, Some(PATIENT)).expect("a scripted source cannot fail");
        let quiet = pump(&source, &mut out, None, Some(PAST_THE_WINDOW))
            .expect("a scripted source cannot fail");
        let after =
            pump(&source, &mut out, None, Some(PATIENT)).expect("a scripted source cannot fail");

        assert_eq!(focus, Some(SessionEvent::Input(termina::Event::FocusIn)));
        assert_eq!(
            quiet, None,
            "the wait ran out, and nothing came due while it did"
        );
        assert_eq!(
            after,
            Some(SessionEvent::Input(typed('q'))),
            "and the session goes on routing what the user does"
        );
        assert!(out.is_empty(), "not one byte went out");
    }

    #[test]
    fn a_terminals_own_answer_is_never_input() {
        // Unsolicited or left over, an escape report is not something the app
        // can read. The contract is the same with a watch and without one.
        for watched in [false, true] {
            let source = Scripted::new("\x1b[?62;1;6cq", &[]);
            let mut watch = Watch::new(None, Instant::now());
            let mut out = Vec::new();

            let event = pump(
                &source,
                &mut out,
                watched.then_some(&mut watch),
                Some(Duration::from_secs(5)),
            )
            .expect("a scripted source cannot fail");

            assert_eq!(
                event,
                Some(SessionEvent::Input(typed('q'))),
                "the report was stepped over, not handed on (watched: {watched})"
            );
        }
    }

    #[test]
    fn a_wait_that_runs_out_reports_nothing_and_asks_for_the_time_it_was_given() {
        let wait = Duration::from_millis(30);
        let source = Scripted::waiting();
        let mut out = Vec::new();

        let event =
            pump(&source, &mut out, None, Some(wait)).expect("a silent source is not an error");

        assert_eq!(event, None);
        let asked = source.poll_at(0).expect("the wait is bounded");
        assert!(asked <= wait, "asked for {asked:?}, was given {wait:?}");
    }

    #[test]
    fn a_pending_watch_shortens_a_wait_the_caller_made_long() {
        // The caller names five seconds; the watch owes a re-query in a
        // hundred milliseconds, and the poll has to come back in time for it.
        // A keystroke after the window closes, so the pump has something to
        // return once the re-query has gone out.
        let source = Scripted::new(&format!("{NOTIFIED}q"), &[1]);
        let mut watch = Watch::new(None, Instant::now());
        let mut out = Vec::new();

        let event = pump(
            &source,
            &mut out,
            Some(&mut watch),
            Some(Duration::from_secs(5)),
        )
        .expect("a scripted source cannot fail");

        assert_eq!(event, Some(SessionEvent::Input(typed('q'))));
        let shortened = source.poll_at(1).expect("a second wait happened");
        assert!(
            shortened <= Duration::from_millis(100),
            "the watch's deadline was not folded into the wait: {shortened:?}"
        );
    }

    #[test]
    fn the_colours_a_re_query_brings_back_are_the_theme_that_comes_out() {
        let source = Scripted::new(&format!("{NOTIFIED}{LIGHT}"), &[1]);
        let mut watch = Watch::new(None, Instant::now());
        let mut out = Vec::new();

        let event =
            pump(&source, &mut out, Some(&mut watch), None).expect("a scripted source cannot fail");

        assert_eq!(
            event,
            Some(SessionEvent::ThemeChanged(
                TerminalColors {
                    background: Color::Rgb(253, 246, 227),
                    foreground: Color::Rgb(101, 123, 131),
                }
                .theme()
            )),
            "the theme is solved from what the terminal just said"
        );
        assert_eq!(
            String::from_utf8_lossy(&out),
            "\x1b]10;?\x07\x1b]11;?\x07",
            "and the re-query went out through the terminal's own writer"
        );
    }

    fn answered() -> TerminalColors {
        TerminalColors {
            background: Color::Rgb(253, 246, 227),
            foreground: Color::Rgb(101, 123, 131),
        }
    }

    #[test]
    fn focus_returning_re_queries_and_reports_a_real_change() {
        // The reproducer, at the seam: a terminal that reports no change of its
        // own, recoloured while the app was in the background.
        let source = Scripted::new(&format!("\x1b[I{LIGHT}"), &[1]);
        let mut watch = Watch::new(None, Instant::now());
        let mut out = Vec::new();

        let focus = pump(&source, &mut out, Some(&mut watch), Some(PATIENT))
            .expect("a scripted source cannot fail");
        let event = pump(&source, &mut out, Some(&mut watch), Some(PATIENT))
            .expect("a scripted source cannot fail");

        assert_eq!(
            focus,
            Some(SessionEvent::Input(termina::Event::FocusIn)),
            "focus reaches the app: the watch observes it without taking it"
        );
        assert_eq!(
            event,
            Some(SessionEvent::ThemeChanged(answered().theme())),
            "the colors the re-query brought back are the new theme"
        );
        assert_eq!(
            String::from_utf8_lossy(&out),
            "\x1b]10;?\x07\x1b]11;?\x07",
            "and the re-query went out"
        );
    }

    #[test]
    fn focus_returning_to_an_unchanged_terminal_owes_the_app_nothing() {
        // Alt-tabbing costs a round trip and no frame.
        let known = TerminalColors {
            background: Color::Rgb(26, 27, 38),
            foreground: Color::Rgb(192, 202, 245),
        };
        let unchanged = "\x1b]10;rgb:c0c0/caca/f5f5\x07\x1b]11;rgb:1a1a/1b1b/2626\x07";
        let source = Scripted::new(&format!("\x1b[I{unchanged}q"), &[1]);
        let mut watch = Watch::new(Some(known), Instant::now());
        let mut out = Vec::new();

        let focus = pump(&source, &mut out, Some(&mut watch), Some(PATIENT))
            .expect("a scripted source cannot fail");
        let event = pump(&source, &mut out, Some(&mut watch), Some(PATIENT))
            .expect("a scripted source cannot fail");

        assert_eq!(focus, Some(SessionEvent::Input(termina::Event::FocusIn)));
        assert_eq!(
            event,
            Some(SessionEvent::Input(typed('q'))),
            "the re-query answered with what was already on screen, so the next \
             thing the app hears is the keystroke"
        );
    }

    #[test]
    fn input_after_a_long_quiet_re_queries_and_reports_a_real_change() {
        // The other reproducer: a compositor that gives the window no focus
        // event at all, so the user coming back to it is the whole signal.
        let source = Scripted::new(&format!("q{LIGHT}"), &[1]);
        let mut watch = quiet_watch();
        let mut out = Vec::new();

        let typing = pump(&source, &mut out, Some(&mut watch), Some(PATIENT))
            .expect("a scripted source cannot fail");
        let event = pump(&source, &mut out, Some(&mut watch), Some(PATIENT))
            .expect("a scripted source cannot fail");

        assert_eq!(
            typing,
            Some(SessionEvent::Input(typed('q'))),
            "the keystroke is the app's, trigger or not"
        );
        assert_eq!(
            event,
            Some(SessionEvent::ThemeChanged(answered().theme())),
            "the colors the re-query brought back are the new theme"
        );
        assert_eq!(
            String::from_utf8_lossy(&out),
            "\x1b]10;?\x07\x1b]11;?\x07",
            "and the re-query went out through the terminal's own writer"
        );
    }

    #[test]
    fn input_inside_a_session_already_in_use_asks_the_terminal_nothing() {
        // Typing is what a session is for. Only the first keystroke after a
        // spell of nothing at all says the user was somewhere else.
        let source = Scripted::new("q", &[1]);
        let mut watch = Watch::new(None, Instant::now());
        let mut out = Vec::new();

        let typing = pump(&source, &mut out, Some(&mut watch), Some(PATIENT))
            .expect("a scripted source cannot fail");
        let quiet = pump(&source, &mut out, Some(&mut watch), Some(PAST_THE_WINDOW))
            .expect("a scripted source cannot fail");

        assert_eq!(typing, Some(SessionEvent::Input(typed('q'))));
        assert_eq!(quiet, None, "the wait ran out with nothing to report");
        assert!(out.is_empty(), "and no window ever opened");
    }

    #[test]
    fn a_session_that_follows_nothing_writes_nothing_when_input_resumes() {
        // A session with no re-query to trigger has no quiet to answer to
        // either, however long it was left alone.
        let source = Scripted::new("q", &[1]);
        let mut out = Vec::new();

        let typing =
            pump(&source, &mut out, None, Some(PATIENT)).expect("a scripted source cannot fail");
        let quiet = pump(&source, &mut out, None, Some(PAST_THE_WINDOW))
            .expect("a scripted source cannot fail");

        assert_eq!(typing, Some(SessionEvent::Input(typed('q'))));
        assert_eq!(quiet, None);
        assert!(out.is_empty(), "not one byte went out");
    }

    #[test]
    fn a_terminal_that_answered_is_worn_and_followed() {
        let (theme, watch) = opening(Some(answered()), Instant::now());

        assert_eq!(theme, Some(answered().theme()), "the answer is worn");
        assert!(watch.is_some(), "and the terminal is followed");
    }

    #[test]
    fn a_terminal_that_said_nothing_is_neither_worn_nor_followed() {
        let (theme, watch) = opening(None, Instant::now());

        assert_eq!(theme, None, "there is nothing to wear but a fallback");
        assert!(
            watch.is_none(),
            "and nothing to follow: the watch exists exactly where mode 2031 \
             is switched on"
        );
    }

    /// The three states a session can be in, through the getters' own logic.
    #[test]
    fn the_change_reported_is_the_change_remembered() {
        // A caller that reads the theme after the event must see the theme the
        // event carried.
        let source = Scripted::new(&format!("{NOTIFIED}{LIGHT}"), &[1]);
        let mut watch = Watch::new(None, Instant::now());
        let mut out = Vec::new();
        let mut stored = None;

        let event = pump_and_remember(&source, &mut out, Some(&mut watch), &mut stored, None)
            .expect("a scripted source cannot fail");

        let solved = TerminalColors {
            background: Color::Rgb(253, 246, 227),
            foreground: Color::Rgb(101, 123, 131),
        }
        .theme();
        assert_eq!(event, Some(SessionEvent::ThemeChanged(solved)));
        assert_eq!(stored, Some(solved), "and the session answers with it");
    }

    #[test]
    fn a_reported_change_becomes_the_theme_the_session_answers_with() {
        let solved = answered().theme();
        let mut stored = None;

        remember(&mut stored, Some(&SessionEvent::ThemeChanged(solved)));
        assert_eq!(stored, Some(solved), "the change is what gets stored");

        remember(&mut stored, Some(&SessionEvent::Input(typed('q'))));
        assert_eq!(stored, Some(solved), "typing does not disturb it");

        remember(&mut stored, None);
        assert_eq!(stored, Some(solved), "nor does a wait that ran out");
    }

    #[test]
    fn the_theme_is_what_the_terminal_said_or_the_fallback() {
        let solved = answered().theme();
        let mine = Theme::gruvbox();

        // Never asked, and asked-but-unanswered, are the same state: nothing
        // stored, so each getter yields its own fallback.
        assert_eq!(stored_or(None, mine), mine);
        assert_eq!(
            stored_or(None, Theme::default_dark()),
            Theme::default_dark(),
            "which is what `theme()` falls back to"
        );

        // Asked and answered: neither fallback is reached.
        assert_eq!(stored_or(Some(solved), mine), solved);
        assert_ne!(stored_or(Some(solved), mine), mine);
    }

    #[test]
    fn nothing_is_switched_on_that_was_not_asked_for() {
        let bare = modes(SessionOptions::new(), false);

        assert_eq!(
            bare,
            vec![M::ClearAndEnableAlternateScreen],
            "a terminal keeps its own mouse and paste until an app asks"
        );
    }

    #[test]
    fn each_option_brings_only_its_own_modes() {
        let mouse = modes(SessionOptions::new().mouse(), false);
        let paste = modes(SessionOptions::new().paste(), false);

        assert!(mouse.contains(&M::SGRMouse) && !mouse.contains(&M::BracketedPaste));
        assert!(paste.contains(&M::BracketedPaste) && !paste.contains(&M::SGRMouse));
    }

    #[test]
    fn the_subscription_goes_on_last_so_it_comes_off_first() {
        let unanswered = modes(SessionOptions::new().mouse().paste(), false);
        let subscribed = modes(SessionOptions::new().mouse().paste(), true);

        assert!(
            !unanswered.contains(&M::Theme),
            "a terminal that did not answer is not subscribed to"
        );
        assert_eq!(
            subscribed.last(),
            Some(&M::Theme),
            "restoring walks the list backwards, so last on is first off"
        );
        assert_eq!(
            subscribed.first(),
            Some(&M::ClearAndEnableAlternateScreen),
            "and the screen the user came from is the last thing given back"
        );
    }

    #[test]
    fn following_the_terminal_turns_on_focus_reporting_too() {
        let unanswered = modes(SessionOptions::new().mouse(), false);
        let subscribed = modes(SessionOptions::new().mouse(), true);

        assert!(
            !unanswered.contains(&M::FocusTracking),
            "a session with no re-query has nothing for focus to trigger"
        );
        assert!(
            subscribed.contains(&M::FocusTracking),
            "focus is the second trigger for a re-query"
        );
    }

    #[test]
    fn restoring_switches_the_subscription_off_before_anything_else() {
        let modes = modes(SessionOptions::new().mouse().paste(), true);
        let mut written = Vec::new();

        restore_modes(&mut written, &modes).expect("a vector accepts every write");

        let written = String::from_utf8(written).expect("escape sequences are ASCII");
        let subscription_off = written
            .find("\x1b[?2031l")
            .expect("the subscription is switched off");
        let screen_back = written
            .find("\x1b[?1049l")
            .expect("the alternate screen is left");

        assert!(
            subscription_off < screen_back,
            "no change may be reported into a terminal that has started being \
             put back: {written:?}"
        );
        assert!(
            written.ends_with("\x1b[?25h"),
            "and the cursor is visible again at the end: {written:?}"
        );
    }
}
