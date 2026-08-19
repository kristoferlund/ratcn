//! Asking the terminal what colors it is actually painting with.
//!
//! [`Theme::adaptive`](crate::Theme::adaptive) solves a whole theme from a
//! background and a foreground. This module is where those two colors come
//! from on a real terminal: one batched query at startup, answered by the
//! terminal itself, so an app adopts the user's colors instead of guessing at
//! them.
//!
//! ```no_run
//! use ratcn::{Theme, terminal_query};
//! use termina::{PlatformTerminal, Terminal as _};
//!
//! # fn main() -> std::io::Result<()> {
//! let mut terminal = PlatformTerminal::new()?;
//! terminal.enter_raw_mode()?;
//!
//! // Before the app's event loop takes its first event: see below.
//! let theme = match terminal_query::query(&mut terminal)? {
//!     Some(colors) => Theme::adaptive(
//!         colors.background,
//!         colors.foreground,
//!         colors.palette16.as_ref(),
//!     ),
//!     None => Theme::default_dark(),
//! };
//! # let _ = theme;
//! # Ok(())
//! # }
//! ```
//!
//! # There is exactly one window for this
//!
//! The query writes to the terminal and reads the terminal's answers back out
//! of the input stream. It must run after raw mode is on — a cooked terminal
//! buffers the reply until Enter — and before the app has read its first
//! event, because until the exchange is fenced the app cannot tell a reply
//! apart from typing. Termina's filtered reads keep any key presses that arrive
//! meanwhile buffered for the event loop, so an impatient user loses nothing.
//!
//! The contract that buys is narrow and worth stating exactly: every reply is
//! consumed here or left buffered as a *typed* event, so nothing the terminal
//! says becomes input the app has to recognize and discard. What it is not is a
//! promise about termina's parser — a read that splits a reply on a lone `ESC`
//! can still decode the remainder as characters, the shape crossterm turns
//! every reply into. Realistic terminal writes do not split there, and this
//! module does not depend on them not doing so.
//!
//! # A query that is not answered has to end anyway
//!
//! Terminals answer in the order asked, so the exchange ends with a primary
//! device-attributes request as a fence: when the DA1 reply arrives, whatever
//! has not answered will not answer, and the query stops without waiting. A
//! terminal that answers nothing at all — including the fence — stops the query
//! at a one-second backstop.
//!
//! The fence is what ends a normal exchange, so an answer that arrives *after*
//! it is simply lost — that is what a fence is for, and the backstop does not
//! rescue it. Ghostty answered the light/dark query out of order until January
//! 2026 and would have lost its verdict here. Nothing depends on that verdict:
//! it is advisory, and the colors are what a theme is built from.
//!
//! Terminals known to mishandle the exchange are never asked: see [`query`].

use std::{
    env, io,
    io::IsTerminal as _,
    time::{Duration, Instant},
};

use ratatui::style::Color;
use termina::{
    Event, Terminal,
    escape::{
        csi::{self, Csi},
        osc::{ColorOrQuery, DynamicColorNumber, Osc},
    },
    style::RgbColor,
};

/// The batched query, in the order the terminal answers it.
///
/// The two color queries are BEL-terminated rather than ST-terminated: every
/// terminal accepts BEL, and the ones that answer with the terminator they were
/// sent then answer with BEL too. Replies are accepted with either terminator
/// regardless, because three terminals mirror neither.
///
/// `CSI ? 996 n` is the one-shot light/dark query. The subscription form (DEC
/// mode 2031, which pushes a notification when the user flips their theme) is
/// deliberately not enabled here: a subscription outlives this window and
/// belongs to whoever owns the event loop.
///
/// `CSI c` — primary device attributes — is last because it is the fence.
const QUERIES: &str = "\x1b]10;?\x07\x1b]11;?\x07\x1b[?996n\x1b[c";

/// How long the whole exchange may take, fence or no fence.
const BACKSTOP: Duration = Duration::from_secs(1);

/// What the terminal reported about its own colors.
///
/// Feed [`background`](Self::background), [`foreground`](Self::foreground), and
/// [`palette16`](Self::palette16) to [`Theme::adaptive`](crate::Theme::adaptive)
/// to get a theme built around them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct TerminalColors {
    /// The terminal's default background, from OSC 11.
    pub background: Color,
    /// The terminal's default foreground, from OSC 10.
    pub foreground: Color,
    /// The terminal's own light/dark verdict, from `CSI ? 996 n`, when it
    /// answered.
    ///
    /// Advisory only. Terminals disagree about whether it describes the palette
    /// or the desktop theme, and
    /// [`Theme::adaptive`](crate::Theme::adaptive) reads polarity off
    /// [`background`](Self::background) anyway — so when the two disagree, the
    /// background wins, because the background is what the theme is built from.
    pub dark: Option<bool>,
    /// The 16 ANSI palette colors, for the accents a theme would otherwise
    /// invent.
    ///
    /// Always [`None`]: OSC 4 is what carries them, and termina 0.3.3's parser
    /// drops OSC 4 replies rather than typing them. Reading them anyway would
    /// mean a second reader competing with termina's for the same bytes, which
    /// is the defect this backend was chosen to avoid.
    pub palette16: Option<[Color; 16]>,
}

/// Ask `terminal` for its colors, and wait out the answer.
///
/// Returns [`None`] when there is nothing to derive a theme from — the terminal
/// was not asked, it did not answer, or it answered only in part. Callers fall
/// back to a preset.
///
/// The terminal must already be in raw mode, and nothing may have read an event
/// from it yet. See the [module documentation](self) for why that window is the
/// only one.
///
/// # Gates
///
/// The query is skipped entirely, without writing a byte, when:
///
/// - `TERM` is unset or empty, or is `dumb` — no escape sequences at all;
/// - `TERM` names GNU screen or Eterm — screen answers the DA1 fence out of its
///   own capabilities while dropping the color queries, so the fence would
///   close on a reply that proves nothing;
/// - stdout is not a terminal — the output is being captured or paged, and the
///   query bytes would land in the capture while a pager races us for the
///   reply.
///
/// # Errors
///
/// Returns an I/O error if the query cannot be written, or if reading the
/// terminal fails. A terminal that simply says nothing is not an error: it is
/// [`None`].
pub fn query<T: Terminal>(terminal: &mut T) -> io::Result<Option<TerminalColors>> {
    exchange(
        terminal,
        asked(env::var("TERM").ok().as_deref(), io::stdout().is_terminal()),
    )
}

/// The exchange itself, with the gate's verdict handed to it rather than read
/// here.
///
/// Whether this process's stdout is a terminal is not something a test can
/// arrange — `cargo test` inherits the developer's tty and hands CI a pipe, so
/// the same assertion would come out differently on the two. Passing the
/// verdict in is what makes the refusal reachable as a value, and it is the
/// only reason this is not one function.
fn exchange<T: Terminal>(terminal: &mut T, asked: bool) -> io::Result<Option<TerminalColors>> {
    if !asked {
        return Ok(None);
    }

    terminal.write_all(QUERIES.as_bytes())?;
    terminal.flush()?;

    let started = Instant::now();
    let mut replies = Replies::default();
    while let Some(remaining) = BACKSTOP.checked_sub(started.elapsed()) {
        // Filtering on escape sequences leaves key presses buffered for the
        // event loop rather than eating them to get at the replies.
        if !terminal.poll(Event::is_escape, Some(remaining))? {
            break;
        }
        if replies.absorb(&terminal.read(Event::is_escape)?) == Fence::Closed {
            break;
        }
    }

    Ok(replies.resolve())
}

/// The re-query a notification triggers: the two colors, and no fence.
///
/// Nothing is left for a fence to decide. The terminal answered these once
/// already — that is what got the subscription switched on — so what bounds
/// this exchange is [`ThemeWatch`]'s own deadline, not a reply that says the
/// rest will never come.
const REQUERY: &str = "\x1b]10;?\x07\x1b]11;?\x07";

/// How long notifications are collected before the colors are asked for.
///
/// Terminals send these in bursts — one per palette slot on some, one per
/// transition step on others — and each one would otherwise cost a round trip
/// and a repaint.
const DEBOUNCE: Duration = Duration::from_millis(100);

/// Follows a terminal that reports its own theme changes, and works out what
/// the app should look like afterwards.
///
/// Switch on DEC mode 2031 once [`query`] has answered, then hand this every
/// event the loop reads and call [`step`](Self::step) once per pass. It asks
/// the terminal for its colors again when a change settles, and hands back the
/// new ones when they arrive. Switch the mode off again before the terminal is
/// handed back.
///
/// It never blocks and owns no thread: the host keeps waiting on its own event
/// source, shortening the wait to [`wake`](Self::wake) when there is a deadline
/// to meet.
///
/// # Which terminals to switch the mode on for
///
/// Any terminal that answered the startup query at all. There is no way to ask
/// mode 2031 whether it is supported: DECRQM would be the direct test, and
/// termina 0.3.3's parser types DECRPM replies for modes 2026 and 2027 only,
/// dropping mode 2031's at every reported value — the answer can be written but
/// never read.
///
/// The light/dark query is *not* the signal either, tempting as it looks. The
/// specification defines `CSI ? 996 n` and mode 2031 as separate capabilities
/// and mandates neither from the other, and the query is the more widely
/// implemented of the two; a terminal whose verdict merely arrived after the
/// startup fence would be written off despite supporting everything.
///
/// Switching the mode on where it is not supported costs nothing: `DECSET`
/// solicits no reply, and a terminal that does not know a private mode ignores
/// it. The notification that never comes is the same silence as not asking.
///
/// # Example
///
/// The shape of a host loop. Note that `wake` returning [`None`] means the
/// watch owes nothing — which is a wait with no timeout, not a wait of zero —
/// and that a poll which times out must not be followed by a read.
///
/// ```no_run
/// use std::time::Instant;
///
/// use ratcn::{Theme, terminal_query::ThemeWatch};
/// use termina::{PlatformTerminal, Terminal as _};
///
/// # fn main() -> std::io::Result<()> {
/// let mut terminal = PlatformTerminal::new()?;
/// terminal.enter_raw_mode()?;
/// let reader = terminal.event_reader();
/// let mut watch = ThemeWatch::new();
///
/// loop {
///     // Wait for input, but never past the watch's next deadline.
///     let ready = reader.poll(watch.wake(Instant::now()), |_| true)?;
///     if ready {
///         let event = reader.read(|_| true)?;
///         watch.absorb(&event, Instant::now());
///         // ... and route `event` to the app as usual.
///     }
///     if let Some(colors) = watch.step(&mut terminal, Instant::now())? {
///         let _theme = Theme::adaptive(colors.background, colors.foreground, None);
///     }
///     # break;
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct ThemeWatch {
    state: Watching,
}

/// Where the watch is in the notification-to-new-colors round trip.
#[derive(Debug)]
enum Watching {
    /// Nothing has happened, and nothing is owed.
    Idle,
    /// A change was reported. Everything that arrives before `due` is the same
    /// change still settling.
    Collapsing { due: Instant },
    /// The colors have been asked for again, and the answer is owed by
    /// `deadline`.
    Awaiting { deadline: Instant, replies: Replies },
}

impl Default for ThemeWatch {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeWatch {
    /// A watch with nothing pending.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: Watching::Idle,
        }
    }

    /// Note one event read from the terminal.
    ///
    /// Events that are neither a change notification nor a reply this watch is
    /// waiting for are ignored, so a host can hand it everything it reads
    /// without sorting first — and hand the same event on to the app, which is
    /// what keeps a keystroke that arrived mid-exchange a keystroke.
    pub fn absorb(&mut self, event: &Event, now: Instant) {
        match event {
            // The notification carries a light/dark value of its own, and it is
            // deliberately not read: terminals disagree over whether it
            // describes the palette or the desktop, so it says *that* something
            // changed and never *what* it changed to. The colors come from the
            // re-query.
            //
            // A `CSI ? 997 ; N n` that is not a notification at all — the
            // startup query's own verdict, arriving after the fence gave up on
            // it — is indistinguishable from one, and costs a re-query that
            // reports the colors already showing. That is the accepted price:
            // it corrects itself, and the alternative is a subscription that
            // second-guesses the terminal.
            Event::Csi(Csi::Mode(csi::Mode::ReportTheme(_))) => {
                // A window already open is not pushed back. Extending it on
                // every notification would let a terminal that keeps sending
                // hold the re-query off indefinitely; collapsing into a fixed
                // window instead guarantees the round trip happens, and a
                // change that lands after it opens a window of its own.
                if !matches!(self.state, Watching::Collapsing { .. }) {
                    self.state = Watching::Collapsing {
                        due: deadline(now, DEBOUNCE),
                    };
                }
            }
            // Anything collected before this notification described the theme
            // the terminal has just left, so it is dropped with the state.
            event => {
                if let Watching::Awaiting { replies, .. } = &mut self.state {
                    let _fence = replies.absorb(event);
                }
            }
        }
    }

    /// How long the host may wait before calling [`step`](Self::step) again, or
    /// [`None`] when nothing is pending and the wait is the app's to decide.
    #[must_use]
    pub fn wake(&self, now: Instant) -> Option<Duration> {
        match self.state {
            Watching::Idle => None,
            Watching::Collapsing { due: at } | Watching::Awaiting { deadline: at, .. } => {
                Some(at.saturating_duration_since(now))
            }
        }
    }

    /// Advance the watch, writing the re-query to `out` when one comes due.
    ///
    /// Returns the terminal's new colors on the pass that completes them. A
    /// terminal that stops answering costs one bounded wait and is then dropped
    /// quietly: the colors already on screen are the last ones it stood behind,
    /// which is a better answer than none.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the re-query cannot be written.
    pub fn step<W: io::Write>(
        &mut self,
        out: &mut W,
        now: Instant,
    ) -> io::Result<Option<TerminalColors>> {
        match &mut self.state {
            Watching::Idle => Ok(None),
            Watching::Collapsing { due } => {
                if now < *due {
                    return Ok(None);
                }
                out.write_all(REQUERY.as_bytes())?;
                out.flush()?;
                self.state = Watching::Awaiting {
                    deadline: deadline(now, BACKSTOP),
                    replies: Replies::default(),
                };
                Ok(None)
            }
            Watching::Awaiting { deadline, replies } => {
                if let Some(colors) = replies.resolve() {
                    self.state = Watching::Idle;
                    return Ok(Some(colors));
                }
                if now >= *deadline {
                    self.state = Watching::Idle;
                }
                Ok(None)
            }
        }
    }
}

/// `now + span`, saturating to `now` at the end of the clock rather than
/// panicking — a deadline already reached is the safe direction.
fn deadline(now: Instant, span: Duration) -> Instant {
    now.checked_add(span).unwrap_or(now)
}

/// Whether this terminal is asked at all. See [`query`]'s gates.
fn asked(term: Option<&str>, stdout_is_tty: bool) -> bool {
    stdout_is_tty
        && match term {
            None | Some("" | "dumb") => false,
            Some(term) => !term.starts_with("screen") && !term.starts_with("Eterm"),
        }
}

/// Whether the device-attributes fence has closed the exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fence {
    Open,
    Closed,
}

/// The replies collected so far.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Replies {
    background: Option<Color>,
    foreground: Option<Color>,
    dark: Option<bool>,
}

impl Replies {
    /// Take one event from the terminal, and report whether it was the fence.
    ///
    /// Anything else — a stray report, a mouse event, a key press that slipped
    /// past the filter — is passed over: the exchange ends at the fence or at
    /// the backstop, never at the first thing it did not recognize.
    fn absorb(&mut self, event: &Event) -> Fence {
        match event {
            Event::Osc(Osc::ChangeDynamicColors(number, colors)) => {
                if let Some(color) = reported(colors) {
                    match number {
                        DynamicColorNumber::TextForegroundColor => self.foreground = Some(color),
                        DynamicColorNumber::TextBackgroundColor => self.background = Some(color),
                        _ => {}
                    }
                }
                Fence::Open
            }
            Event::Csi(Csi::Mode(csi::Mode::ReportTheme(mode))) => {
                self.dark = Some(matches!(mode, csi::ThemeMode::Dark));
                Fence::Open
            }
            // Only the private form, `CSI ? … c`, which is what every terminal
            // actually replies with. A bare `CSI … c` reply would miss the
            // fence and cost the backstop's second — a stall, not a wrong
            // answer, since the replies collected so far still stand.
            Event::Csi(Csi::Device(csi::Device::DeviceAttributes(()))) => Fence::Closed,
            _ => Fence::Open,
        }
    }

    /// Both colors or nothing.
    ///
    /// A theme is derived from the pair, so half of it derives nothing. A
    /// light/dark verdict that arrived without them could still pick between
    /// two presets by polarity — but that is a preset either way, which is what
    /// the caller's fallback already is.
    fn resolve(self) -> Option<TerminalColors> {
        Some(TerminalColors {
            background: self.background?,
            foreground: self.foreground?,
            dark: self.dark,
            palette16: None,
        })
    }
}

/// The color out of a dynamic-color reply, if the reply carried one.
///
/// A `?` in a reply is the query form echoed back, which says nothing.
fn reported(colors: &[ColorOrQuery]) -> Option<Color> {
    colors.iter().find_map(|color| match color {
        ColorOrQuery::Color(RgbColor { red, green, blue }) => Some(Color::Rgb(*red, *green, *blue)),
        ColorOrQuery::Query => None,
    })
}

/// The exchange, driven from scripted terminal bytes.
///
/// Termina's parser is the same one the event reader runs, so a fixture reply
/// string reaches [`Replies::absorb`] exactly as the terminal's own bytes
/// would — and none of it needs a terminal.
#[cfg(test)]
mod tests {
    use super::{
        ColorOrQuery, Csi, Duration, DynamicColorNumber, Event, Fence, Osc, QUERIES, REQUERY,
        Replies, Terminal, TerminalColors, ThemeWatch, asked, csi, exchange, io,
    };
    use ratatui::style::Color;
    use std::{cell::RefCell, collections::VecDeque, time::Instant};
    use termina::Parser;

    /// A terminal that records what was written to it and answers from a script.
    ///
    /// Only what [`exchange`] touches is real: the writes, the filtered polls
    /// and reads, and the timeout each wait was given. `event_reader` is the one
    /// method whose return type this crate cannot build, and the exchange never
    /// calls it — it reads through the trait, which is what lets the whole
    /// exchange be driven with no terminal anywhere.
    struct FakeTerminal {
        written: Vec<u8>,
        /// Replies not yet handed over, oldest first.
        pending: RefCell<VecDeque<Event>>,
        /// The timeout each `poll` was given, in order.
        polls: RefCell<Vec<Option<Duration>>>,
    }

    impl FakeTerminal {
        /// A terminal that will answer with whatever `script` parses to.
        fn answering(script: &str) -> Self {
            let mut parser = Parser::default();
            parser.parse(script.as_bytes(), false);
            let mut pending = VecDeque::new();
            while let Some(event) = parser.pop() {
                pending.push_back(event);
            }
            Self {
                written: Vec::new(),
                pending: RefCell::new(pending),
                polls: RefCell::new(Vec::new()),
            }
        }

        fn still_pending(&self) -> Vec<Event> {
            self.pending.borrow().iter().cloned().collect()
        }
    }

    impl io::Write for FakeTerminal {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Terminal for FakeTerminal {
        fn enter_raw_mode(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn enter_cooked_mode(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn get_dimensions(&self) -> io::Result<termina::WindowSize> {
            Ok(termina::WindowSize {
                cols: 80,
                rows: 24,
                pixel_width: None,
                pixel_height: None,
            })
        }

        fn event_reader(&self) -> termina::EventReader {
            unimplemented!("the exchange reads through `poll` and `read`, never through a reader")
        }

        fn poll<F: Fn(&Event) -> bool>(
            &self,
            filter: F,
            timeout: Option<Duration>,
        ) -> io::Result<bool> {
            self.polls.borrow_mut().push(timeout);
            Ok(self.pending.borrow().iter().any(filter))
        }

        fn read<F: Fn(&Event) -> bool>(&self, filter: F) -> io::Result<Event> {
            let mut pending = self.pending.borrow_mut();
            match pending.iter().position(filter) {
                Some(index) => Ok(pending
                    .remove(index)
                    .expect("the index came from this queue")),
                // The real reader blocks here until something matches. Failing
                // instead is what makes a read let through on the strength of
                // the wrong filter show up as a failure rather than a hang.
                None => Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "nothing pending matches the filter",
                )),
            }
        }

        fn set_panic_hook(
            &mut self,
            _hook: impl Fn(&mut termina::PlatformHandle) + Send + Sync + 'static,
        ) {
        }
    }

    fn typed(code: char) -> Event {
        Event::Key(termina::event::KeyEvent::from(
            termina::event::KeyCode::Char(code),
        ))
    }

    /// Everything the watch is told, at a time the test names. No clock is read
    /// anywhere in here: `t0` is a fixed origin and every deadline is an offset
    /// from it, so the state machine is exercised at exact instants.
    struct Watched {
        watch: ThemeWatch,
        written: Vec<u8>,
        t0: Instant,
    }

    impl Watched {
        fn new() -> Self {
            Self {
                watch: ThemeWatch::new(),
                written: Vec::new(),
                t0: Instant::now(),
            }
        }

        fn at(&self, millis: u64) -> Instant {
            self.t0 + Duration::from_millis(millis)
        }

        /// Feed everything `script` parses to, as arriving at `millis`.
        fn feed(&mut self, millis: u64, script: &str) {
            let mut parser = Parser::default();
            parser.parse(script.as_bytes(), false);
            while let Some(event) = parser.pop() {
                self.watch.absorb(&event, self.at(millis));
            }
        }

        fn step(&mut self, millis: u64) -> Option<TerminalColors> {
            let at = self.at(millis);
            self.watch
                .step(&mut self.written, at)
                .expect("writing to a vector cannot fail")
        }

        fn wake(&self, millis: u64) -> Option<Duration> {
            self.watch.wake(self.at(millis))
        }

        /// How many re-queries have gone out.
        fn requeries(&self) -> usize {
            String::from_utf8_lossy(&self.written)
                .matches(REQUERY)
                .count()
        }
    }

    /// The reply to a re-query: the two colors, no verdict and no fence.
    const RETHEMED: &str = "\x1b]10;rgb:6565/7b7b/8383\x07\x1b]11;rgb:fdfd/f6f6/e3e3\x07";

    /// A change notification. The value it carries is never read.
    const NOTIFIED: &str = "\x1b[?997;2n";

    #[test]
    fn a_lost_light_dark_verdict_costs_nothing_but_the_verdict() {
        // The verdict and the subscription are separate capabilities, and the
        // verdict is the more widely implemented of the two — so a terminal
        // whose verdict was lost to the fence still answers the colors, and is
        // still worth subscribing to. Nothing here reads `dark` to decide.
        let both = absorbed(ANSWERED).0.resolve().expect("both colors");
        let lost_verdict = absorbed("\x1b]10;rgb:c0c0/caca/f5f5\x07\x1b]11;rgb:1a1a/1b1b/2626\x07")
            .0
            .resolve()
            .expect("both colors");

        assert_eq!(both.dark, Some(true));
        assert_eq!(lost_verdict.dark, None);
        assert_eq!(
            (both.background, both.foreground),
            (lost_verdict.background, lost_verdict.foreground),
            "the colors a theme is built from are the same either way"
        );
    }

    #[test]
    fn a_watch_with_nothing_pending_asks_for_no_wait_and_writes_nothing() {
        let mut watched = Watched::new();

        assert_eq!(watched.wake(0), None, "the wait is the app's to decide");
        assert_eq!(watched.step(0), None);
        assert!(watched.written.is_empty());
    }

    #[test]
    fn a_notification_is_re_queried_once_the_window_closes() {
        let mut watched = Watched::new();

        watched.feed(0, NOTIFIED);
        assert_eq!(
            watched.wake(0),
            Some(Duration::from_millis(100)),
            "the host is asked to come back when the window closes"
        );
        assert_eq!(watched.step(50), None, "still settling");
        assert_eq!(watched.requeries(), 0, "and nothing has gone out");

        assert_eq!(watched.step(100), None, "the re-query goes out, unanswered");
        assert_eq!(watched.requeries(), 1);
        assert_eq!(
            String::from_utf8_lossy(&watched.written),
            REQUERY,
            "what goes out is the re-query and nothing else"
        );
    }

    #[test]
    fn the_re_query_asks_for_the_two_colors_and_fences_nothing() {
        // Read back through the parser the replies come through, so the OSC
        // numbers and the `?` payload are checked as values. Pinned
        // independently of `REQUERY` itself: an assertion written against the
        // constant would agree with whatever the constant said.
        let mut parser = Parser::default();
        parser.parse(REQUERY.as_bytes(), false);
        let mut asked_for = Vec::new();
        while let Some(event) = parser.pop() {
            asked_for.push(event);
        }

        assert_eq!(
            asked_for,
            vec![
                Event::Osc(Osc::ChangeDynamicColors(
                    DynamicColorNumber::TextForegroundColor,
                    vec![ColorOrQuery::Query]
                )),
                Event::Osc(Osc::ChangeDynamicColors(
                    DynamicColorNumber::TextBackgroundColor,
                    vec![ColorOrQuery::Query]
                )),
            ]
        );
        assert_eq!(
            REQUERY.matches('\x07').count(),
            2,
            "both go out BEL-terminated, as at startup: {REQUERY:?}"
        );

        let fence = Csi::Device(csi::Device::RequestPrimaryDeviceAttributes).to_string();
        assert!(
            !REQUERY.contains(&fence),
            "nothing is left for a fence to decide — the terminal answered these \
             once already — and its reply would only arrive as an event no one \
             is waiting for: {REQUERY:?}"
        );
    }

    #[test]
    fn a_burst_of_notifications_costs_exactly_one_re_query() {
        // Terminals send one per palette slot, or one per transition step.
        let mut watched = Watched::new();

        for arrival in [0, 5, 12, 30, 99] {
            watched.feed(arrival, NOTIFIED);
        }
        watched.step(100);

        assert_eq!(watched.requeries(), 1, "the window collapsed all five");
    }

    #[test]
    fn a_window_already_open_is_not_pushed_back_by_more_notifications() {
        // Extending on every notification would let a terminal that keeps
        // sending hold the re-query off for as long as it liked.
        let mut watched = Watched::new();

        watched.feed(0, NOTIFIED);
        for arrival in [40, 80, 95] {
            watched.feed(arrival, NOTIFIED);
        }
        watched.step(100);

        assert_eq!(
            watched.requeries(),
            1,
            "the window still closed a hundred milliseconds after it opened"
        );
    }

    #[test]
    fn the_replies_to_a_re_query_become_the_terminals_new_colors() {
        let mut watched = Watched::new();

        watched.feed(0, NOTIFIED);
        assert_eq!(watched.step(100), None, "the re-query is out");
        watched.feed(120, RETHEMED);

        assert_eq!(
            watched.step(120),
            Some(TerminalColors {
                background: Color::Rgb(253, 246, 227),
                foreground: Color::Rgb(101, 123, 131),
                dark: None,
                palette16: None,
            }),
            "a re-query asks for colors alone, so it brings back no verdict"
        );
        assert_eq!(watched.wake(120), None, "and the watch is idle again");
    }

    #[test]
    fn half_an_answer_is_not_a_theme_and_the_watch_gives_up_on_its_own() {
        let mut watched = Watched::new();

        watched.feed(0, NOTIFIED);
        watched.step(100);
        watched.feed(150, "\x1b]11;rgb:fdfd/f6f6/e3e3\x07");

        assert_eq!(watched.step(200), None, "one color derives nothing");
        assert!(
            watched.wake(200).is_some(),
            "the answer is still owed, so the host still has a deadline"
        );
        assert_eq!(
            watched.step(1_200),
            None,
            "the terminal stopped answering, and the colors on screen stand"
        );
        assert_eq!(
            watched.wake(1_200),
            None,
            "the watch let it go rather than waiting on it forever"
        );
    }

    #[test]
    fn a_terminal_that_never_answers_at_all_is_dropped_after_one_bounded_wait() {
        let mut watched = Watched::new();

        watched.feed(0, NOTIFIED);
        watched.step(100);

        assert_eq!(watched.step(1_099), None, "still inside the wait");
        assert!(watched.wake(1_099).is_some());
        assert_eq!(watched.step(1_100), None);
        assert_eq!(watched.wake(1_100), None, "one second after the re-query");
        assert_eq!(watched.requeries(), 1, "and it was not retried");
    }

    #[test]
    fn typing_and_unrelated_reports_pass_the_watch_by() {
        let mut watched = Watched::new();

        watched.feed(0, NOTIFIED);
        watched.step(100);
        // A keystroke, a cursor-position report meant for someone else, and a
        // dynamic color nobody asked for.
        watched.feed(110, "q\x1b[10;5R\x1b]12;rgb:ffff/0000/0000\x07");

        assert_eq!(watched.step(110), None, "none of that is an answer");
        watched.feed(120, RETHEMED);
        assert!(
            watched.step(120).is_some(),
            "and none of it displaced the real one"
        );
    }

    #[test]
    fn a_second_change_mid_exchange_starts_over_rather_than_mixing_answers() {
        let mut watched = Watched::new();

        watched.feed(0, NOTIFIED);
        watched.step(100);
        // Half the answer to the first re-query, and then the user flips again.
        watched.feed(110, "\x1b]11;rgb:1a1a/1b1b/2626\x07");
        watched.feed(120, NOTIFIED);

        assert_eq!(watched.step(120), None, "the window is open again");
        assert_eq!(watched.step(220), None, "and a second re-query goes out");
        assert_eq!(watched.requeries(), 2);

        // Only one color arrives this time. If the discarded half had been kept
        // it would complete a theme out of two different terminals.
        watched.feed(230, "\x1b]10;rgb:6565/7b7b/8383\x07");
        assert_eq!(
            watched.step(230),
            None,
            "the pre-flip half was dropped with the state it described"
        );
    }

    #[test]
    fn the_notifications_own_light_dark_value_is_never_read_as_a_color() {
        // `CSI ?997;2n` says light, and it is only ever a trigger: the terminals
        // that send it disagree about whether it describes the palette or the
        // desktop.
        let mut dark_notice = Watched::new();
        dark_notice.feed(0, "\x1b[?997;1n");
        dark_notice.step(100);
        dark_notice.feed(110, RETHEMED);

        let colors = dark_notice.step(110).expect("the re-query was answered");

        assert_eq!(
            colors.background,
            Color::Rgb(253, 246, 227),
            "the background is the one the re-query brought back, not the one \
             the notification implied"
        );
        assert_eq!(colors.dark, None);
    }

    #[test]
    fn a_terminal_that_is_not_to_be_asked_is_never_written_to() {
        let mut fake = FakeTerminal::answering(ANSWERED);

        let colors = exchange(&mut fake, false).expect("refusing to ask cannot fail");

        assert_eq!(colors, None);
        assert!(
            fake.written.is_empty(),
            "not one byte goes out: {:?}",
            String::from_utf8_lossy(&fake.written)
        );
        assert!(
            fake.polls.borrow().is_empty(),
            "and nothing is waited for either"
        );
    }

    #[test]
    fn the_exchange_writes_the_whole_batch_and_reads_the_answer_back() {
        let mut fake = FakeTerminal::answering(ANSWERED);

        let colors = exchange(&mut fake, true).expect("the terminal answers");

        assert_eq!(String::from_utf8_lossy(&fake.written), QUERIES);
        assert_eq!(
            colors,
            Some(TerminalColors {
                background: Color::Rgb(26, 27, 38),
                foreground: Color::Rgb(192, 202, 245),
                dark: Some(true),
                palette16: None,
            })
        );
    }

    #[test]
    fn a_silent_terminal_is_waited_on_for_one_second_and_no_longer() {
        let mut fake = FakeTerminal::answering("");

        let colors = exchange(&mut fake, true).expect("silence is not an error");

        assert_eq!(colors, None);
        let polls = fake.polls.borrow();
        assert_eq!(
            polls.len(),
            1,
            "one bounded wait, and its empty answer ends the exchange: {polls:?}"
        );
        let waited = polls[0].expect("the wait is bounded, never indefinite");
        // Pinned against the literal second rather than against `BACKSTOP`,
        // which would leave the assertion agreeing with whatever value that
        // constant happened to take.
        assert!(
            waited > Duration::from_millis(900) && waited <= Duration::from_secs(1),
            "the first wait is the whole backstop, less the time the write took: {waited:?}"
        );
    }

    #[test]
    fn typing_during_the_exchange_is_left_for_the_event_loop() {
        // A key press that got in ahead of every reply. Reading past it is the
        // whole point of the filter: consumed here, the keystroke would be
        // neither an answer nor an event the app ever sees.
        let mut fake = FakeTerminal::answering(&format!("q{ANSWERED}"));

        let colors = exchange(&mut fake, true).expect("the terminal answers");

        assert!(colors.is_some(), "the replies were found behind the typing");
        assert_eq!(
            fake.still_pending(),
            vec![typed('q')],
            "the key press is still queued for the event loop to read"
        );
    }

    #[test]
    fn a_terminal_that_delivers_only_typing_is_never_read_from() {
        // Nothing queued is an escape sequence, so the wait has to come back
        // empty rather than wake on the typing and read something that is not
        // a reply at all.
        let mut fake = FakeTerminal::answering("qz");

        let colors = exchange(&mut fake, true).expect("typing is not an error");

        assert_eq!(colors, None);
        assert_eq!(
            fake.still_pending(),
            vec![typed('q'), typed('z')],
            "both key presses survive the exchange untouched"
        );
    }

    /// Feed `script` through termina's parser and absorb the events it yields,
    /// stopping at the fence the way [`exchange`] does.
    fn absorbed(script: &str) -> (Replies, Fence) {
        let mut parser = Parser::default();
        parser.parse(script.as_bytes(), false);
        let mut replies = Replies::default();
        while let Some(event) = parser.pop() {
            if replies.absorb(&event) == Fence::Closed {
                return (replies, Fence::Closed);
            }
        }
        (replies, Fence::Open)
    }

    /// The colors out of `script`, ignoring the fence.
    fn colors(script: &str) -> Replies {
        absorbed(script).0
    }

    /// A full, well-behaved exchange: both colors, a light/dark verdict, then
    /// the fence.
    ///
    /// The fence goes out as `CSI c` and comes back as `CSI ? … c`: the reply
    /// carries the private marker and the terminal's capability list, and the
    /// list varies per terminal, so only the shape can be matched on.
    const ANSWERED: &str = "\x1b]10;rgb:c0c0/caca/f5f5\x07\
                            \x1b]11;rgb:1a1a/1b1b/2626\x07\
                            \x1b[?997;1n\
                            \x1b[?62;1;6c";

    #[test]
    fn the_batch_asks_for_both_colors_and_nothing_else() {
        // Read back through the same parser the terminal's replies go through,
        // so the OSC numbers and the `?` payload are checked as values rather
        // than mirrored as literals. `?996n` and `CSI c` are requests termina
        // has no reply type for, so they do not appear here — the assertions
        // below cover them.
        let mut parser = Parser::default();
        parser.parse(QUERIES.as_bytes(), false);
        let mut asked_for = Vec::new();
        while let Some(event) = parser.pop() {
            asked_for.push(event);
        }

        assert_eq!(
            asked_for,
            vec![
                Event::Osc(Osc::ChangeDynamicColors(
                    DynamicColorNumber::TextForegroundColor,
                    vec![ColorOrQuery::Query]
                )),
                Event::Osc(Osc::ChangeDynamicColors(
                    DynamicColorNumber::TextBackgroundColor,
                    vec![ColorOrQuery::Query]
                )),
            ]
        );
    }

    #[test]
    fn the_light_dark_query_comes_second_to_last_and_the_fence_last() {
        // The fence only fences what precedes it, so its position is the
        // protocol: anything asked after it would be answered after the
        // exchange has already been declared over.
        let tail = format!(
            "{}{}",
            Csi::Mode(csi::Mode::QueryTheme),
            Csi::Device(csi::Device::RequestPrimaryDeviceAttributes)
        );

        assert!(
            QUERIES.ends_with(&tail),
            "the batch must end `?996n` then DA1: {QUERIES:?}"
        );
    }

    #[test]
    fn the_color_queries_go_out_bell_terminated() {
        // Every terminal accepts BEL, and the ones that mirror the terminator
        // they were sent then answer with BEL too. Two queries, two bells.
        assert_eq!(
            QUERIES.matches('\x07').count(),
            2,
            "both color queries end in BEL rather than ST: {QUERIES:?}"
        );
    }

    #[test]
    fn a_terminal_whose_output_is_captured_is_never_asked() {
        // The query bytes would land in the capture, and whatever is paging
        // that capture would race us for the reply.
        assert!(asked(Some("xterm-256color"), true));
        assert!(!asked(Some("xterm-256color"), false));
        assert!(!asked(Some("dumb"), false));
    }

    #[test]
    fn a_terminal_that_answers_everything_yields_both_colors_and_the_verdict() {
        let (replies, fence) = absorbed(ANSWERED);

        assert_eq!(fence, Fence::Closed);
        assert_eq!(
            replies.resolve(),
            Some(TerminalColors {
                foreground: Color::Rgb(192, 202, 245),
                background: Color::Rgb(26, 27, 38),
                dark: Some(true),
                palette16: None,
            })
        );
    }

    #[test]
    fn rgb_channels_are_scaled_to_eight_bits_at_every_width() {
        // `rgb:` channels are 1 to 4 hex digits each, scaled to the full range:
        // `c0c0`, `c0`, and `c` all mean the same channel value.
        for reply in [
            "\x1b]11;rgb:c0c0/caca/f5f5\x07",
            "\x1b]11;rgb:c0/ca/f5\x07",
            "\x1b]11;rgb:c00/ca0/f50\x07",
        ] {
            assert!(
                colors(reply).background.is_some(),
                "a channel width the terminal may use: {reply:?}"
            );
        }
        assert_eq!(
            colors("\x1b]11;rgb:c0/ca/f5\x07").background,
            Some(Color::Rgb(192, 202, 245))
        );
        assert_eq!(
            colors("\x1b]11;rgb:f/0/8\x07").background,
            Some(Color::Rgb(255, 0, 136)),
            "one digit per channel spans the whole range, not the bottom of it"
        );
    }

    #[test]
    fn hash_replies_are_read_as_colors_too() {
        // The other format `XParseColor` accepts. Six digits is the one width
        // where scaling and left-alignment cannot disagree, and it is the width
        // terminals actually reply in.
        assert_eq!(
            colors("\x1b]11;#1a1b26\x07").background,
            Some(Color::Rgb(26, 27, 38))
        );
        assert_eq!(
            colors("\x1b]11;#1a1a1b1b2626\x07").background,
            Some(Color::Rgb(26, 27, 38))
        );
    }

    #[test]
    fn a_three_digit_hash_reply_is_scaled_rather_than_left_aligned() {
        // The trap in the `#` format: `XParseColor` left-*aligns* its digits
        // rather than scaling them, so `#3a7` is `#3000a0007000` — (48, 160,
        // 112) — where the `rgb:` rule gives (51, 170, 119). Termina 0.3.3
        // scales both formats, and it owns the parse: the reply reaches this
        // crate already decoded, and recovering the raw payload would mean a
        // second reader racing termina's for the same bytes.
        //
        // Only six digits is exact under both rules. Nine and twelve differ by
        // at most one part in 255 (`#010000000000` is 0 scaled and 1 aligned);
        // three, below, differs by up to fifteen.
        assert_eq!(
            colors("\x1b]11;#3a7\x07").background,
            Some(Color::Rgb(51, 170, 119)),
            "this pins termina 0.3.3's parse, not `XParseColor`'s rule — if it \
             ever starts left-aligning, this failing is upstream changing its \
             answer and not a fault to hunt for here"
        );
    }

    #[test]
    fn both_string_terminators_end_a_reply() {
        let bel = colors("\x1b]11;rgb:1a1a/1b1b/2626\x07").background;
        let st = colors("\x1b]11;rgb:1a1a/1b1b/2626\x1b\\").background;

        assert_eq!(bel, Some(Color::Rgb(26, 27, 38)));
        assert_eq!(st, bel, "the terminator is not part of the answer");
    }

    #[test]
    fn foreground_and_background_land_in_their_own_slots() {
        let replies = colors("\x1b]10;rgb:c0c0/caca/f5f5\x07\x1b]11;rgb:1a1a/1b1b/2626\x07");

        assert_eq!(replies.foreground, Some(Color::Rgb(192, 202, 245)));
        assert_eq!(replies.background, Some(Color::Rgb(26, 27, 38)));
    }

    #[test]
    fn the_light_dark_query_is_read_both_ways_round() {
        assert_eq!(colors("\x1b[?997;1n").dark, Some(true));
        assert_eq!(colors("\x1b[?997;2n").dark, Some(false));
    }

    #[test]
    fn the_fence_ends_the_exchange_and_nothing_after_it_is_read() {
        // A terminal that answers the fence but not the colors has said all it
        // is going to; a reply arriving afterwards is not ours to wait for.
        let (replies, fence) = absorbed("\x1b[?6c\x1b]11;rgb:1a1a/1b1b/2626\x07\x1b[?997;1n");

        assert_eq!(fence, Fence::Closed);
        assert_eq!(replies, Replies::default());
        assert_eq!(replies.resolve(), None);
    }

    #[test]
    fn replies_the_fence_did_not_cover_are_kept() {
        let (replies, fence) = absorbed("\x1b]11;rgb:1a1a/1b1b/2626\x07\x1b[?6c");

        assert_eq!(fence, Fence::Closed);
        assert_eq!(replies.background, Some(Color::Rgb(26, 27, 38)));
    }

    #[test]
    fn unrecognized_traffic_between_replies_is_passed_over() {
        // A key press the user got in early, a cursor-position report meant for
        // someone else, and an OSC termina does not type at all — none of them
        // may end the exchange or displace a reply.
        let (replies, fence) = absorbed(
            "\x1b]10;rgb:c0c0/caca/f5f5\x07\
             q\
             \x1b[10;5R\
             \x1b]4;1;rgb:ffff/0000/0000\x07\
             \x1b]11;rgb:1a1a/1b1b/2626\x07\
             \x1b[?62;1;6c",
        );

        assert_eq!(fence, Fence::Closed);
        assert!(replies.resolve().is_some());
    }

    #[test]
    fn a_terminal_that_says_nothing_resolves_to_nothing() {
        let (replies, fence) = absorbed("");

        assert_eq!(fence, Fence::Open, "no fence, so the backstop is what ends");
        assert_eq!(replies.resolve(), None);
    }

    #[test]
    fn a_light_dark_verdict_with_no_colors_behind_it_is_not_a_theme() {
        let replies = colors("\x1b[?997;2n");

        assert_eq!(replies.dark, Some(false));
        assert_eq!(replies.resolve(), None);
    }

    #[test]
    fn one_color_without_the_other_is_not_a_theme() {
        assert_eq!(colors("\x1b]11;rgb:1a1a/1b1b/2626\x07").resolve(), None);
        assert_eq!(colors("\x1b]10;rgb:c0c0/caca/f5f5\x07").resolve(), None);
    }

    #[test]
    fn a_query_echoed_back_instead_of_answered_carries_no_color() {
        assert_eq!(colors("\x1b]11;?\x07").background, None);
    }

    #[test]
    fn a_dynamic_color_this_query_never_asked_for_is_not_a_theme_color() {
        // OSC 12 is the cursor color, and OSC 17 the selection highlight —
        // both typed by termina, neither asked for here. Taking the first
        // dynamic color that turns up would let an unrelated answer, or a
        // reply to somebody else's earlier query, become the screen.
        let stray = colors(
            "\x1b]12;rgb:ffff/0000/0000\x07\
             \x1b]17;rgb:0000/ffff/0000\x07",
        );

        assert_eq!(stray, Replies::default());
        assert_eq!(stray.resolve(), None);
    }

    #[test]
    fn only_the_terminals_that_can_answer_are_asked() {
        for term in ["xterm-256color", "foot", "wezterm", "tmux-256color"] {
            assert!(asked(Some(term), true), "{term} answers escape queries");
        }
        // Both refusals are prefixes, so both need a name that only a prefix
        // match catches: `screen.xterm-256color` and `Eterm-color`.
        for term in [
            "dumb",
            "",
            "screen",
            "screen.xterm-256color",
            "Eterm",
            "Eterm-color",
        ] {
            assert!(!asked(Some(term), true), "{term} must not be asked");
        }
        assert!(!asked(None, true), "an unset TERM promises nothing");
    }
}
