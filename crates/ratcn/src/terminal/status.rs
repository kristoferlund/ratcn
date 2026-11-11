//! What the app is doing, reported to the terminal with OSC 7501.
//!
//! The [Program Status Protocol](https://www.superlogical.com/rex/docs/build/program-status)
//! (revision 0.4) lets a program say it is working, blocked on the user, done,
//! or failed, with a message and optional progress. The terminal decides how
//! to show it: a tab glyph, a session list, a notification.
//!
//! A terminal drops a report it cannot take without saying so. The free text
//! is made to fit — control characters become spaces and an overlong title or
//! message is cut short — since it is often not the app's own (an error, a
//! file name). The record id and the program name are the app's own
//! identifiers, so one the terminal would drop fails loud instead.

use std::{collections::HashMap, io};

/// The longest record id, in bytes.
const ID_MAX: usize = 128;
/// The most segments in a record id.
const ID_SEGMENTS: usize = 8;
/// The longest segment of a record id, in bytes.
const SEGMENT_MAX: usize = 32;
/// The longest program identifier, in bytes.
const APP_MAX: usize = 32;
/// The longest title, in bytes before encoding.
const TITLE_MAX: usize = 192;
/// The longest message, in bytes before encoding.
const MESSAGE_MAX: usize = 2048;
/// The most records a terminal keeps. Past this many, the session forgets what
/// it wrote rather than grow, and repeats a report it would have skipped.
const RECORDS_MAX: usize = 256;

/// What a program is doing, as [`ProgramStatus`] reports it.
///
/// The protocol grows by adding states, so this may gain variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProgramState {
    /// Running, and waiting on nothing. Also what to report when the user
    /// cancels.
    Idle,
    /// Under way, at this percentage, or with no idea how far.
    Working {
        /// Percent done; above 100 is written as 100. [`None`] is
        /// indeterminate.
        progress: Option<u8>,
    },
    /// Waiting on the user.
    Blocked {
        /// What the user is asked for, if it is one of the protocol's kinds.
        kind: Option<BlockedKind>,
        /// Percent done; above 100 is written as 100.
        progress: Option<u8>,
    },
    /// Finished. The terminal keeps it until the user has seen it.
    Done,
    /// Failed. The terminal keeps it until the user has seen it.
    Error,
}

/// What a [`ProgramState::Blocked`] program is waiting for.
///
/// The protocol grows by adding kinds, so this may gain variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockedKind {
    /// Approval to go ahead.
    Permission,
    /// An answer the user types.
    Question,
    /// Credentials.
    Auth,
}

/// One report for [`Session::set_program_status`](super::Session::set_program_status):
/// a state, and what the terminal shows with it.
///
/// Each report replaces its record whole, so send the
/// [`app`](Self::app) and [`title`](Self::title) with every one that needs
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct ProgramStatus {
    state: ProgramState,
    id: Option<String>,
    app: Option<String>,
    title: Option<String>,
    message: Option<String>,
}

impl ProgramStatus {
    /// A report of `state` for the program as a whole.
    pub const fn new(state: ProgramState) -> Self {
        Self {
            state,
            id: None,
            app: None,
            title: None,
            message: None,
        }
    }

    /// Report on one task rather than the whole program: a path such as
    /// `build/test`, of at most 8 segments of `A–Z a–z 0–9 _ . + -`, 128 bytes
    /// in all.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// A stable name for the program, such as `deploy`: at most 32 bytes of
    /// `A–Z a–z 0–9 _ . + -`. Tasks inherit it from the record above them.
    pub fn app(mut self, app: impl Into<String>) -> Self {
        self.app = Some(app.into());
        self
    }

    /// A short title. It is shown on one line, with control characters as
    /// spaces, and cut at 192 bytes.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// What is happening. It is shown on one line, with control characters as
    /// spaces, and cut at 2048 bytes.
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

/// What each record shows, by record id (`""` for the program's own), as the
/// session last reported it: so an unchanged report is not written again, and
/// exit knows which records are still in progress.
pub(super) type Records = HashMap<String, ProgramStatus>;

/// Write `status` as OSC 7501, unless its record already shows it.
pub(super) fn write_status(
    out: &mut impl io::Write,
    records: &mut Records,
    status: &ProgramStatus,
) -> io::Result<()> {
    let key = status.id.as_deref().unwrap_or_default();
    if records.get(key) == Some(status) {
        return Ok(());
    }
    write!(out, "\x1b]7501;{}\x1b\\", encode(status)?)?;
    if records.len() >= RECORDS_MAX && !records.contains_key(key) {
        records.clear();
    }
    records.insert(key.to_owned(), status.clone());
    out.flush()
}

/// Remove the record `id` and every task beneath it.
pub(super) fn write_clear(
    out: &mut impl io::Write,
    records: &mut Records,
    id: &str,
) -> io::Result<()> {
    check_id(id)?;
    write!(out, "\x1b]7501;state=clear:id={id}\x1b\\")?;
    out.flush()?;
    let beneath = format!("{id}/");
    records.retain(|key, _| key != id && !key.starts_with(&beneath));
    Ok(())
}

/// Take back, once, the records still in progress as the app goes: the
/// terminal would otherwise show them until the shell exits, since a program
/// run from a shell ending is not something it sees. A task's record is
/// cleared, and the program's own reported idle, since clearing it would
/// take every other program's records with it. Done and error records stay
/// for the user to find.
pub(super) fn restore_records(out: &mut impl io::Write, records: &mut Records) -> io::Result<()> {
    let mut owed: Vec<(String, ProgramStatus)> = records
        .drain()
        .filter(|(_, status)| {
            matches!(
                status.state,
                ProgramState::Working { .. } | ProgramState::Blocked { .. }
            )
        })
        .collect();
    if owed.is_empty() {
        return Ok(());
    }
    owed.sort_by(|(one, _), (other, _)| one.cmp(other));
    for (id, status) in owed {
        if id.is_empty() {
            let idle = ProgramStatus {
                app: status.app,
                ..ProgramStatus::new(ProgramState::Idle)
            };
            write!(out, "\x1b]7501;{}\x1b\\", encode(&idle)?)?;
        } else {
            write!(out, "\x1b]7501;state=clear:id={id}\x1b\\")?;
        }
    }
    out.flush()
}

/// The report's `key=value` pairs, in the order the protocol's reference
/// implementations write them.
fn encode(status: &ProgramStatus) -> io::Result<String> {
    let (state, kind, progress) = match status.state {
        ProgramState::Idle => ("idle", None, None),
        ProgramState::Working { progress } => ("working", None, progress),
        ProgramState::Blocked { kind, progress } => ("blocked", kind, progress),
        ProgramState::Done => ("done", None, None),
        ProgramState::Error => ("error", None, None),
    };
    let mut pairs = vec![format!("state={state}")];
    if let Some(id) = &status.id {
        check_id(id)?;
        pairs.push(format!("id={id}"));
    }
    if let Some(kind) = kind {
        let kind = match kind {
            BlockedKind::Permission => "permission",
            BlockedKind::Question => "question",
            BlockedKind::Auth => "auth",
        };
        pairs.push(format!("kind={kind}"));
    }
    if let Some(progress) = progress {
        pairs.push(format!("progress={}", progress.min(100)));
    }
    if let Some(app) = &status.app {
        if app.is_empty() || app.len() > APP_MAX || !app.bytes().all(is_name_byte) {
            return Err(invalid(format!(
                "app {app:?} is not 1–{APP_MAX} bytes of A–Z a–z 0–9 _ . + -"
            )));
        }
        pairs.push(format!("app={app}"));
    }
    for (key, text, max) in [
        ("title", &status.title, TITLE_MAX),
        ("msg", &status.message, MESSAGE_MAX),
    ] {
        let Some(text) = text.as_deref().filter(|text| !text.is_empty()) else {
            continue;
        };
        pairs.push(format!("{key}={}", base64(one_line(text, max).as_bytes())));
    }
    Ok(pairs.join(":"))
}

/// A record id the terminal takes: `/`-separated segments, as many and as long
/// as the spec allows, of the name characters.
fn check_id(id: &str) -> io::Result<()> {
    let segments: Vec<&str> = id.split('/').collect();
    let fits = id.len() <= ID_MAX
        && segments.len() <= ID_SEGMENTS
        && segments.iter().all(|segment| {
            (1..=SEGMENT_MAX).contains(&segment.len()) && segment.bytes().all(is_name_byte)
        });
    if fits {
        Ok(())
    } else {
        Err(invalid(format!(
            "id {id:?} is not up to {ID_SEGMENTS} segments of 1–{SEGMENT_MAX} bytes of \
             A–Z a–z 0–9 _ . + -, {ID_MAX} bytes in all"
        )))
    }
}

/// `text` as the terminal takes it: every control character, a line break
/// included, as a space, and cut on a character boundary at `max` bytes.
fn one_line(text: &str, max: usize) -> String {
    let mut line = String::with_capacity(text.len().min(max));
    for c in text.chars().map(|c| if c.is_control() { ' ' } else { c }) {
        if line.len() + c.len_utf8() > max {
            break;
        }
        line.push(c);
    }
    line
}

const fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'+' | b'-')
}

fn invalid(reason: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}

/// Standard, padded base64.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk.iter().enumerate().fold(0u32, |acc, (i, &byte)| {
            acc | u32::from(byte) << (16 - 8 * i)
        });
        for i in 0..4 {
            if i <= chunk.len() {
                let sextet = (triple >> (18 - 6 * i)) & 0x3f;
                encoded.push(char::from(ALPHABET[sextet as usize]));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{
        BlockedKind, ProgramState, ProgramStatus, RECORDS_MAX, Records, base64, restore_records,
        write_clear, write_status,
    };

    fn written(status: &ProgramStatus) -> io::Result<String> {
        let mut out = Vec::new();
        write_status(&mut out, &mut Records::new(), status)?;
        Ok(String::from_utf8(out).expect("a report is ASCII"))
    }

    fn working(progress: Option<u8>) -> ProgramStatus {
        ProgramStatus::new(ProgramState::Working { progress })
    }

    /// The spec's examples, with the keys in the order the reference
    /// implementations write them (the spec's own examples vary it).
    #[test]
    fn the_spec_examples_are_written_in_the_reference_key_order() {
        let blocked = ProgramStatus::new(ProgramState::Blocked {
            kind: Some(BlockedKind::Permission),
            progress: None,
        })
        .app("terraform")
        .message("Apply 3 to add, 1 to change, 0 to destroy?");
        assert_eq!(
            written(&blocked).expect("valid"),
            "\x1b]7501;state=blocked:kind=permission:app=terraform:\
             msg=QXBwbHkgMyB0byBhZGQsIDEgdG8gY2hhbmdlLCAwIHRvIGRlc3Ryb3k/\x1b\\"
        );

        let task = working(Some(40))
            .id("us-east")
            .title("US East")
            .message("Pushing image");
        assert_eq!(
            written(&task).expect("valid"),
            "\x1b]7501;state=working:id=us-east:progress=40:title=VVMgRWFzdA==:\
             msg=UHVzaGluZyBpbWFnZQ==\x1b\\"
        );

        let done = ProgramStatus::new(ProgramState::Done)
            .app("brew")
            .message("Upgraded 12 packages");
        assert_eq!(
            written(&done).expect("valid"),
            "\x1b]7501;state=done:app=brew:msg=VXBncmFkZWQgMTIgcGFja2FnZXM=\x1b\\"
        );
    }

    #[test]
    fn each_state_has_its_name_and_progress_is_clamped() {
        for (state, body) in [
            (ProgramState::Idle, "state=idle"),
            (ProgramState::Working { progress: None }, "state=working"),
            (
                ProgramState::Working {
                    progress: Some(250),
                },
                "state=working:progress=100",
            ),
            (
                ProgramState::Blocked {
                    kind: Some(BlockedKind::Auth),
                    progress: Some(7),
                },
                "state=blocked:kind=auth:progress=7",
            ),
            (ProgramState::Done, "state=done"),
            (ProgramState::Error, "state=error"),
        ] {
            assert_eq!(
                written(&ProgramStatus::new(state)).expect("valid"),
                format!("\x1b]7501;{body}\x1b\\")
            );
        }
    }

    /// An empty title or message says nothing, and is left out rather than
    /// sent as an empty value.
    #[test]
    fn empty_text_is_left_out() {
        let status = ProgramStatus::new(ProgramState::Done).title("").message("");
        assert_eq!(
            written(&status).expect("valid"),
            "\x1b]7501;state=done\x1b\\"
        );
    }

    /// The text is often not the app's own: an error, a file name. A terminal
    /// would drop the whole report over one line break or one byte too many,
    /// so the text is made to fit rather than refused — at the moment an app
    /// reports an error, refusing would lose the report.
    #[test]
    fn text_is_made_to_fit_on_one_line_within_its_limit() {
        let error = ProgramStatus::new(ProgramState::Error)
            .title("t\u{85}")
            .message("failed:\n\tcaused by: x\x1b");
        assert_eq!(
            written(&error).expect("text never fails"),
            format!(
                "\x1b]7501;state=error:title={}:msg={}\x1b\\",
                base64(b"t "),
                base64(b"failed:  caused by: x ")
            )
        );

        // Cut on a character boundary: 191 bytes and a 2-byte character.
        let long = ProgramStatus::new(ProgramState::Done)
            .title(format!("{}é", "t".repeat(191)))
            .message("m".repeat(3000));
        assert_eq!(
            written(&long).expect("text never fails"),
            format!(
                "\x1b]7501;state=done:title={}:msg={}\x1b\\",
                base64("t".repeat(191).as_bytes()),
                base64("m".repeat(2048).as_bytes())
            )
        );
    }

    /// The record id and program name are the app's own identifiers. One the
    /// terminal would drop is a bug in the app, so it fails, with nothing
    /// written.
    #[test]
    fn an_id_or_app_the_terminal_would_drop_is_refused_unwritten() {
        let done = || ProgramStatus::new(ProgramState::Done);
        let refused = |status: ProgramStatus| {
            let mut out = Vec::new();
            let error = write_status(&mut out, &mut Records::new(), &status)
                .expect_err("the terminal would drop it");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{status:?}");
            assert!(out.is_empty(), "nothing is written: {out:?}");
        };

        for id in [
            "",
            "a//b",
            "a/",
            "has space",
            "a:b",
            &"x".repeat(33),
            "1/2/3/4/5/6/7/8/9",
            &vec!["y".repeat(32); 4].join("/"),
        ] {
            refused(done().id(id));
        }
        for app in ["", "has space", &"a".repeat(33)] {
            refused(done().app(app));
        }

        let longest_id = [
            "y".repeat(32),
            "y".repeat(32),
            "y".repeat(32),
            "y".repeat(29),
        ]
        .join("/");
        assert_eq!(longest_id.len(), 128);
        written(&done().id(longest_id).app("a".repeat(32))).expect("at the limits, valid");
    }

    /// An app can hand over its status every frame: a record already showing
    /// the report is not written again, while another record, or a change, is.
    #[test]
    fn a_report_is_written_only_when_its_record_changes() {
        let mut out = Vec::new();
        let mut records = Records::new();

        for status in [
            working(Some(10)),
            working(Some(10)),
            working(Some(10)).id("task"),
            working(Some(20)),
            working(Some(10)).id("task"),
        ] {
            write_status(&mut out, &mut records, &status).expect("valid");
        }

        assert_eq!(
            String::from_utf8(out).expect("ASCII"),
            "\x1b]7501;state=working:progress=10\x1b\\\
             \x1b]7501;state=working:id=task:progress=10\x1b\\\
             \x1b]7501;state=working:progress=20\x1b\\"
        );
    }

    /// Clearing a record takes the tasks beneath it too, so their next report
    /// is written again.
    #[test]
    fn clearing_forgets_the_record_and_everything_beneath_it() {
        let mut out = Vec::new();
        let mut records = Records::new();
        for id in ["build", "build/test", "builder"] {
            write_status(&mut out, &mut records, &working(None).id(id)).expect("valid");
        }

        out.clear();
        write_clear(&mut out, &mut records, "build").expect("valid");
        assert_eq!(out, b"\x1b]7501;state=clear:id=build\x1b\\");
        assert_eq!(
            records.keys().collect::<Vec<_>>(),
            ["builder"],
            "a sibling that shares a prefix stays"
        );

        let error = write_clear(&mut out, &mut records, "not valid").expect_err("dropped");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    /// The terminal drops a working or blocked record only when the shell
    /// exits or prompts again, so an app quitting mid-job takes its own back:
    /// tasks cleared, the program reported idle under its name. Finished
    /// records stay for the user to find, and a second restore writes nothing.
    #[test]
    fn exit_takes_back_only_what_is_still_in_progress() {
        let mut out = Vec::new();
        let mut records = Records::new();
        for status in [
            working(Some(5)).app("deploy"),
            ProgramStatus::new(ProgramState::Blocked {
                kind: None,
                progress: None,
            })
            .id("eu-west"),
            working(None).id("us-east"),
            ProgramStatus::new(ProgramState::Done).id("ap-south"),
        ] {
            write_status(&mut out, &mut records, &status).expect("valid");
        }

        out.clear();
        restore_records(&mut out, &mut records).expect("writable");
        assert_eq!(
            String::from_utf8(out).expect("ASCII"),
            "\x1b]7501;state=idle:app=deploy\x1b\\\
             \x1b]7501;state=clear:id=eu-west\x1b\\\
             \x1b]7501;state=clear:id=us-east\x1b\\"
        );

        let mut again = Vec::new();
        restore_records(&mut again, &mut records).expect("writable");
        assert!(again.is_empty(), "{again:?}");
    }

    /// What the session remembers is bounded by what a terminal keeps.
    #[test]
    fn the_memory_of_reports_stays_within_the_records_a_terminal_keeps() {
        let mut records = Records::new();
        for task in 0..=RECORDS_MAX {
            let status = ProgramStatus::new(ProgramState::Idle).id(format!("t{task}"));
            write_status(&mut io::sink(), &mut records, &status).expect("valid");
        }
        assert!(records.len() <= RECORDS_MAX, "{}", records.len());
    }

    #[test]
    fn base64_is_standard_and_padded() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64("Åsa".as_bytes()), "w4VzYQ==");
    }
}
