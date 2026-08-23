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
        // A terminal that answered is followed; one that would not say, or was
        // never asked, is not, so the watch exists exactly where mode 2031 is
        // switched on. Now is the quiet the first event is measured against.
        let watch = answer.map(|colors| Watch::new(colors, Instant::now()));
        let theme = answer.map(TerminalColors::theme);

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
    pub fn theme(&self) -> Theme {
        self.theme_with_fallback(Theme::default_dark())
    }

    /// What the terminal says it looks like, or `fallback` — the app's own
    /// preset for the terminals that stay silent.
    #[must_use]
    pub fn theme_with_fallback(&self, fallback: Theme) -> Theme {
        self.theme.unwrap_or(fallback)
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
        pump(
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
/// the way and `theme` in step with what it reports.
///
/// Reporting a change and remembering it are one step: a caller that read
/// [`Session::theme`] after a [`SessionEvent::ThemeChanged`] would otherwise
/// see the theme the change replaced.
fn pump<S: Source, W: io::Write>(
    source: &S,
    out: &mut W,
    mut watch: Option<&mut Watch>,
    theme: &mut Option<Theme>,
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
            // This preempts no input: the watch completes a theme only on the
            // pass whose event was the reply that completed it, which is an
            // escape the match below would drop anyway.
            if let Some(colors) = watch.step(out, now)? {
                let changed = colors.theme();
                *theme = Some(changed);
                return Ok(Some(SessionEvent::ThemeChanged(changed)));
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
    use std::io;

    use super::{
        DecPrivateModeCode as M, Duration, Instant, SessionEvent, SessionOptions, TerminalColors,
        Watch, modes, pump, restore_modes,
        watch::{DEBOUNCE, IDLE},
    };
    use crate::terminal::fake::FakeTerminal;
    use ratatui::style::Color;

    const NOTIFIED: &str = "\x1b[?997;2n";
    const LIGHT: &str = "\x1b]10;rgb:6565/7b7b/8383\x07\x1b]11;rgb:fdfd/f6f6/e3e3\x07";

    fn typed(code: char) -> termina::Event {
        termina::Event::Key(termina::event::KeyEvent::from(
            termina::event::KeyCode::Char(code),
        ))
    }

    fn notified() -> termina::Event {
        let mut parser = termina::Parser::default();
        parser.parse(NOTIFIED.as_bytes(), false);
        parser.pop().expect("a notification is one event")
    }

    /// The dark colors the startup query answered.
    fn dark() -> TerminalColors {
        TerminalColors {
            background: Color::Rgb(26, 27, 38),
            foreground: Color::Rgb(192, 202, 245),
        }
    }

    /// `span` ago — an instant the clock is past, so a deadline measured from
    /// it is already due without sitting anything out.
    fn ago(span: Duration) -> Instant {
        Instant::now()
            .checked_sub(span)
            .expect("the process clock has seconds behind it")
    }

    /// A watch whose collapse window closed already, so its re-query goes out
    /// on the next step.
    fn watch_owing_a_re_query() -> Watch {
        let triggered = ago(DEBOUNCE + Duration::from_millis(10));
        let mut watch = Watch::new(dark(), ago(IDLE));
        watch.absorb(&notified(), triggered);
        watch
    }

    /// A writer the terminal has stopped accepting.
    struct Unwritable;

    impl io::Write for Unwritable {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "gone"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_terminals_own_answer_is_never_input() {
        // Unsolicited or left over, an escape report is not something the app
        // can read. The contract is the same with a watch and without one.
        for watched in [false, true] {
            let source = FakeTerminal::scripted("\x1b[?62;1;6cq");
            let mut watch = Watch::new(dark(), Instant::now());
            let mut out = Vec::new();
            let mut theme = None;

            let event = pump(
                &source,
                &mut out,
                watched.then_some(&mut watch),
                &mut theme,
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
    fn a_pending_watch_shortens_a_wait_the_caller_made_long() {
        // The caller names five seconds; the watch owes a re-query when its
        // window closes, and the poll has to come back in time for it.
        let source = FakeTerminal::scripted("q");
        let mut watch = Watch::new(dark(), Instant::now());
        watch.absorb(&notified(), Instant::now());
        let mut out = Vec::new();
        let mut theme = None;

        let event = pump(
            &source,
            &mut out,
            Some(&mut watch),
            &mut theme,
            Some(Duration::from_secs(5)),
        )
        .expect("a scripted source cannot fail");

        assert_eq!(event, Some(SessionEvent::Input(typed('q'))));
        let shortened = source.poll_at(0).expect("the wait is bounded");
        assert!(
            shortened <= DEBOUNCE,
            "the watch's deadline was not folded into the wait: {shortened:?}"
        );
    }

    #[test]
    fn the_change_reported_is_the_change_remembered() {
        // A caller that reads the theme after the event must see the theme the
        // event carried.
        let mut watch = watch_owing_a_re_query();
        watch
            .step(&mut Vec::new(), Instant::now())
            .expect("writing to a vector cannot fail");
        let source = FakeTerminal::scripted(LIGHT);
        let mut out = Vec::new();
        let mut theme = None;

        let event = pump(&source, &mut out, Some(&mut watch), &mut theme, None)
            .expect("a scripted source cannot fail");

        let solved = TerminalColors {
            background: Color::Rgb(253, 246, 227),
            foreground: Color::Rgb(101, 123, 131),
        }
        .theme();
        assert_eq!(
            event,
            Some(SessionEvent::ThemeChanged(solved)),
            "the theme is solved from what the terminal just said"
        );
        assert_eq!(theme, Some(solved), "and the session answers with it");
    }

    #[test]
    fn a_re_query_that_cannot_be_written_is_an_error() {
        // The contract `Session::next` states: a terminal that will not take
        // the re-query is a terminal the app cannot go on with.
        let mut watch = watch_owing_a_re_query();
        let source = FakeTerminal::scripted("q");
        let mut theme = None;

        let result = pump(
            &source,
            &mut Unwritable,
            Some(&mut watch),
            &mut theme,
            Some(Duration::from_secs(5)),
        );

        assert!(
            matches!(&result, Err(error) if error.kind() == io::ErrorKind::BrokenPipe),
            "the write failure is the caller's to see: {result:?}"
        );
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
