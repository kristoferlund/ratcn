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
//! at a one-second backstop, which is also what catches the terminals that
//! answered the light/dark query *after* the fence.
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
        ColorOrQuery, Csi, Duration, DynamicColorNumber, Event, Fence, Osc, QUERIES, Replies,
        Terminal, TerminalColors, asked, csi, exchange, io,
    };
    use ratatui::style::Color;
    use std::{cell::RefCell, collections::VecDeque};
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
