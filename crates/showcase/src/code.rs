//! Rust source in, themed lines out — the syntax coloring for the snippets the
//! Getting started page shows.
//!
//! Hand-rolled for one reason: the colors have to come out of the ratcn
//! [`Theme`]. Every ratatui highlighting crate surveyed maps a *color* to a
//! color rather than a token kind to a role, so none of them can follow a theme
//! switch, and none can serve [`Theme::adaptive`], whose palette is solved at
//! runtime from the terminal's own colors. The input here is small, fixed, and
//! in this repository, so one scanner over it is less code than adapting one of
//! them — and it is not a general highlighter, only enough Rust to color the
//! snippets we ship.
//!
//! A kind maps to a theme role and nothing else. The palette is used as the
//! library offers it: how far apart a theme holds its own colors is the theme's
//! decision, not this module's to second-guess.

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use ratcn::Theme;

/// What a run of source is, as far as coloring is concerned.
///
/// Deliberately finer than the color mapping needs: [`Kind::Type`] and
/// [`Kind::Function`] resolve to the same role. They stay separate variants
/// because the scanner can tell them apart, and a kind is a fact about the
/// source while a role is a decision about paint — merging the two here would
/// throw away something true to save a match arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Whitespace, and any identifier that is none of the below.
    Text,
    /// A run of operators and delimiters.
    Punct,
    /// A word in [`KEYWORDS`].
    Keyword,
    /// A name that looks like a type.
    Type,
    /// A name followed by `(`.
    Function,
    /// A name followed by `!`.
    Macro,
    /// A string literal, in any of its forms.
    Str,
    /// A character literal.
    Char,
    /// A numeric literal, suffix and radix included.
    Number,
    /// A line or block comment, doc comments included.
    Comment,
    /// A whole `#[...]` or `#![...]`.
    Attribute,
    /// A lifetime, `'` included.
    Lifetime,
}

/// The words that color as keywords: Rust's own, plus `self`/`Self` and the
/// primitive types, which read as keywords in every editor and are not worth a
/// second role.
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while", "bool", "char", "str", "f32", "f64", "i8", "i16", "i32",
    "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
];

/// Split Rust source into colored runs, in order.
///
/// Every byte of `source` lands in exactly one run, so concatenating the slices
/// reproduces the input. That is the property the tests lean on: a scanner bug
/// that eats or duplicates input fails the round trip rather than showing up as
/// a missing line on the page.
pub fn tokens(source: &str) -> Vec<(Kind, &str)> {
    let bytes = source.as_bytes();
    let mut runs = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let start = i;
        let byte = bytes[i];
        let kind = match byte {
            _ if byte.is_ascii_whitespace() => {
                i = scan(bytes, i, |b| b.is_ascii_whitespace());
                Kind::Text
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = scan(bytes, i, |b| b != b'\n');
                Kind::Comment
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = block_comment_end(bytes, i);
                Kind::Comment
            }
            b'#' if attribute_body(bytes, i).is_some() => {
                i = attribute_end(bytes, i);
                Kind::Attribute
            }
            b'"' => {
                i = string_end(bytes, i + 1);
                Kind::Str
            }
            // `b'a'` is a byte character; the prefix is consumed here and the
            // quote is scanned by the same code the unprefixed form uses.
            b'b' if char_end(source, i + 1).is_some() => {
                i = char_end(source, i + 1).expect("the guard just checked it");
                Kind::Char
            }
            b'\'' => match char_end(source, i) {
                Some(end) => {
                    i = end;
                    Kind::Char
                }
                None => {
                    i = scan(bytes, i + 1, is_word);
                    Kind::Lifetime
                }
            },
            b'0'..=b'9' => {
                i = number_end(bytes, i);
                Kind::Number
            }
            _ => {
                if let Some(end) = prefixed_string_end(bytes, i) {
                    i = end;
                    Kind::Str
                } else if is_word(byte) {
                    i = scan(bytes, i + 1, is_word);
                    let kind = classify(&source[start..i], bytes, i);
                    if kind == Kind::Macro {
                        // The `!` is part of the name on screen.
                        i += 1;
                    }
                    kind
                } else if is_punct(byte) {
                    i = punct_end(bytes, i);
                    Kind::Punct
                } else {
                    // A stray control byte. It is not a token, but it is a
                    // byte, and every byte has to land somewhere.
                    i += 1;
                    Kind::Text
                }
            }
        };
        runs.push((kind, &source[start..i]));
    }

    runs
}

/// One snippet, ready for a `Paragraph`.
///
/// A trailing newline does not produce a trailing empty line, so a snippet read
/// straight out of a file renders as the lines it has.
pub fn highlight<'a>(source: &'a str, theme: &Theme) -> Vec<Line<'a>> {
    let mut lines = vec![Line::default()];

    for (kind, text) in tokens(source) {
        let style = Style::new().fg(role(kind, theme));
        let mut parts = text.split('\n');
        // The first part continues the line in progress; every part after a
        // newline starts a new one.
        push(&mut lines, parts.next().unwrap_or_default(), style);
        for part in parts {
            lines.push(Line::default());
            push(&mut lines, part, style);
        }
    }

    if lines.last().is_some_and(|line| line.spans.is_empty()) {
        lines.pop();
    }
    lines
}

/// Append `text` to the last line, unless there is nothing to append.
fn push<'a>(lines: &mut [Line<'a>], text: &'a str, style: Style) {
    if let (false, Some(line)) = (text.is_empty(), lines.last_mut()) {
        line.spans.push(Span::styled(text, style));
    }
}

/// The color a kind paints in: the theme's role, straight, with no lifting and
/// no arithmetic.
fn role(kind: Kind, theme: &Theme) -> Color {
    match kind {
        Kind::Text | Kind::Punct => theme.foreground,
        Kind::Comment => theme.muted_foreground,
        Kind::Keyword => theme.accent,
        Kind::Str | Kind::Char | Kind::Number => theme.warning,
        Kind::Type | Kind::Function | Kind::Macro | Kind::Attribute | Kind::Lifetime => {
            theme.primary
        }
    }
}

// The scanner. One pass over the bytes; each arm above matches on the first
// byte of a token and one of these consumes to its end.

/// Bytes a word is made of. Everything non-ASCII counts, which keeps a
/// non-ASCII identifier in one run and every slice on a char boundary.
fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// Bytes a punctuation run is made of. `"` and `'` are left out: each starts a
/// token of its own.
fn is_punct(byte: u8) -> bool {
    matches!(byte, b'!'..=b'/' | b':'..=b'@' | b'['..=b'^' | b'`' | b'{'..=b'~')
        && !matches!(byte, b'"' | b'\'')
}

/// The first index at or after `from` where `keep` stops holding.
fn scan(bytes: &[u8], from: usize, keep: impl Fn(u8) -> bool) -> usize {
    let mut i = from;
    while i < bytes.len() && keep(bytes[i]) {
        i += 1;
    }
    i
}

/// Which kind a word is, deciding in this order: a `!` that is not `!=` makes
/// it a macro, the keyword table makes it a keyword, a `(` makes it a function,
/// a leading uppercase makes it a type.
///
/// Tuple-variant construction (`Some(`, `Msg::Hello(`) therefore reads as a
/// function rather than a type. Both resolve to the same role, so it is
/// invisible — which is the reason [`role`] merges them, and the reason this is
/// a comment instead of a special case.
fn classify(word: &str, bytes: &[u8], end: usize) -> Kind {
    if bytes.get(end) == Some(&b'!') && bytes.get(end + 1) != Some(&b'=') {
        Kind::Macro
    } else if KEYWORDS.contains(&word) {
        Kind::Keyword
    } else if bytes.get(end) == Some(&b'(') {
        Kind::Function
    } else if word.starts_with(char::is_uppercase) {
        Kind::Type
    } else {
        Kind::Text
    }
}

/// A punctuation run, stopping before anything that starts a token of its own:
/// a comment, or an attribute.
fn punct_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < bytes.len() && is_punct(bytes[i]) {
        let opens_a_token = (bytes[i] == b'/' && matches!(bytes.get(i + 1), Some(b'/' | b'*')))
            || (bytes[i] == b'#' && attribute_body(bytes, i).is_some());
        if opens_a_token {
            break;
        }
        i += 1;
    }
    i
}

/// Past the closing `*/` of a block comment, counting depth so a commented-out
/// comment is still one run.
fn block_comment_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 2;
    let mut depth = 1_usize;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"/*") {
            depth += 1;
            i += 2;
        } else if bytes[i..].starts_with(b"*/") {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    bytes.len()
}

/// The index of the `[` opening an attribute at `start`, for `#[` and `#![`.
fn attribute_body(bytes: &[u8], start: usize) -> Option<usize> {
    match bytes.get(start + 1) {
        Some(b'[') => Some(start + 1),
        Some(b'!') if bytes.get(start + 2) == Some(&b'[') => Some(start + 2),
        _ => None,
    }
}

/// Past the `]` matching an attribute's `[`, so the whole attribute is one run.
///
/// Strings inside are skipped whole. A `]` in one is not a bracket —
/// `#[cfg(feature = "]")]` is real Rust — and counting it would end the
/// attribute early, leaving the rest of the file to scan as one unterminated
/// string literal.
fn attribute_end(bytes: &[u8], start: usize) -> usize {
    let Some(open) = attribute_body(bytes, start) else {
        return start + 1;
    };
    let mut depth = 0_usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i = string_end(bytes, i + 1);
                continue;
            }
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

/// Past the closing quote of a string opened at `from - 1`, honoring escapes.
fn string_end(bytes: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Past the closing `"` and hashes of a raw string whose body starts at `from`.
fn raw_string_end(bytes: &[u8], from: usize, hashes: usize) -> usize {
    let mut i = from;
    while i < bytes.len() {
        let closed = bytes[i] == b'"'
            && bytes
                .get(i + 1..i + 1 + hashes)
                .is_some_and(|tail| tail.iter().all(|byte| *byte == b'#'));
        if closed {
            return i + 1 + hashes;
        }
        i += 1;
    }
    bytes.len()
}

/// The end of a string with a `b`, `r`, or `br` prefix at `start`, or `None` if
/// that is not what is there — a raw identifier (`r#type`) or a plain word
/// beginning with one of those letters.
fn prefixed_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    if bytes.get(i) == Some(&b'b') {
        i += 1;
    }
    let raw = bytes.get(i) == Some(&b'r');
    if raw {
        i += 1;
    }
    if i == start {
        return None;
    }
    if !raw {
        return (bytes.get(i) == Some(&b'"')).then(|| string_end(bytes, i + 1));
    }
    let hashes = scan(bytes, i, |byte| byte == b'#') - i;
    let quote = i + hashes;
    (bytes.get(quote) == Some(&b'"')).then(|| raw_string_end(bytes, quote + 1, hashes))
}

/// Past the closing `'` of a character literal at `start`, or `None` when the
/// `'` opens a lifetime instead.
///
/// `b'a'` is scanned by pointing this at the quote, one byte on: the prefix is
/// the caller's to consume, exactly as it is for `b"..."`.
///
/// One lookahead decides it: `'a'` has a quote one character on, `'a` does not.
fn char_end(source: &str, start: usize) -> Option<usize> {
    let rest = source.get(start + 1..)?;
    let first = rest.chars().next()?;
    if first != '\\' {
        return rest[first.len_utf8()..]
            .starts_with('\'')
            .then_some(start + 1 + first.len_utf8() + 1);
    }
    // An escape is as long as it needs to be (`'\n'`, `'\u{1f600}'`): the
    // backslash consumes the character after it, then the next quote closes.
    let bytes = source.as_bytes();
    let end = scan(bytes, start + 3, |byte| byte != b'\'');
    (bytes.get(end) == Some(&b'\'')).then_some(end + 1)
}

/// Past a numeric literal, keeping the radix, the digit separators, the suffix
/// and a fractional part in one run.
///
/// A `.` only continues the number when a digit follows it, so `1.0` is one
/// token while `0..=100` is a number, a punctuation run, and a number.
fn number_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() {
        if is_word(bytes[i]) {
            i += 1;
        } else if bytes[i] == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
            i += 2;
        } else {
            break;
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real ratcn code, and the snapshot's subject: lifetimes, doc comments, a
    /// builder chain, a `match` on messages, attributes.
    const TEMPLATE: &str = include_str!("../../cargo-ratcn/templates/first-app.rs");

    /// Everything the scanner has an arm for, including the shapes it is
    /// allowed to get wrong (an unterminated literal, a stray `#`) but never
    /// allowed to drop.
    const FIXTURES: &[&str] = &[
        TEMPLATE,
        "",
        "\n\n",
        "let x = 1;",
        "'static 'a' '\\n' '\\u{1f600}' 'é'",
        "r\"raw\" r#\"ra\"w\"# br#\"bytes\"# b\"bytes\" \"esc\\\"aped\"",
        "/* /* nested */ */ // line\n/// doc\n//! inner",
        "#[derive(Debug)] #![allow(dead_code)] #[cfg(all(a, b))]",
        // A `]` inside a string is not the attribute's closing bracket.
        "#[cfg(feature = \"]\")] fn after() {}",
        "b'a' b'\\n' b\"bytes\" b_ident",
        "0..=100 1.0 3u32 0xff 1_000_usize 9.",
        "vec![1] a != b x!=y Some(1) String::new() é_ident",
        "let unterminated = \"oops",
        "let dangling = '",
        "#",
    ];

    /// The one kind of scanner bug that would be invisible on the page: a run
    /// that eats or duplicates input. Every other test here rests on this one.
    #[test]
    fn every_byte_lands_in_exactly_one_run() {
        for source in FIXTURES {
            let rebuilt: String = tokens(source).into_iter().map(|(_, text)| text).collect();
            assert_eq!(&rebuilt, source, "the runs do not rebuild the source");
        }
    }

    crate::snapshot::snapshot_test! {
        /// A snapshot of a real, in-repo file: the starter `cargo ratcn init`
        /// writes. If someone edits the template, this fails and a human looks
        /// at what the coloring became.
        name: the_template_tokenizes_the_same_way_it_did,
        fixture: "src/code_snapshot.txt",
        expected: include_str!("code_snapshot.txt"),
        actual: crate::snapshot::of([TEMPLATE]),
    }

    /// The rules the surveyed highlighters get wrong, one assertion each. These
    /// are why this module exists rather than a dependency.
    #[test]
    fn the_awkward_literals_are_each_one_run() {
        let one = |source: &str| {
            let runs = tokens(source);
            assert_eq!(runs.len(), 1, "{source:?} split into {runs:?}");
            runs[0].0
        };

        assert_eq!(
            one("'static"),
            Kind::Lifetime,
            "a lifetime, not three tokens"
        );
        assert_eq!(one("'a'"), Kind::Char);
        assert_eq!(one("'\\n'"), Kind::Char, "an escape is still one char");
        assert_eq!(one("=>"), Kind::Punct, "punctuation runs greedily");
        assert_eq!(one("});"), Kind::Punct);
        assert_eq!(one("r#\"raw\"#"), Kind::Str);
        assert_eq!(one("/* /* */ */"), Kind::Comment, "block comments nest");
        assert_eq!(one("/// doc"), Kind::Comment);
        assert_eq!(one("#[derive(Debug)]"), Kind::Attribute);
        assert_eq!(
            one("#[cfg(feature = \"]\")]"),
            Kind::Attribute,
            "a `]` inside a string does not close the attribute — miscounting it \
             leaves the whole rest of the file scanning as one string"
        );
        assert_eq!(one("b'a'"), Kind::Char, "a byte character, like b\"...\"");
        assert_eq!(one("b'\\n'"), Kind::Char);
        assert_eq!(
            tokens("b_ident")[0].0,
            Kind::Text,
            "and a plain word starting with b is still a word"
        );
        assert_eq!(
            one("3u32"),
            Kind::Number,
            "the suffix is part of the number"
        );
        assert_eq!(one("0xff"), Kind::Number);
        assert_eq!(one("1.0"), Kind::Number);

        assert_eq!(
            tokens("vec![1]")[0],
            (Kind::Macro, "vec!"),
            "the `!` is part of the macro's name"
        );
        assert_eq!(tokens("a != b")[0].0, Kind::Text, "`!=` is not a macro");
        assert_eq!(tokens("fn main()")[2].0, Kind::Function);
        assert_eq!(tokens("Theme::default()")[0].0, Kind::Type);
        assert_eq!(
            tokens("0..=100")[1].0,
            Kind::Punct,
            "a range is not a float"
        );
    }

    /// The mapping is wired to the fields it names. An accidental swap of two
    /// roles is invisible on the page until someone runs a theme that paints
    /// them differently, so it is asserted here instead.
    #[test]
    fn kinds_resolve_to_the_theme_fields_they_name() {
        let theme = Theme::default();
        assert_eq!(role(Kind::Keyword, &theme), theme.accent);
        assert_eq!(role(Kind::Type, &theme), theme.primary);
        assert_eq!(role(Kind::Str, &theme), theme.warning);
        assert_eq!(role(Kind::Comment, &theme), theme.muted_foreground);
    }

    /// The lines a `Paragraph` gets: one per source line, styled per run.
    #[test]
    fn highlighting_splits_on_newlines_and_colors_each_run() {
        let theme = Theme::default();
        let lines = highlight("let x = 1;\nlet y = 2;\n", &theme);

        assert_eq!(lines.len(), 2, "a trailing newline is not a trailing line");
        assert_eq!(
            lines[0].spans[0].style.fg,
            Some(role(Kind::Keyword, &theme))
        );
        let rebuilt: String = lines[1]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(rebuilt, "let y = 2;");
    }
}
