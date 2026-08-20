//! Following a terminal that reports its own theme changes.
//!
//! [`Watch`] is what a [`Session`](super::Session) runs behind its event
//! source: it notices a change notification, waits for the burst to settle,
//! asks the terminal for its colors again, and hands back the answer. The app
//! sees none of that — only the
//! [`ThemeChanged`](super::SessionEvent::ThemeChanged) that comes out the end.

use std::{
    io,
    time::{Duration, Instant},
};

use termina::{
    Event,
    escape::csi::{self, Csi},
};

use super::query::{BACKSTOP, Replies, TerminalColors};

/// The re-query a notification triggers: the two colors, and no fence.
///
/// Nothing is left for a fence to decide. The terminal answered these once
/// already — that is what got the subscription switched on — so what bounds
/// this exchange is [`Watch`]'s own deadline, not a reply that says the
/// rest will never come.
const REQUERY: &str = "\x1b]10;?\x07\x1b]11;?\x07";

/// How long notifications are collected before the colors are asked for.
///
/// Terminals send these in bursts — one per palette slot on some, one per
/// transition step on others — and each one would otherwise cost a round trip
/// and a repaint.
const DEBOUNCE: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub(super) struct Watch {
    state: Watching,
    /// What the terminal last said its colors were. A re-query that lands back
    /// on these is not a change and is not reported.
    last: Option<TerminalColors>,
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

impl Watch {
    /// A watch with nothing pending.
    /// `known` is what the startup query answered, so a re-query that repeats
    /// it reports nothing.
    pub(super) const fn new(known: Option<TerminalColors>) -> Self {
        Self {
            state: Watching::Idle,
            last: known,
        }
    }

    /// Note one event read from the terminal.
    ///
    /// Events that are neither a change notification nor a reply this watch is
    /// waiting for are ignored, so a host can hand it everything it reads
    /// without sorting first — and hand the same event on to the app, which is
    /// what keeps a keystroke that arrived mid-exchange a keystroke.
    pub(super) fn absorb(&mut self, event: &Event, now: Instant) {
        match event {
            // The notification carries a light/dark value of its own, and it is
            // deliberately not read: terminals disagree over whether it
            // describes the palette or the desktop, so it says *that* something
            // changed and never *what* it changed to. The colors come from the
            // re-query.
            //
            // A stray `CSI ? 997 ; N n` — a startup verdict that arrived after
            // the fence — is indistinguishable from a notification. It costs
            // one re-query, and the answer reports no change.
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
    pub(super) fn wake(&self, now: Instant) -> Option<Duration> {
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
    pub(super) fn step<W: io::Write>(
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
                    return Ok((self.last.replace(colors) != Some(colors)).then_some(colors));
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
pub(super) fn deadline(now: Instant, span: Duration) -> Instant {
    now.checked_add(span).unwrap_or(now)
}

/// The watch, driven at instants the test names rather than at the clock's.
#[cfg(test)]
mod tests {
    use super::{Duration, Instant, REQUERY, TerminalColors, Watch};
    use ratatui::style::Color;
    use termina::{
        Event, Parser,
        escape::{
            csi::{self, Csi},
            osc::{ColorOrQuery, DynamicColorNumber, Osc},
        },
    };

    /// Everything the watch is told, at a time the test names. No clock is read
    /// anywhere in here: `t0` is a fixed origin and every deadline is an offset
    /// from it, so the state machine is exercised at exact instants.
    struct Watched {
        watch: Watch,
        written: Vec<u8>,
        t0: Instant,
    }

    impl Watched {
        /// A watch that already knows what `script` says the colors are.
        fn started_from(script: &str) -> Self {
            let mut parser = Parser::default();
            parser.parse(script.as_bytes(), false);
            let mut replies = super::Replies::default();
            while let Some(event) = parser.pop() {
                let _fence = replies.absorb(&event);
            }
            let mut watched = Self::new();
            watched.watch = Watch::new(replies.resolve());
            watched
        }

        fn new() -> Self {
            Self {
                watch: Watch::new(None),
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
    fn a_re_query_that_lands_on_the_colours_already_showing_is_not_a_change() {
        // The late verdict case: a `?997` that was really the startup query's
        // answer costs a round trip, and the terminal says what it said before.
        let mut watched = Watched::started_from(RETHEMED);

        watched.feed(0, NOTIFIED);
        watched.step(100);
        watched.feed(110, RETHEMED);

        assert_eq!(watched.step(110), None, "nothing moved, so nothing is news");
        assert_eq!(watched.requeries(), 1, "the round trip still happened");

        // A foreground the user changed without touching the background is a
        // change: comparing backgrounds alone would drop it.
        watched.feed(200, NOTIFIED);
        watched.step(300);
        watched.feed(
            310,
            "\x1b]10;rgb:2d2d/3737/3b3b\x07\x1b]11;rgb:fdfd/f6f6/e3e3\x07",
        );
        assert!(
            watched.step(310).is_some(),
            "the background stood still and the foreground did not"
        );

        // And so is a background the user changed on its own.
        watched.feed(400, NOTIFIED);
        watched.step(500);
        watched.feed(
            510,
            "\x1b]10;rgb:2d2d/3737/3b3b\x07\x1b]11;rgb:1a1a/1b1b/2626\x07",
        );
        assert!(watched.step(510).is_some());
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
    }
}
