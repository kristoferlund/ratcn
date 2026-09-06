//! The token-stream snapshot both coloring fixtures are kept by.
//!
//! Two tests snapshot `code::tokens` output — the CLI template that ships in
//! this repository, and the two Rust snippets the Getting started page shows —
//! and they were the same twenty lines twice, kept in step by hand. This is
//! that machinery once. `crates/ratcn/src/test_support.rs` is the precedent for
//! a `#[cfg(test)]` module of shared fixtures.

use crate::code;

/// The environment variable that rewrites a fixture instead of comparing
/// against it.
///
/// **It is read by every snapshot test at once**, so setting it regenerates
/// every fixture in the crate, not the one named on the command line. That is
/// usually what you want and never silent: rewriting is a deliberate act, and
/// the point of these tests is that the diff gets read.
pub const UPDATE: &str = "UPDATE_CODE_SNAPSHOT";

/// One `Kind:text` per line, whitespace dropped: enough to read as a
/// classification, short of a debug dump of the whole `Vec`. A run that spans
/// lines of its own — a block comment, a multi-line string — keeps the
/// one-per-line shape by escaping its newlines.
pub fn of<'a>(sources: impl IntoIterator<Item = &'a str>) -> String {
    sources
        .into_iter()
        .flat_map(|source| {
            code::tokens(source)
                .into_iter()
                .filter(|(_, text)| !text.trim().is_empty())
                .map(|(kind, text)| format!("{kind:?}:{}\n", text.replace('\n', "\\n")))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Compare `actual` against `expected`, or rewrite `fixture` when [`UPDATE`] is
/// set. `name` is the test's own name, so the failure names the command that
/// repairs it.
///
/// `fixture` is relative to the crate root, which is where `cargo test` runs
/// tests from.
pub fn check(name: &str, fixture: &str, expected: &str, actual: &str) {
    if std::env::var_os(UPDATE).is_some() {
        std::fs::write(fixture, actual).expect("the snapshot is writable");
        return;
    }

    let regenerate = format!("{UPDATE}=1 cargo test -p showcase {name}");
    let differs = expected
        .lines()
        .zip(actual.lines())
        .position(|(expected, actual)| expected != actual);

    assert!(
        differs.is_none(),
        "token snapshot differs at line {}:\n  expected: {}\n  actual:   {}\n\
         If that was intended, regenerate with:\n  {regenerate}",
        differs.unwrap_or_default() + 1,
        expected
            .lines()
            .nth(differs.unwrap_or_default())
            .unwrap_or_default(),
        actual
            .lines()
            .nth(differs.unwrap_or_default())
            .unwrap_or_default(),
    );
    assert_eq!(
        expected.lines().count(),
        actual.lines().count(),
        "the snapshot gained or lost tokens; if that was intended, regenerate with:\n  \
         {regenerate}"
    );
}

/// Declare a snapshot test: the doc comment, the `#[test]`, and the call into
/// [`check`] with the test's own name, so the name in the failure message
/// cannot drift from the name of the test that printed it.
macro_rules! snapshot_test {
    (
        $(#[doc = $doc:expr])*
        name: $name:ident,
        fixture: $fixture:expr,
        expected: $expected:expr,
        actual: $actual:expr,
    ) => {
        $(#[doc = $doc])*
        ///
        /// Setting `UPDATE_CODE_SNAPSHOT` rewrites the fixture — every fixture
        /// in the crate — instead of comparing against it.
        #[test]
        fn $name() {
            $crate::snapshot::check(stringify!($name), $fixture, $expected, &$actual);
        }
    };
}

pub(crate) use snapshot_test;
