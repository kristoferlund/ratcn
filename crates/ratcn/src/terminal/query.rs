//! The startup query: asking the terminal what colors it is painting with,
//! and whether it shows 24-bit color.
//!
//! One batched exchange, fenced, before the app has read an event. See the
//! [module documentation](super) for where it sits in a session.

use std::{
    env, io,
    io::IsTerminal as _,
    time::{Duration, Instant},
};

use ratatui::style::Color;
use termina::{
    Event, Terminal,
    escape::{
        csi::{self, Csi, Sgr},
        dcs::{Dcs, DcsResponse},
        osc::{ColorOrQuery, DynamicColorNumber, Osc},
    },
    style::RgbColor,
};

/// The two color queries, BEL-terminated; replies are accepted with either
/// terminator. The startup batch is these followed by [`FENCE`]; a re-query
/// is these alone.
pub(super) const COLOR_QUERIES: &str = "\x1b]10;?\x07\x1b]11;?\x07";

/// Neovim's probe for 24-bit color: set an rgb background, ask for the
/// current SGR with DECRQSS, and reset. A terminal that holds the color reports
/// it back; one that does not reports something else or nothing. The leading
/// reset keeps the reply to the one attribute.
const RGB_PROBE: &str = "\x1b[0m\x1b[48;2;1;2;3m\x1bP$qm\x1b\\\x1b[0m";

/// The background [`RGB_PROBE`] sets, as a reply reports it.
const PROBED: (u8, u8, u8) = (1, 2, 3);

/// `CSI c` — primary device attributes — written after the color queries.
/// Every terminal answers it, so its reply says the colors are not coming.
const FENCE: &str = "\x1b[c";

/// How long the whole exchange may take, fence or no fence. The watch reuses
/// it as the deadline for a re-query's answer.
pub(super) const BACKSTOP: Duration = Duration::from_secs(1);

/// What the terminal reported about its own colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TerminalColors {
    /// The terminal's default background, from OSC 11.
    pub(super) background: Color,
    /// The terminal's default foreground, from OSC 10.
    pub(super) foreground: Color,
}

impl TerminalColors {
    /// The theme these colors solve to.
    pub(super) fn theme(self) -> crate::Theme {
        crate::Theme::adaptive(self.background, self.foreground, None)
    }
}

/// What the startup exchange asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Ask {
    /// The background and foreground, for an adaptive theme.
    pub(super) colors: bool,
    /// Whether the terminal shows 24-bit color, with [`RGB_PROBE`].
    pub(super) rgb: bool,
}

/// What the terminal answered.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Answer {
    /// Both colors, once the terminal supplied them. Callers paint with a
    /// preset otherwise.
    pub(super) colors: Option<TerminalColors>,
    /// Whether the terminal reported the probe's rgb background back.
    pub(super) rgb: bool,
}

/// Ask `terminal` what `ask` names, and wait out the answer.
///
/// Call it with the terminal in raw mode, before anything has read an event
/// from it.
///
/// # Gates
///
/// The query is skipped, without writing a byte, when `TERM` is unset, empty,
/// `dumb`, GNU screen, or Eterm, and when stdout is not a terminal.
///
/// # Errors
///
/// Returns an I/O error if the query cannot be written, or if reading the
/// terminal fails. A terminal that stays silent answers nothing.
pub(super) fn query<T: Terminal>(terminal: &mut T, ask: Ask) -> io::Result<Answer> {
    exchange(
        terminal,
        asked(env::var("TERM").ok().as_deref(), io::stdout().is_terminal()),
        ask,
    )
}

/// The exchange itself. `asked` is the gate's verdict, passed in because
/// whether stdout is a terminal is not something a test can arrange.
fn exchange<T: Terminal>(terminal: &mut T, asked: bool, ask: Ask) -> io::Result<Answer> {
    if !asked || !(ask.colors || ask.rgb) {
        return Ok(Answer::default());
    }

    if ask.colors {
        terminal.write_all(COLOR_QUERIES.as_bytes())?;
    }
    if ask.rgb {
        terminal.write_all(RGB_PROBE.as_bytes())?;
    }
    terminal.write_all(FENCE.as_bytes())?;
    terminal.flush()?;

    let started = Instant::now();
    let mut replies = Replies::default();
    while let Some(remaining) = BACKSTOP.checked_sub(started.elapsed()) {
        // Filtering on escape sequences leaves key presses buffered for the
        // event loop rather than eating them to get at the replies.
        if !terminal.poll(Event::is_escape, Some(remaining))? {
            break;
        }
        if replies.absorb(&terminal.read(Event::is_escape)?) {
            break;
        }
    }

    Ok(Answer {
        colors: replies.resolve(),
        rgb: replies.rgb,
    })
}

/// Whether this terminal is asked at all. See [`query`]'s gates.
fn asked(term: Option<&str>, stdout_is_tty: bool) -> bool {
    stdout_is_tty
        && match term {
            None | Some("" | "dumb") => false,
            Some(term) => !term.starts_with("screen") && !term.starts_with("Eterm"),
        }
}

/// The replies collected so far.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Replies {
    background: Option<Color>,
    foreground: Option<Color>,
    rgb: bool,
}

impl Replies {
    /// Take one event from the terminal, and report whether it was the fence.
    ///
    /// Anything else — a stray report, a mouse event, a key press that slipped
    /// past the filter — is passed over: the exchange ends at the fence or at
    /// the backstop, never at the first thing it did not recognize.
    pub(super) fn absorb(&mut self, event: &Event) -> bool {
        match event {
            Event::Osc(Osc::ChangeDynamicColors(number, colors)) => {
                if let Some(color) = reported(colors) {
                    match number {
                        DynamicColorNumber::TextForegroundColor => self.foreground = Some(color),
                        DynamicColorNumber::TextBackgroundColor => self.background = Some(color),
                        _ => {}
                    }
                }
                false
            }
            // Only the private form, `CSI ? … c`, which is what every terminal
            // actually replies with. A bare `CSI … c` reply would miss the
            // fence and cost the backstop's second — a stall, not a wrong
            // answer, since the replies collected so far still stand.
            Event::Csi(Csi::Device(csi::Device::DeviceAttributes(()))) => true,
            Event::Dcs(Dcs::Response {
                is_request_valid: true,
                value: DcsResponse::GraphicRendition(sgrs),
            }) => {
                self.rgb |= holds_probe(sgrs);
                false
            }
            _ => false,
        }
    }

    /// Both colors or nothing.
    ///
    /// A theme is derived from the pair, so half of it derives nothing. A
    /// light/dark verdict that arrived without them could still pick between
    /// two presets by polarity — but that is a preset either way, which is what
    /// the caller's fallback already is.
    pub(super) fn resolve(self) -> Option<TerminalColors> {
        Some(TerminalColors {
            background: self.background?,
            foreground: self.foreground?,
        })
    }
}

/// Whether a DECRQSS reply is the probe's background and nothing else, as
/// Neovim reads it: that one attribute, after a reset at most.
///
/// Only the colon form reaches here: termina 0.3.3 splits a reply on `;`, so
/// one written `48;2;1;2;3` is malformed to it and dropped, and that terminal
/// keeps the environment's depth.
fn holds_probe(sgrs: &[Sgr]) -> bool {
    let (r, g, b) = PROBED;
    let probe = Sgr::Background(RgbColor::new(r, g, b).into());
    matches!(sgrs, [only] | [Sgr::Reset, only] if *only == probe)
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
        Ask, COLOR_QUERIES, ColorOrQuery, Csi, Duration, DynamicColorNumber, Event, FENCE, Osc,
        RGB_PROBE, Replies, TerminalColors, asked, csi, exchange,
    };

    /// The adaptive theme's question alone.
    const COLORS: Ask = Ask {
        colors: true,
        rgb: false,
    };
    use crate::terminal::fake::FakeTerminal;
    use ratatui::style::Color;
    use termina::Parser;

    fn typed(code: char) -> Event {
        Event::Key(termina::event::KeyEvent::from(
            termina::event::KeyCode::Char(code),
        ))
    }

    #[test]
    fn a_terminal_that_is_not_to_be_asked_is_never_written_to() {
        let mut fake = FakeTerminal::scripted(ANSWERED);

        let colors = exchange(&mut fake, false, COLORS)
            .expect("refusing to ask cannot fail")
            .colors;

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
        let mut fake = FakeTerminal::scripted(ANSWERED);

        let colors = exchange(&mut fake, true, COLORS)
            .expect("the terminal answers")
            .colors;

        assert_eq!(
            String::from_utf8_lossy(&fake.written),
            format!("{COLOR_QUERIES}{FENCE}"),
            "the colors first, and the fence after what it fences"
        );
        assert_eq!(
            colors,
            Some(TerminalColors {
                background: Color::Rgb(26, 27, 38),
                foreground: Color::Rgb(192, 202, 245),
            })
        );
    }

    #[test]
    fn a_silent_terminal_is_waited_on_for_one_second_and_no_longer() {
        let mut fake = FakeTerminal::scripted("");

        let colors = exchange(&mut fake, true, COLORS)
            .expect("silence is not an error")
            .colors;

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
        let mut fake = FakeTerminal::scripted(&format!("q{ANSWERED}"));

        let colors = exchange(&mut fake, true, COLORS)
            .expect("the terminal answers")
            .colors;

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
        let mut fake = FakeTerminal::scripted("qz");

        let colors = exchange(&mut fake, true, COLORS)
            .expect("typing is not an error")
            .colors;

        assert_eq!(colors, None);
        assert_eq!(
            fake.still_pending(),
            vec![typed('q'), typed('z')],
            "both key presses survive the exchange untouched"
        );
    }

    /// Feed `script` through termina's parser and absorb the events it yields,
    /// stopping at the fence the way [`exchange`] does. The flag is whether
    /// the fence was reached.
    fn absorbed(script: &str) -> (Replies, bool) {
        let mut parser = Parser::default();
        parser.parse(script.as_bytes(), false);
        let mut replies = Replies::default();
        while let Some(event) = parser.pop() {
            if replies.absorb(&event) {
                return (replies, true);
            }
        }
        (replies, false)
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
    fn the_color_queries_ask_for_the_two_colors_and_fence_nothing() {
        // Read back through the same parser the terminal's replies go through,
        // so the OSC numbers and the `?` payload are checked as values rather
        // than mirrored as literals.
        let mut parser = Parser::default();
        parser.parse(COLOR_QUERIES.as_bytes(), false);
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
        // Every terminal accepts BEL, and the ones that mirror the terminator
        // they were sent then answer with BEL too. Two queries, two bells.
        assert_eq!(
            COLOR_QUERIES.matches('\x07').count(),
            2,
            "both color queries end in BEL rather than ST: {COLOR_QUERIES:?}"
        );
        // A re-query goes out alone: the terminal answered these once already,
        // and a fence's reply would only arrive as an event no one is waiting
        // for.
        assert!(
            !COLOR_QUERIES.contains(FENCE),
            "the color queries carry no fence of their own: {COLOR_QUERIES:?}"
        );
    }

    #[test]
    fn the_fence_is_a_primary_device_attributes_request() {
        // The one request every terminal answers, so its reply says the colors
        // are not coming.
        assert_eq!(
            FENCE,
            Csi::Device(csi::Device::RequestPrimaryDeviceAttributes).to_string()
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
        let (replies, fenced) = absorbed(ANSWERED);

        assert!(fenced);
        assert_eq!(
            replies.resolve(),
            Some(TerminalColors {
                foreground: Color::Rgb(192, 202, 245),
                background: Color::Rgb(26, 27, 38),
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
        // The other format `XParseColor` accepts.
        assert_eq!(
            colors("\x1b]11;#1a1b26\x07").background,
            Some(Color::Rgb(26, 27, 38))
        );
        assert_eq!(
            colors("\x1b]11;#1a1a1b1b2626\x07").background,
            Some(Color::Rgb(26, 27, 38))
        );
        assert_eq!(
            colors("\x1b]11;#1a1b26\x07").background,
            Some(Color::Rgb(26, 27, 38)),
            "six digits"
        );
        assert_eq!(
            colors("\x1b]11;#048\x07").background,
            Some(Color::Rgb(0, 68, 136)),
            "one digit per channel"
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
    fn the_fence_ends_the_exchange_and_nothing_after_it_is_read() {
        // A terminal that answers the fence but not the colors has said all it
        // is going to; a reply arriving afterwards is not ours to wait for.
        let (replies, fenced) = absorbed("\x1b[?6c\x1b]11;rgb:1a1a/1b1b/2626\x07\x1b[?997;1n");

        assert!(fenced);
        assert_eq!(replies, Replies::default());
        assert_eq!(replies.resolve(), None);
    }

    #[test]
    fn replies_the_fence_did_not_cover_are_kept() {
        let (replies, fenced) = absorbed("\x1b]11;rgb:1a1a/1b1b/2626\x07\x1b[?6c");

        assert!(fenced);
        assert_eq!(replies.background, Some(Color::Rgb(26, 27, 38)));
    }

    #[test]
    fn unrecognized_traffic_between_replies_is_passed_over() {
        // A key press the user got in early, a cursor-position report meant for
        // someone else, and an OSC termina does not type at all — none of them
        // may end the exchange or displace a reply.
        let (replies, fenced) = absorbed(
            "\x1b]10;rgb:c0c0/caca/f5f5\x07\
             q\
             \x1b[10;5R\
             \x1b]4;1;rgb:ffff/0000/0000\x07\
             \x1b]11;rgb:1a1a/1b1b/2626\x07\
             \x1b[?62;1;6c",
        );

        assert!(fenced);
        assert!(replies.resolve().is_some());
    }

    #[test]
    fn a_terminal_that_says_nothing_resolves_to_nothing() {
        let (replies, fenced) = absorbed("");

        assert!(!fenced, "no fence, so the backstop is what ends");
        assert_eq!(replies.resolve(), None);
    }

    #[test]
    fn the_light_dark_verdict_is_a_trigger_and_never_a_color() {
        // Terminals disagree over whether it describes the palette or the
        // desktop, so it says only that something changed.
        for verdict in ["\x1b[?997;1n", "\x1b[?997;2n"] {
            assert_eq!(colors(verdict), Replies::default(), "{verdict:?}");
        }
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

    /// A DECRQSS reply, `DCS 1 $ r <sgr> m ST`.
    fn decrqss(sgr: &str) -> String {
        format!("\x1bP1$r{sgr}m\x1b\\")
    }

    #[test]
    fn the_probe_goes_out_ahead_of_the_fence_only_when_asked() {
        let ask = |colors, rgb| {
            let mut fake = FakeTerminal::scripted("");
            exchange(&mut fake, true, Ask { colors, rgb }).expect("silence is not an error");
            String::from_utf8_lossy(&fake.written).into_owned()
        };
        assert_eq!(
            ask(true, true),
            format!("{COLOR_QUERIES}{RGB_PROBE}{FENCE}")
        );
        assert_eq!(ask(false, true), format!("{RGB_PROBE}{FENCE}"));
        assert_eq!(ask(true, false), format!("{COLOR_QUERIES}{FENCE}"));
        assert_eq!(ask(false, false), "", "nothing to ask writes nothing");
    }

    #[test]
    fn the_probe_resets_what_it_set() {
        // It runs on the shell the user can still see, so the background it
        // sets must not outlive it.
        assert!(RGB_PROBE.starts_with("\x1b[0m\x1b[48;2;1;2;3m"));
        assert!(RGB_PROBE.ends_with("\x1b[0m"));
    }

    #[test]
    fn a_terminal_that_reports_the_probes_color_back_has_24_bit_color() {
        // Both shapes Neovim accepts: the attribute alone, or after a reset,
        // with or without xterm's empty color-space slot.
        for sgr in ["48:2:1:2:3", "0;48:2:1:2:3", "48:2::1:2:3"] {
            assert!(colors(&decrqss(sgr)).rgb, "{sgr}");
        }
    }

    #[test]
    fn anything_but_the_probes_color_leaves_the_depth_to_the_environment() {
        // A 256-color terminal that approximated it, another color, a reply
        // with more than the one attribute, an invalid request, and silence.
        for script in [
            decrqss("48:5:16"),
            decrqss("48:2:1:2:4"),
            decrqss("1;48:2:1:2:3"),
            "\x1bP0$r48:2:1:2:3m\x1b\\".to_owned(),
            String::new(),
        ] {
            assert!(!colors(&script).rgb, "{script:?}");
        }
    }

    #[test]
    fn the_probes_answer_comes_back_through_the_exchange() {
        let mut fake = FakeTerminal::scripted(&format!("{}\x1b[?62;1;6c", decrqss("48:2:1:2:3")));
        let answer = exchange(
            &mut fake,
            true,
            Ask {
                colors: false,
                rgb: true,
            },
        )
        .expect("the terminal answers");
        assert!(answer.rgb);
        assert_eq!(answer.colors, None);
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
