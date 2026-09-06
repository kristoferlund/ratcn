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

// The theme mapping. Code paints on `background`, not `field`: ratcn holds
// `foreground` and `muted_foreground` to a text floor, but checks each accent
// against its *own* foreground, and on `field` Solarized's `accent` measures
// 2.86:1.

/// How far apart two colors have to be to read as two colors rather than one,
/// in CIE76 ΔE.
///
/// A judgement about human vision rather than a number tuned to a palette,
/// which is what every earlier version of this rule was. On the standard ΔE
/// reading, 1–2 is the just-noticeable difference for two large patches, 2–10
/// is "perceptible at a glance", and above 10 two colors are read as two colors
/// rather than as one shade of each other. Syntax coloring lives at the top of
/// that: these are small glyphs, side by side, and a difference you have to
/// look for is a difference that is not doing any work.
const VISIBLE_STEP: f64 = 10.0;

/// WCAG 2.1 normal text (1.4.3), the same floor ratcn holds its own text roles
/// to.
const TEXT_FLOOR: f64 = 4.5;

/// The color a kind paints in.
///
/// Five painted colors at most, and never more: the docs site's own Shiki
/// output uses six for Rust and spends the sixth on `self` in a second blue,
/// which nobody reads. A palette whose accents cannot carry five paints fewer —
/// see [`name_and_keyword`].
fn role(kind: Kind, theme: &Theme) -> Color {
    let (name, keyword) = name_and_keyword(theme);
    match kind {
        Kind::Text | Kind::Punct => theme.foreground,
        Kind::Comment => to_floor(theme.muted_foreground, theme),
        Kind::Keyword => keyword,
        Kind::Str | Kind::Char | Kind::Number => to_floor(theme.warning, theme),
        Kind::Type | Kind::Function | Kind::Macro | Kind::Attribute | Kind::Lifetime => name,
    }
}

/// What names and keywords paint in, decided together out of the palette's two
/// accents.
///
/// The two roles are not equal. On a page whose whole job is an API tour,
/// `Button`, `Msg`, `EventResult`, `render` and `on_press` are the words the
/// reader came for, and `let` and `if` are not — so **names take whichever
/// accent stands out further from the body text**, and keywords take the other.
/// An accent that cannot be told from the body text is not used at all, and
/// neither is one that cannot be told from the literals beside it: those fall
/// back to `foreground`, which is honest — this palette has fewer colors than
/// the mapping wants, and pretending otherwise paints two kinds alike.
///
/// This compares rather than thresholds, and that is the point. A threshold
/// deciding *which* accent is a name color is a cliff wherever a palette lands
/// on it: `default_dark` and Gruvbox need the choice made one way, Catppuccin
/// the other, and `Theme::adaptive` — which the showcase runs on, and which
/// builds `primary` off the neutral ladder while taking `accent` from the
/// terminal's own palette — solves to distances spread across the whole range,
/// so no number is safe. Catppuccin Latte missing a 40-channel threshold by one
/// is a symptom of that, not the bug. A comparison has no cliff to land on:
/// moving an accent closer to the body text can only ever demote it behind the
/// other one.
///
/// "Further" is measured in ΔE, not in channels, and that changes answers:
/// Catppuccin's blue `primary` is 68 channels off its body text and its mauve
/// `accent` only 48, but the body text is itself a pale blue, so the mauve is
/// the one that stands out — 34.65 ΔE against 26.87.
///
/// A color carries no meaning across themes, so which hue lands on which kind
/// may differ per palette, and so does how many colors a palette can carry —
/// three on a terminal whose accents both sit close to its body text, five on
/// Catppuccin. What must not differ is that every color the block paints is
/// either plainly another one or plainly not: no near-misses, which is what
/// [`separated`] is asked, twice, above, and what
/// `no_painted_color_is_a_near_miss_for_another` holds every palette to.
///
/// Comments are outside that guarantee, and deliberately: they take the
/// theme's own `muted_foreground`, which Gruvbox, Nord and Tokyo Night hold
/// 20–31 channels off `foreground`. How far a theme de-emphasises its muted
/// text is the theme's decision, not this module's to overrule.
fn name_and_keyword(theme: &Theme) -> (Color, Color) {
    let literals = to_floor(theme.warning, theme);
    // Furthest from the body text first. A distance that cannot be measured —
    // `Theme::terminal` paints in colors this cannot read — sorts as the
    // furthest, and the stable sort then leaves `primary` where it was.
    let mut accents = [
        to_floor(theme.primary, theme),
        to_floor(theme.accent, theme),
    ];
    accents.sort_by(|a, b| {
        let apart =
            |color: &Color| perceptual_distance(*color, theme.foreground).unwrap_or(f64::INFINITY);
        apart(b).total_cmp(&apart(a))
    });
    let usable = |color: Color| {
        (separated(color, theme.foreground) && separated(color, literals)).then_some(color)
    };
    let [name, keyword] = accents;
    (
        usable(name).unwrap_or(theme.foreground),
        usable(keyword).unwrap_or(theme.foreground),
    )
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

/// `color` in CIELAB, or [`None`] when it has no channels to read.
///
/// [`Theme::terminal`] is the palette that answers [`None`]: it leaves the
/// background and the body text as [`Color::Reset`], which is the terminal's
/// own and carries nothing to measure. Its accents are ordinary *named* colors,
/// which resolve fine — so a comparison against them is real arithmetic and
/// passes on the palette's merits, not by exemption.
fn lab(color: Color) -> Option<(f64, f64, f64)> {
    let (red, green, blue) = color::resolve_rgb(color)?;
    // sRGB, undo the transfer function, to CIE XYZ under D65, to Lab.
    let linear = |channel: u8| {
        let channel = f64::from(channel) / 255.0;
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    let (red, green, blue) = (linear(red), linear(green), linear(blue));
    let x = (0.412_456_4 * red + 0.357_576_1 * green + 0.180_437_5 * blue) / 0.950_489;
    let y = 0.212_672_9 * red + 0.715_152_2 * green + 0.072_175_0 * blue;
    let z = (0.019_333_9 * red + 0.119_192_0 * green + 0.950_304_1 * blue) / 1.088_840;
    let f = |t: f64| {
        if t > 216.0 / 24_389.0 {
            t.cbrt()
        } else {
            t.mul_add(841.0 / 108.0, 4.0 / 29.0)
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    Some((
        fy.mul_add(116.0, -16.0),
        500.0 * (fx - fy),
        200.0 * (fy - fz),
    ))
}

/// How far apart two colors look, in CIE76 ΔE, or [`None`] when one of them has
/// no channels to read.
///
/// Perceptual, and that is the whole point. Max-channel distance — what this
/// replaced — counts a lightness step and a hue shift as the same quantity, so
/// it could not tell `default_dark`'s near-white `primary` 21 channels off its
/// body text (invisible) from Catppuccin Mocha's pink `accent` 33 channels off
/// its own (plainly a color). In ΔE those are 7.33 and 22.19, and no threshold
/// on channels separates them from Iceberg's 7.45 and Zenburn's 6.60, which are
/// equally invisible and were being admitted.
fn perceptual_distance(a: Color, b: Color) -> Option<f64> {
    let (a, b) = (lab(a)?, lab(b)?);
    Some(
        (b.2 - a.2)
            .mul_add(
                b.2 - a.2,
                (b.0 - a.0).mul_add(b.0 - a.0, (b.1 - a.1) * (b.1 - a.1)),
            )
            .sqrt(),
    )
}

/// Whether two colors are far enough apart to read as two colors. See
/// [`VISIBLE_STEP`].
///
/// A distance nothing can measure is taken as separated: the unreadable half is
/// the user's own terminal color, and second-guessing it is out of scope.
fn separated(a: Color, b: Color) -> bool {
    perceptual_distance(a, b).is_none_or(|apart| apart >= VISIBLE_STEP)
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

    /// The themes the mapping has to hold for: the presets, plus solved ones —
    /// the case the showcase itself runs, and the one the presets cannot stand
    /// in for, since an adaptive palette is not known until it runs.
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
        themes.push(Theme::adaptive(
            Color::Rgb(90, 90, 95),
            Color::Rgb(200, 200, 205),
            None,
        ));
        themes.extend(PALETTES.iter().map(Palette::solve));
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

    /// Where a palette has one usable accent, it goes to the names — on an API
    /// tour those are the words the reader came for — and the keywords take the
    /// body color rather than something that would look like the literals.
    #[test]
    fn a_palette_with_one_hue_to_spare_spends_it_on_names_not_keywords() {
        for theme in [
            Theme::default_dark(),
            Theme::gruvbox(),
            Theme::adaptive(Color::Rgb(10, 10, 10), Color::Rgb(250, 250, 250), None),
        ] {
            assert_eq!(
                role(Kind::Type, &theme),
                to_floor(theme.accent, &theme),
                "{}: the one hue left should paint the names",
                theme.name
            );
            assert_eq!(
                role(Kind::Keyword, &theme),
                role(Kind::Text, &theme),
                "{}: and the keywords are what give theirs up",
                theme.name
            );
        }
        // A palette with two usable accents keeps both, or this would be a
        // blanket rule rather than a fallback. Its mauve `accent` stands
        // further off its pale blue body text than its blue `primary` does —
        // 34.65 ΔE against 26.87 — so the mauve is what the names take.
        let theme = Theme::catppuccin();
        assert_eq!(role(Kind::Type, &theme), theme.accent);
        assert_eq!(role(Kind::Keyword, &theme), theme.primary);
    }

    /// Names and keywords are never nearly alike.
    ///
    /// This is the one pair nothing else constrains. [`name_and_keyword`] holds
    /// each accent away from the body text and away from the literals, so those
    /// pairs are true by construction and asserting them here would only
    /// restate the floor. Whether the two accents are far enough from *each
    /// other* is asked nowhere else, and a palette that failed it would paint
    /// `Button` and `let` in two shades of the same color.
    ///
    /// Measured against a step of its own rather than by calling
    /// [`separated`]: a test whose arbiter is the code under test cannot fail,
    /// which is exactly how a floor that admitted Iceberg's invisible keywords
    /// went unnoticed. This number is the one an editor's own two accents would
    /// have to clear, not the one the mapping happens to use.
    const ACCENTS_APART: f64 = 10.0;

    #[test]
    fn names_and_keywords_are_never_two_shades_of_the_same_color() {
        for theme in themes() {
            let (name, keyword) = name_and_keyword(&theme);
            if name == keyword {
                // Both fell back to the body text: the palette has no accent to
                // spare, which is a different outcome from a near-miss.
                assert_eq!(name, theme.foreground);
                continue;
            }
            let Some(apart) = perceptual_distance(name, keyword) else {
                continue; // A color the terminal owns, which is not ours to measure.
            };
            assert!(
                apart >= ACCENTS_APART,
                "{}: names and keywords are {apart:.2} ΔE apart — near enough to look \
                 like a mistake, far enough to look deliberate",
                theme.name
            );
        }
    }

    /// A real terminal palette, in the two colors `Theme::adaptive` is given
    /// plus the sixteen it solves the hued roles from.
    struct Palette {
        name: &'static str,
        background: u32,
        foreground: u32,
        ansi: [u32; 16],
    }

    impl Palette {
        fn solve(&self) -> Theme {
            let hex = |value: u32| {
                Color::Rgb(
                    (value >> 16) as u8,
                    ((value >> 8) & 0xff) as u8,
                    (value & 0xff) as u8,
                )
            };
            let ansi: [Color; 16] = std::array::from_fn(|index| hex(self.ansi[index]));
            Theme::adaptive(hex(self.background), hex(self.foreground), Some(&ansi))
        }
    }

    /// Real palettes people run terminals in, which is the population the
    /// showcase actually meets: it is `ADAPTIVE`, so every one of these is
    /// solved at runtime and none of them is a preset.
    const PALETTES: &[Palette] = &[
        Palette {
            name: "Catppuccin Mocha",
            background: 0x001e_1e2e & 0xff_ffff,
            foreground: 0x00cd_d6f4 & 0xff_ffff,
            ansi: [
                0x0045_475a,
                0x00f3_8ba8,
                0x00a6_e3a1,
                0x00f9_e2af,
                0x0089_b4fa,
                0x00f5_c2e7,
                0x0094_e2d5,
                0x00ba_c2de,
                0x0058_5b70,
                0x00f3_8ba8,
                0x00a6_e3a1,
                0x00f9_e2af,
                0x0089_b4fa,
                0x00f5_c2e7,
                0x0094_e2d5,
                0x00a6_adc8,
            ],
        },
        Palette {
            name: "Catppuccin Latte",
            background: 0x00ef_f1f5,
            foreground: 0x004c_4f69,
            ansi: [
                0x005c_5f77,
                0x00d2_0f39,
                0x0040_a02b,
                0x00df_8e1d,
                0x001e_66f5,
                0x00ea_76cb,
                0x0017_9299,
                0x00ac_b0be,
                0x006c_6f85,
                0x00d2_0f39,
                0x0040_a02b,
                0x00df_8e1d,
                0x001e_66f5,
                0x00ea_76cb,
                0x0017_9299,
                0x00bc_c0cc,
            ],
        },
        Palette {
            name: "Iceberg",
            background: 0x0016_1821,
            foreground: 0x00c6_c8d1,
            ansi: [
                0x001e_2132,
                0x00e2_7878,
                0x00b4_be82,
                0x00e2_a478,
                0x0084_a0c6,
                0x00a0_93c7,
                0x0089_b8c2,
                0x00c6_c8d1,
                0x006b_7089,
                0x00e9_8989,
                0x00c0_ca8e,
                0x00e9_b189,
                0x0091_acd1,
                0x00ad_a0d3,
                0x0095_c4ce,
                0x00d2_d4de,
            ],
        },
        Palette {
            name: "Zenburn",
            background: 0x003f_3f3f,
            foreground: 0x00dc_dccc,
            ansi: [
                0x004d_4d4d,
                0x0070_5050,
                0x0060_b48a,
                0x00df_af8f,
                0x0050_6070,
                0x00dc_8cc3,
                0x008c_d0d3,
                0x00dc_dccc,
                0x0070_9080,
                0x00dc_a3a3,
                0x00c3_bf9f,
                0x00f0_dfaf,
                0x0094_bff3,
                0x00ec_93d3,
                0x0093_e0e3,
                0x00ff_ffff,
            ],
        },
        Palette {
            name: "Dracula",
            background: 0x0028_2a36,
            foreground: 0x00f8_f8f2,
            ansi: [
                0x0021_222c,
                0x00ff_5555,
                0x0050_fa7b,
                0x00f1_fa8c,
                0x00bd_93f9,
                0x00ff_79c6,
                0x008b_e9fd,
                0x00f8_f8f2,
                0x0062_72a4,
                0x00ff_6e6e,
                0x0069_ff94,
                0x00ff_ffa5,
                0x00d6_acff,
                0x00ff_92df,
                0x00a4_ffff,
                0x00ff_ffff,
            ],
        },
        Palette {
            name: "One Dark",
            background: 0x0028_2c34,
            foreground: 0x00ab_b2bf,
            ansi: [
                0x0028_2c34,
                0x00e0_6c75,
                0x0098_c379,
                0x00e5_c07b,
                0x0061_afef,
                0x00c6_78dd,
                0x0056_b6c2,
                0x00ab_b2bf,
                0x005c_6370,
                0x00e0_6c75,
                0x0098_c379,
                0x00e5_c07b,
                0x0061_afef,
                0x00c6_78dd,
                0x0056_b6c2,
                0x00ff_ffff,
            ],
        },
        Palette {
            name: "Rose Pine",
            background: 0x0019_1724,
            foreground: 0x00e0_def4,
            ansi: [
                0x0026_233a,
                0x00eb_6f92,
                0x0031_748f,
                0x00f6_c177,
                0x009c_cfd8,
                0x00c4_a7e7,
                0x00eb_bcba,
                0x00e0_def4,
                0x006e_6a86,
                0x00eb_6f92,
                0x0031_748f,
                0x00f6_c177,
                0x009c_cfd8,
                0x00c4_a7e7,
                0x00eb_bcba,
                0x00e0_def4,
            ],
        },
        Palette {
            name: "Ayu Dark",
            background: 0x000b_0e14,
            foreground: 0x00bf_bdb6,
            ansi: [
                0x000d_1016,
                0x00ea_6c73,
                0x007f_d962,
                0x00f9_af4f,
                0x0053_bdfa,
                0x00cd_a1fa,
                0x0090_e1c6,
                0x00bf_bdb6,
                0x0056_5b66,
                0x00ea_6c73,
                0x007f_d962,
                0x00f9_af4f,
                0x0053_bdfa,
                0x00cd_a1fa,
                0x0090_e1c6,
                0x00bf_bdb6,
            ],
        },
    ];

    fn palette(name: &str) -> Theme {
        PALETTES
            .iter()
            .find(|palette| palette.name == name)
            .expect("a palette by that name")
            .solve()
    }

    /// The cases the metric has to get right, and why it is perceptual.
    ///
    /// These four defeated every version of this rule that measured channels.
    /// `default_dark` sits 21 channels off its body text and Catppuccin Mocha's
    /// accent 33, so a channel floor between them looked like the answer — but
    /// Iceberg is 21 and Zenburn 19, both as invisible as `default_dark`, and
    /// both were admitted by a floor low enough to keep Mocha. In ΔE the three
    /// invisible ones are 7.45, 7.33 and 6.60 — indistinguishable from each
    /// other — and Mocha's pink is 22.19, which is a different kind of
    /// difference and not merely a larger one.
    #[test]
    fn the_metric_declines_what_no_reader_can_see_and_admits_what_they_can() {
        let dark = Theme::default_dark();
        let declined: [(&str, Theme, Color); 3] = [
            ("default_dark", dark, dark.primary),
            ("Iceberg", palette("Iceberg"), palette("Iceberg").primary),
            ("Zenburn", palette("Zenburn"), palette("Zenburn").primary),
        ];
        for (name, theme, candidate) in declined {
            let candidate = to_floor(candidate, &theme);
            let apart =
                perceptual_distance(candidate, theme.foreground).expect("a solved palette is rgb");
            assert!(
                apart < VISIBLE_STEP,
                "{name}: {apart:.2} ΔE is not below the step, so this case has stopped \
                 being the one it was written for"
            );
            assert_eq!(
                role(Kind::Keyword, &theme),
                theme.foreground,
                "{name}: a near-neutral {apart:.2} ΔE off the body text is not a color \
                 anyone can read in the text, and painting keywords in it is worse than \
                 painting them plain"
            );
        }

        let mocha = palette("Catppuccin Mocha");
        let accent = to_floor(mocha.accent, &mocha);
        let apart = perceptual_distance(accent, mocha.foreground).expect("a solved palette is rgb");
        assert!(
            apart >= VISIBLE_STEP,
            "Catppuccin Mocha: its pink accent measures {apart:.2} ΔE"
        );
        assert_eq!(
            role(Kind::Type, &mocha),
            accent,
            "a pink at {apart:.2} ΔE is a color, and throwing it away left one of the \
             most-used terminal themes with literals and comments and nothing else"
        );
    }

    /// The comparison has no threshold to fall off.
    ///
    /// Catppuccin Latte is why: its `primary` sits 41 channels from its body
    /// text, one the wrong side of a 40-channel cut, so a mapping that picked
    /// the name color by threshold painted its names near-black while `let` and
    /// `if` got a loud purple. Nothing about 41 is special, and no other number
    /// is either — across a spread of `Theme::adaptive` solves the distance
    /// takes very nearly every value there is.
    #[test]
    fn the_name_color_is_whichever_accent_stands_out_further() {
        for theme in themes() {
            let (name, _) = name_and_keyword(&theme);
            let (primary, accent) = (
                to_floor(theme.primary, &theme),
                to_floor(theme.accent, &theme),
            );
            let distance =
                |color| perceptual_distance(color, theme.foreground).unwrap_or(f64::INFINITY);
            let (far, near) = if distance(accent) > distance(primary) {
                (accent, primary)
            } else {
                (primary, accent)
            };
            assert!(
                name == far || name == theme.foreground,
                "{}: the names took the accent nearer the body text",
                theme.name
            );
            assert_ne!(
                name, near,
                "{}: the names took the accent nearer the body text",
                theme.name
            );
        }
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
