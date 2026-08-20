//! Following a terminal that reports its own theme changes.
//!
//! [`Watch`] is what a [`Session`](super::Session) runs behind its event
//! source: it notices a change notification, waits for the burst to settle,
//! asks the terminal for its colors again, and hands back the answer. It asks
//! again over the seconds that follow, so a recolour still in progress is
//! reached at the value it arrives at. The app sees none of that — only the
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

/// The re-asks a trigger schedules after its own, measured from the trigger.
///
/// A recolour that starts at the moment the trigger fires answers the first
/// round trip with the colors it is in the middle of replacing. Asking again a
/// second later, and once more two seconds after that, is what reaches the
/// answer a desktop script arrives at in its own time.
pub(super) const SETTLE: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];

/// How long a session goes without events before the next one counts as the
/// user coming back to it.
///
/// Longer than the pauses inside continuous use — reading a screen, moving
/// between fields, waiting out a command — so a gap this wide places the user
/// somewhere else, which is where a terminal gets recoloured on the compositors
/// that report no focus at all.
pub(super) const IDLE: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(super) struct Watch {
    state: Watching,
    /// What the terminal last said its colors were. A re-query that lands back
    /// on these is not a change and is not reported.
    last: Option<TerminalColors>,
    /// The settle sequence the current trigger opened, while it still owes a
    /// re-ask.
    settle: Option<Settle>,
    /// When the terminal last produced an event of any kind, starting at the
    /// instant the session opened.
    seen: Instant,
}

/// The re-asks one trigger still owes: where they are measured from, and how
/// many of [`SETTLE`] have gone out.
#[derive(Debug, Clone, Copy)]
struct Settle {
    from: Instant,
    spent: usize,
}

/// Where the watch is in the notification-to-new-colors round trip.
#[derive(Debug, Clone, Copy)]
enum Watching {
    /// Nothing has happened, and nothing is owed.
    Idle,
    /// A change was reported. Everything that arrives before `due` is the same
    /// change still settling.
    Collapsing { due: Instant },
    /// The colors have been asked for again, and the answer is owed by
    /// `deadline`.
    Awaiting { deadline: Instant, replies: Replies },
    /// The exchange is over and the trigger still owes a re-ask, due at `due`.
    Settling { due: Instant },
}

impl Watch {
    /// A watch with nothing pending, opened at `opened`.
    /// `known` is what the startup query answered, so a re-query that repeats
    /// it reports nothing.
    pub(super) const fn new(known: Option<TerminalColors>, opened: Instant) -> Self {
        Self {
            state: Watching::Idle,
            last: known,
            settle: None,
            seen: opened,
        }
    }

    /// Note one event read from the terminal.
    ///
    /// The watch takes what it needs of each event — a trigger, a reply it is
    /// waiting for, or the instant it arrived — and leaves the event itself
    /// alone, so a host hands it everything it reads without sorting first and
    /// hands the same event on to the app, which is what keeps a keystroke that
    /// arrived mid-exchange a keystroke.
    pub(super) fn absorb(&mut self, event: &Event, now: Instant) {
        let quiet = now.saturating_duration_since(self.seen);
        self.seen = now;
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
            //
            // Focus returning to the window is the other trigger. A terminal
            // recoloured by something other than its own theme selector — a
            // desktop that injects OSC 10/11 into every pty — reports no
            // change, and mode 2031 does not exist at all on some terminals.
            // Asking again when the user comes back covers both: the colors
            // answer for themselves, and an unchanged palette ends the round
            // trip without an event.
            Event::Csi(Csi::Mode(csi::Mode::ReportTheme(_))) | Event::FocusIn => {
                self.trigger(now);
            }
            // Anything collected before a trigger described the theme the
            // terminal has just left, so it is dropped with the state.
            event => {
                if let Watching::Awaiting { replies, .. } = &mut self.state {
                    let _fence = replies.absorb(event);
                }
                // Input resuming after a long quiet is the third trigger, and
                // the one that needs nothing of the terminal: the user was
                // away, and the compositors that recolour a window without
                // giving it a focus event are covered by the same round trip.
                // The comparison happens on an event that arrived, so a session
                // nobody is using costs no wake-up at all.
                //
                // Only from rest: a sequence already running asks again on its
                // own schedule, and a settle runs shorter than a quiet spell.
                else if matches!(self.state, Watching::Idle)
                    && quiet >= IDLE
                    && !event.is_escape()
                {
                    self.trigger(now);
                }
            }
        }
    }

    /// Open a collapse window, and the settle sequence that follows it.
    ///
    /// A window already open is not pushed back, and keeps the sequence it
    /// started. Extending it on every trigger would let a terminal that keeps
    /// sending hold the re-query off indefinitely; collapsing into a fixed
    /// window instead guarantees the round trip happens, and a trigger that
    /// lands after it opens a window — and a sequence — of its own.
    fn trigger(&mut self, now: Instant) {
        if matches!(self.state, Watching::Collapsing { .. }) {
            return;
        }
        self.state = Watching::Collapsing {
            due: deadline(now, DEBOUNCE),
        };
        self.settle = Some(Settle {
            from: now,
            spent: 0,
        });
    }

    /// When the current trigger's next re-ask comes due, while it owes one.
    fn settle_due(&self) -> Option<Instant> {
        let settle = self.settle?;
        Some(deadline(settle.from, *SETTLE.get(settle.spent)?))
    }

    /// Where the watch goes when an exchange ends: on to the re-ask the trigger
    /// still owes, or to rest.
    fn rest(&mut self) {
        if let Some(due) = self.settle_due() {
            self.state = Watching::Settling { due };
            return;
        }
        self.settle = None;
        self.state = Watching::Idle;
    }

    /// Write the re-query, and start waiting out its answer.
    fn ask<W: io::Write>(&mut self, out: &mut W, now: Instant) -> io::Result<()> {
        out.write_all(REQUERY.as_bytes())?;
        out.flush()?;
        self.state = Watching::Awaiting {
            deadline: deadline(now, BACKSTOP),
            replies: Replies::default(),
        };
        Ok(())
    }

    /// How long the host may wait before calling [`step`](Self::step) again, or
    /// [`None`] when nothing is pending and the wait is the app's to decide.
    pub(super) fn wake(&self, now: Instant) -> Option<Duration> {
        match self.state {
            Watching::Idle => None,
            Watching::Collapsing { due: at }
            | Watching::Settling { due: at }
            | Watching::Awaiting { deadline: at, .. } => Some(at.saturating_duration_since(now)),
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
        match self.state {
            Watching::Idle => Ok(None),
            Watching::Collapsing { due } => {
                if now >= due {
                    self.ask(out, now)?;
                }
                Ok(None)
            }
            Watching::Settling { due } => {
                if now >= due {
                    if let Some(settle) = &mut self.settle {
                        settle.spent += 1;
                    }
                    self.ask(out, now)?;
                }
                Ok(None)
            }
            Watching::Awaiting { deadline, replies } => {
                if let Some(colors) = replies.resolve() {
                    self.rest();
                    return Ok((self.last.replace(colors) != Some(colors)).then_some(colors));
                }
                if now >= deadline {
                    self.rest();
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
    use super::{DEBOUNCE, Duration, IDLE, Instant, REQUERY, SETTLE, TerminalColors, Watch};
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
            watched.watch = Watch::new(replies.resolve(), watched.t0);
            watched
        }

        fn new() -> Self {
            let t0 = Instant::now();
            Self {
                watch: Watch::new(None, t0),
                written: Vec::new(),
                t0,
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

    /// The same shape of reply, carrying the dark colors a light re-theme
    /// replaces.
    const DARK: &str = "\x1b]10;rgb:c0c0/caca/f5f5\x07\x1b]11;rgb:1a1a/1b1b/2626\x07";

    /// A change notification. The value it carries is never read.
    const NOTIFIED: &str = "\x1b[?997;2n";

    /// The window regaining focus, and losing it.
    const FOCUSED: &str = "\x1b[I";
    const UNFOCUSED: &str = "\x1b[O";

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
    fn focus_returning_to_the_window_asks_the_terminal_again() {
        // The terminals that matter here report no change of their own: a
        // desktop recoloured them from outside, or they have no mode 2031.
        let mut watched = Watched::new();

        watched.feed(0, FOCUSED);
        assert_eq!(
            watched.wake(0),
            Some(Duration::from_millis(100)),
            "the window opens, as it does for a notification"
        );
        watched.step(100);
        assert_eq!(watched.requeries(), 1);

        watched.feed(110, RETHEMED);
        assert!(watched.step(110).is_some(), "and the answer is a theme");
    }

    #[test]
    fn losing_focus_asks_the_terminal_nothing() {
        let mut watched = Watched::new();

        watched.feed(0, UNFOCUSED);

        assert_eq!(watched.wake(0), None, "nothing is owed");
        watched.step(100);
        assert_eq!(watched.requeries(), 0);
    }

    #[test]
    fn alt_tabbing_costs_one_re_query() {
        // Focus arrives once per switch, and a user switching windows produces
        // a burst of them.
        let mut watched = Watched::new();

        for arrival in [0, 5, 40, 80, 99] {
            watched.feed(arrival, FOCUSED);
        }
        watched.step(100);

        assert_eq!(watched.requeries(), 1, "the window collapsed all five");
    }

    #[test]
    fn a_notification_and_a_focus_in_the_same_window_cost_one_re_query() {
        let mut watched = Watched::new();

        watched.feed(0, NOTIFIED);
        watched.feed(20, FOCUSED);
        watched.feed(60, NOTIFIED);
        watched.step(100);

        assert_eq!(watched.requeries(), 1);
    }

    #[test]
    fn focus_on_a_terminal_that_did_not_change_costs_nothing_but_the_round_trip() {
        // Alt-tabbing back and forth is free: the colors answer for themselves.
        let mut watched = Watched::started_from(RETHEMED);

        watched.feed(0, FOCUSED);
        watched.step(100);
        watched.feed(110, RETHEMED);

        assert_eq!(watched.step(110), None, "nothing moved, so nothing is news");
        assert_eq!(watched.requeries(), 1);
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
        assert_eq!(
            watched.wake(120),
            Some(Duration::from_millis(880)),
            "and what is still owed is the re-ask a second after the trigger"
        );
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
            watched.requeries(),
            1,
            "the exchange was let go rather than retried inside its own wait"
        );

        // The re-ask the trigger owed goes out on its own schedule, and starts
        // from nothing.
        watched.step(1_200);
        assert_eq!(watched.requeries(), 2);
        watched.feed(1_210, "\x1b]10;rgb:6565/7b7b/8383\x07");
        assert_eq!(
            watched.step(1_210),
            None,
            "the half collected before the wait ran out went with it"
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
        assert_eq!(
            watched.requeries(),
            1,
            "one second after the re-query it is let go, never retried inside \
             its own wait"
        );

        // The sequence the trigger opened runs its course against the silence,
        // and each re-ask costs the same one bounded wait.
        for at in [1_100, 2_100, 3_000, 4_000] {
            watched.step(at);
        }

        assert_eq!(watched.requeries(), 3, "the trigger's, and its two re-asks");
        assert_eq!(
            watched.wake(4_000),
            None,
            "and then a terminal that says nothing is left alone"
        );
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
    fn a_focus_mid_exchange_starts_over_rather_than_mixing_answers() {
        // Focus and a notification share one arm, so what a second notification
        // does to a half-collected answer, focus does too.
        let mut watched = Watched::new();

        watched.feed(0, NOTIFIED);
        watched.step(100);
        // Half the answer to the first re-query, and then the window comes back.
        watched.feed(110, "\x1b]11;rgb:1a1a/1b1b/2626\x07");
        watched.feed(120, FOCUSED);

        assert_eq!(watched.step(120), None, "the window is open again");
        assert_eq!(watched.step(220), None, "and a second re-query goes out");
        assert_eq!(watched.requeries(), 2);

        // Only one color arrives this time. If the discarded half had been kept
        // it would complete a theme out of two different terminals.
        watched.feed(230, "\x1b]10;rgb:6565/7b7b/8383\x07");
        assert_eq!(
            watched.step(230),
            None,
            "the pre-focus half was dropped with the state it described"
        );
    }

    #[test]
    fn a_trigger_asks_again_a_second_later_and_three_seconds_later() {
        let mut watched = Watched::started_from(DARK);

        watched.feed(0, NOTIFIED);
        watched.step(100);
        watched.feed(110, DARK);
        watched.step(110);
        assert_eq!(watched.requeries(), 1, "the trigger's own round trip");

        assert_eq!(
            watched.wake(110),
            Some(Duration::from_millis(890)),
            "the host is asked back for the first re-ask, a second after the \
             trigger"
        );
        watched.step(999);
        assert_eq!(watched.requeries(), 1, "still inside the first second");
        watched.step(1_000);
        assert_eq!(watched.requeries(), 2);

        watched.feed(1_010, DARK);
        watched.step(1_010);
        watched.step(2_999);
        assert_eq!(watched.requeries(), 2, "still inside the third second");
        watched.step(3_000);
        assert_eq!(watched.requeries(), 3, "and the last re-ask goes out");
    }

    #[test]
    fn the_watch_rests_once_the_settle_sequence_is_spent() {
        let mut watched = Watched::started_from(DARK);

        watched.feed(0, NOTIFIED);
        for (asked, answered) in [(100, 110), (1_000, 1_010), (3_000, 3_010)] {
            watched.step(asked);
            watched.feed(answered, DARK);
            watched.step(answered);
        }

        assert_eq!(watched.requeries(), 3, "the trigger's, and its two re-asks");
        assert_eq!(
            watched.wake(3_010),
            None,
            "the sequence is spent, and the wait is the app's again"
        );
        watched.step(60_000);
        assert_eq!(watched.requeries(), 3, "nothing goes out after it rests");
    }

    #[test]
    fn a_settle_re_ask_that_lands_on_the_colours_already_showing_says_nothing() {
        // Alt-tabbing back to a terminal nobody touched costs three round trips
        // and no frame.
        let mut watched = Watched::started_from(DARK);

        watched.feed(0, FOCUSED);
        for (asked, answered) in [(100, 110), (1_000, 1_010), (3_000, 3_010)] {
            watched.step(asked);
            watched.feed(answered, DARK);
            assert_eq!(
                watched.step(answered),
                None,
                "nothing moved at {answered} ms, so nothing is news"
            );
        }

        assert_eq!(
            watched.requeries(),
            3,
            "and each of the three round trips happened: silence here is the \
             answer being the same, not the asking being skipped"
        );
    }

    #[test]
    fn a_recolour_that_starts_as_the_trigger_fires_is_caught_by_the_settle() {
        // The selector overlay closes, focus comes back, and the desktop's
        // re-theme script starts at that same instant: the first round trip
        // reads the colors it is in the middle of replacing.
        let mut watched = Watched::started_from(DARK);

        watched.feed(0, FOCUSED);
        watched.step(100);
        watched.feed(110, DARK);
        assert_eq!(
            watched.step(110),
            None,
            "the terminal has not repainted yet, so its answer is the old one"
        );

        // The recolour lands eight hundred milliseconds later, unannounced.
        watched.step(1_000);
        watched.feed(1_010, RETHEMED);

        assert!(
            watched.step(1_010).is_some(),
            "the re-ask a second after the trigger is what reaches it"
        );
        assert_eq!(watched.requeries(), 2);
    }

    #[test]
    fn a_fresh_trigger_during_a_settle_sequence_starts_the_sequence_over() {
        let mut watched = Watched::started_from(DARK);

        watched.feed(0, NOTIFIED);
        watched.step(100);
        watched.feed(110, DARK);
        watched.step(110);

        // A second flip while the first sequence is waiting out its re-ask.
        watched.feed(500, NOTIFIED);
        watched.step(600);
        watched.feed(610, DARK);
        watched.step(610);
        assert_eq!(watched.requeries(), 2, "the new trigger's own round trip");

        watched.step(1_000);
        assert_eq!(
            watched.requeries(),
            2,
            "the first sequence went with the trigger it belonged to"
        );
        watched.step(1_500);
        assert_eq!(
            watched.requeries(),
            3,
            "and the re-asks are measured from the trigger that is current"
        );
        watched.feed(1_510, DARK);
        watched.step(1_510);
        watched.step(3_000);
        assert_eq!(watched.requeries(), 3, "still the new sequence's schedule");
        watched.step(3_500);
        assert_eq!(watched.requeries(), 4);
    }

    #[test]
    fn five_triggers_inside_one_window_cost_one_settle_sequence() {
        // A user switching windows produces a burst of focus events, and the
        // whole burst is one return to one terminal.
        let mut watched = Watched::started_from(DARK);

        for arrival in [0, 5, 40, 80, 99] {
            watched.feed(arrival, FOCUSED);
        }
        for (asked, answered) in [(100, 110), (1_000, 1_010), (3_000, 3_010)] {
            watched.step(asked);
            watched.feed(answered, DARK);
            watched.step(answered);
        }

        assert_eq!(
            watched.requeries(),
            3,
            "one round trip and one pair of re-asks, not five of each"
        );
        assert_eq!(watched.wake(3_010), None, "and then it rests");
    }

    #[test]
    fn typing_after_a_long_quiet_asks_the_terminal_again() {
        // The compositors that recolour a window without giving it a focus
        // event leave the user's return as the only signal there is.
        let mut brief = Watched::new();
        brief.feed(4_900, "q");

        assert_eq!(
            brief.wake(4_900),
            None,
            "four and nine tenths of a second is a pause inside one sitting"
        );

        let mut long = Watched::new();
        long.feed(5_100, "q");

        assert_eq!(
            long.wake(5_100),
            Some(DEBOUNCE),
            "five and a tenth places the user somewhere else, and coming back \
             opens the same window a notification does"
        );
        long.step(5_200);
        assert_eq!(long.requeries(), 1);
        long.feed(5_210, RETHEMED);
        assert!(
            long.step(5_210).is_some(),
            "and the colors it brings back are a theme like any other"
        );
    }

    #[test]
    fn typing_through_a_session_asks_the_terminal_nothing() {
        let mut watched = Watched::new();

        for arrival in [0, 3_000, 6_000, 9_000, 12_000] {
            watched.feed(arrival, "q");
        }

        assert_eq!(
            watched.wake(12_000),
            None,
            "no gap in twelve seconds of use was a quiet one"
        );
        watched.step(12_100);
        assert_eq!(watched.requeries(), 0);
    }

    #[test]
    fn a_session_nobody_is_using_costs_no_wake_up_at_all() {
        // The quiet is measured on an event that arrived, so an idle session
        // asks the host for no deadline and the host blocks.
        let watched = Watched::new();

        assert_eq!(watched.wake(60_000), None, "a minute in, nothing is owed");
    }

    #[test]
    fn a_terminals_own_answer_after_a_long_quiet_is_the_terminal_talking() {
        // A stray report is not the user coming back to the window.
        let mut watched = Watched::new();

        watched.feed(9_000, "\x1b[?62;1;6c");

        assert_eq!(watched.wake(9_000), None, "nothing is owed");
        watched.step(9_100);
        assert_eq!(watched.requeries(), 0);
    }

    #[test]
    fn a_quiet_spell_during_a_settle_sequence_opens_no_window_of_its_own() {
        // Every re-ask lands inside the gap it would take to open a quiet
        // spell, so the state below is reached by naming it: what it pins is
        // that the input trigger answers to the resting state alone.
        assert!(
            SETTLE.iter().all(|span| *span < IDLE),
            "a settle sequence runs shorter than a quiet spell: {SETTLE:?}"
        );

        let mut watched = Watched::started_from(DARK);
        watched.feed(0, NOTIFIED);
        watched.step(100);
        watched.feed(110, DARK);
        watched.step(110);

        watched.watch.seen = watched.at(0);
        watched.feed(6_000, "q");

        assert_eq!(
            watched.wake(6_000),
            Some(Duration::ZERO),
            "the re-ask the trigger still owes is the only deadline, and it is \
             already due"
        );
        watched.step(6_000);
        assert_eq!(
            watched.requeries(),
            2,
            "the sequence asked once, on its own schedule"
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
