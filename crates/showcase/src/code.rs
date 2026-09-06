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

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use ratcn::{Theme, color};

/// What a run of source is, as far as coloring is concerned.
///
/// Deliberately finer than the color mapping needs: [`Kind::Type`] and
/// [`Kind::Function`] resolve to the same role today, and stay separate
/// variants so that merge can be undone in [`role`] without touching the
/// scanner.
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

// The theme mapping. Code paints on `background`, not `field`: ratcn holds
// `foreground` and `muted_foreground` to a text floor, but checks each accent
// against its *own* foreground, and on `field` Solarized's `accent` measures
// 2.86:1.

/// How far apart two colors have to be, in the channel that separates them
/// most, to read as two colors rather than one.
///
/// Contrast is the wrong measure for this: it is luminance only, and
/// Catppuccin's blue and peach are 1.19:1 apart while being obviously different
/// colors. Channel distance is measured, not chosen: `default_dark` and the
/// adaptive themes put `primary` 21 channels from `foreground` — `#e5e5e5`
/// against `#fafafa` — and Gruvbox paints `primary` and `warning` the identical
/// yellow, 0 apart. The nearest preset above is Catppuccin at 68, so any
/// threshold between the two behaves the same.
const NAME_STEP: u8 = 40;

/// WCAG 2.1 normal text (1.4.3), the same floor ratcn holds its own text roles
/// to.
const TEXT_FLOOR: f64 = 4.5;

/// The color a kind paints in.
///
/// Five painted colors, no more: the docs site's own Shiki output uses six for
/// Rust and spends the sixth on `self` in a second blue, which nobody reads.
fn role(kind: Kind, theme: &Theme) -> Color {
    match kind {
        Kind::Text | Kind::Punct => theme.foreground,
        Kind::Comment => to_floor(theme.muted_foreground, theme),
        Kind::Keyword => to_floor(theme.accent, theme),
        Kind::Str | Kind::Char | Kind::Number => to_floor(theme.warning, theme),
        Kind::Type | Kind::Function | Kind::Macro | Kind::Attribute | Kind::Lifetime => {
            name_color(theme)
        }
    }
}

/// What names paint in: `primary`, or `foreground` when this palette's
/// `primary` would be indistinguishable from the text or the literals beside
/// it.
///
/// Gruvbox is the second case in one direction — `primary` *is* `warning`, the
/// same yellow, so types and literals would be one color — and `default_dark`
/// and the adaptive themes are it in the other, with `primary` a hair off
/// `foreground`. Falling back to the body color is honest there: those palettes
/// are monochrome, so a monochrome code block is coherent.
fn name_color(theme: &Theme) -> Color {
    let primary = to_floor(theme.primary, theme);
    let distinct =
        separated(primary, theme.foreground) && separated(primary, to_floor(theme.warning, theme));
    if distinct { primary } else { theme.foreground }
}

/// `color` lifted onto the text floor against the background it is painted on,
/// by blending toward `foreground` until it clears — the same solve-to-a-floor
/// [`Theme::adaptive`] does, rather than picking a color that happens to work.
///
/// Solarized's `accent` sits at 3.30:1 on its background and adaptive-light's
/// `warning` at 3.12:1; both would be keywords and literals you have to lean in
/// to read.
fn to_floor(color: Color, theme: &Theme) -> Color {
    // A color with no channels to measure is the terminal's own and passes
    // through: `contrast` answering `None` is what makes this a no-op.
    let clears =
        |color: Color| color::contrast(color, theme.background).is_none_or(|c| c >= TEXT_FLOOR);
    if clears(color) {
        return color;
    }
    (1..=100)
        .map(|amount| color::dim(color, theme.foreground, amount))
        .find(|lifted| clears(*lifted))
        // The walk ends at `foreground` itself, which ratcn holds to the floor,
        // so this is unreachable; naming it keeps the function total.
        .unwrap_or(theme.foreground)
}

/// Whether two colors are far enough apart to read as two colors.
///
/// [`Theme::terminal`] paints in `Color::Reset` and ANSI-indexed colors, whose
/// channels nothing here can read: that palette is the user's own, and
/// second-guessing it is out of scope. An unreadable color is taken as
/// separated, which is what makes the guards above no-ops there.
fn separated(a: Color, b: Color) -> bool {
    match (color::resolve_rgb(a), color::resolve_rgb(b)) {
        (Some(a), Some(b)) => {
            let apart = |a: u8, b: u8| a.abs_diff(b);
            apart(a.0, b.0).max(apart(a.1, b.1)).max(apart(a.2, b.2)) >= NAME_STEP
        }
        _ => true,
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
fn attribute_end(bytes: &[u8], start: usize) -> usize {
    let Some(open) = attribute_body(bytes, start) else {
        return start + 1;
    };
    let mut depth = 0_usize;
    for (i, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
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
        "0..=100 1.0 3u32 0xff 1_000_usize 9.",
        "vec![1] a != b x!=y Some(1) String::new() é_ident",
        "let unterminated = \"oops",
        "let dangling = '",
        "#",
    ];

    /// One `Kind:text` per line, whitespace dropped: enough to read as a
    /// classification, short of a debug dump of the whole `Vec`. A run that
    /// spans lines of its own — a block comment, a multi-line string — keeps
    /// the one-per-line shape by escaping its newlines.
    fn snapshot(source: &str) -> String {
        tokens(source)
            .into_iter()
            .filter(|(_, text)| !text.trim().is_empty())
            .map(|(kind, text)| format!("{kind:?}:{}\n", text.replace('\n', "\\n")))
            .collect()
    }

    /// The one kind of scanner bug that would be invisible on the page: a run
    /// that eats or duplicates input. Every other test here rests on this one.
    #[test]
    fn every_byte_lands_in_exactly_one_run() {
        for source in FIXTURES {
            let rebuilt: String = tokens(source).into_iter().map(|(_, text)| text).collect();
            assert_eq!(&rebuilt, source, "the runs do not rebuild the source");
        }
    }

    /// Where the snapshot lives, relative to the crate root `cargo test` runs
    /// tests from.
    const SNAPSHOT: &str = "src/code_snapshot.txt";

    /// The one command that repairs this fixture, named everywhere it is
    /// needed so the next person reads the fix rather than deriving it.
    const REGENERATE: &str =
        "UPDATE_CODE_SNAPSHOT=1 cargo test -p showcase the_template_tokenizes_the_same_way_it_did";

    /// A snapshot of a real, in-repo file. If someone edits the template, this
    /// fails and a human looks at what the coloring became.
    ///
    /// Setting `UPDATE_CODE_SNAPSHOT` rewrites the fixture instead of comparing
    /// against it — see [`REGENERATE`] for the whole command. Rewriting is a
    /// deliberate act: the point of the test is that the diff gets read.
    #[test]
    fn the_template_tokenizes_the_same_way_it_did() {
        let actual = snapshot(TEMPLATE);
        if std::env::var_os("UPDATE_CODE_SNAPSHOT").is_some() {
            std::fs::write(SNAPSHOT, &actual).expect("the snapshot is writable");
            return;
        }

        let expected = include_str!("code_snapshot.txt");
        let differs = expected
            .lines()
            .zip(actual.lines())
            .position(|(expected, actual)| expected != actual);

        if let Some(line) = differs {
            panic!(
                "token snapshot differs at line {}:\n  expected: {}\n  actual:   {}\n\
                 If `first-app.rs` changed on purpose, regenerate with:\n  {REGENERATE}",
                line + 1,
                expected.lines().nth(line).unwrap_or_default(),
                actual.lines().nth(line).unwrap_or_default(),
            );
        }
        assert_eq!(
            expected.lines().count(),
            actual.lines().count(),
            "the template gained or lost tokens; if that was intended, regenerate with:\n  \
             {REGENERATE}"
        );
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

    /// Every kind, for iterating a theme's whole mapping.
    const KINDS: [Kind; 12] = [
        Kind::Text,
        Kind::Punct,
        Kind::Keyword,
        Kind::Type,
        Kind::Function,
        Kind::Macro,
        Kind::Str,
        Kind::Char,
        Kind::Number,
        Kind::Comment,
        Kind::Attribute,
        Kind::Lifetime,
    ];

    /// The themes the mapping has to hold for: the presets, plus a solved one
    /// on either polarity, whose palette is not known until it runs.
    fn themes() -> Vec<Theme> {
        let mut themes = Theme::presets().to_vec();
        themes.push(Theme::adaptive(
            Color::Rgb(253, 246, 227),
            Color::Rgb(101, 123, 131),
            None,
        ));
        themes.push(Theme::adaptive(
            Color::Rgb(10, 10, 10),
            Color::Rgb(250, 250, 250),
            None,
        ));
        themes
    }

    /// Gruvbox paints `primary` and `warning` the identical yellow, so without
    /// the guard a type name and a string would be one color.
    #[test]
    fn gruvbox_does_not_paint_names_and_literals_alike() {
        let theme = Theme::gruvbox();
        assert_eq!(theme.primary, theme.warning, "the collision this guards");
        assert_ne!(role(Kind::Type, &theme), role(Kind::Str, &theme));
    }

    /// `default_dark` and the adaptive themes put `primary` a hair off
    /// `foreground`, so without the guard a name would be invisible in the
    /// text. The fallback makes it plain text on purpose.
    #[test]
    fn a_monochrome_palette_paints_names_as_plain_text() {
        for theme in [
            Theme::default_dark(),
            Theme::adaptive(Color::Rgb(10, 10, 10), Color::Rgb(250, 250, 250), None),
        ] {
            assert_eq!(
                role(Kind::Type, &theme),
                role(Kind::Text, &theme),
                "{}: a name a step off the body color should fall back to it",
                theme.name
            );
        }
        // A palette with a real accent keeps it, or the guard would be a
        // blanket rule rather than a fallback.
        let theme = Theme::catppuccin();
        assert_eq!(role(Kind::Type, &theme), theme.primary);
    }

    /// Code paints on `background`, and everything on it is text.
    #[test]
    fn every_painted_color_clears_the_text_floor() {
        for theme in themes() {
            for kind in KINDS {
                let Some(contrast) = color::contrast(role(kind, &theme), theme.background) else {
                    continue; // A color the terminal owns, which is not ours to measure.
                };
                assert!(
                    contrast >= TEXT_FLOOR,
                    "{}: {kind:?} paints at {contrast:.2}:1 on the background",
                    theme.name
                );
            }
        }
    }

    /// `Theme::terminal` is the user's own palette. Every guard has to be a
    /// no-op on a color that carries no channels to measure.
    #[test]
    fn the_terminal_palette_passes_through_untouched() {
        let theme = Theme::terminal();
        assert_eq!(role(Kind::Text, &theme), theme.foreground);
        assert_eq!(role(Kind::Comment, &theme), theme.muted_foreground);
        assert_eq!(role(Kind::Keyword, &theme), theme.accent);
        assert_eq!(role(Kind::Str, &theme), theme.warning);
        assert_eq!(role(Kind::Type, &theme), theme.primary);
    }

    /// The lines a `Paragraph` gets: one per source line, styled per run.
    #[test]
    fn highlighting_splits_on_newlines_and_colors_each_run() {
        let theme = Theme::catppuccin();
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
