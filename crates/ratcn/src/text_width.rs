//! Cell-width text measurement shared by components and the runtime.
//!
//! Terminal cells are not characters: CJK characters and many emoji occupy
//! two cells, combining marks occupy none. Everything that sizes, centers, or
//! truncates text against an area measures through here so layout and paint
//! can never disagree.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The number of terminal cells `text` occupies on one row.
#[must_use]
pub fn display_width(text: &str) -> usize {
    text.width()
}

/// [`display_width`] clamped into `u16` for rect arithmetic. The clamp
/// saturates; combine it with saturating arithmetic at the call site.
#[must_use]
pub fn display_width_u16(text: &str) -> u16 {
    u16::try_from(display_width(text)).unwrap_or(u16::MAX)
}

/// The longest prefix of `text` that fits in `width` cells, cut on a grapheme
/// cluster boundary. Never splits a multi-code-point glyph or a wide glyph's
/// two cells, so the prefix may come up one cell short of `width`.
#[must_use]
pub fn truncate_to_width(text: &str, width: usize) -> &str {
    if display_width(text) <= width {
        return text;
    }
    let mut end = 0;
    for (idx, grapheme) in text.grapheme_indices(true) {
        let next = idx + grapheme.len();
        if display_width(&text[..next]) > width {
            break;
        }
        end = next;
    }
    &text[..end]
}

/// `text` word-wrapped into rows of at most `width` cells. Explicit line breaks
/// are preserved. Within each line, wrapping breaks at spaces and trims them at
/// row edges; a word wider than a row hard-breaks on a grapheme cluster boundary,
/// and a single glyph wider than the row is emitted alone rather than dropped.
/// A zero `width` or empty `text` yields one row so callers still count a
/// rendered line.
#[must_use]
pub fn wrap_to_width(text: &str, width: usize) -> Vec<&str> {
    text.split('\n')
        .flat_map(|line| wrap_line_to_width(line, width))
        .collect()
}

fn wrap_line_to_width(text: &str, width: usize) -> Vec<&str> {
    let mut rest = trim_space_graphemes(text);
    if width == 0 || rest.is_empty() {
        return vec![rest];
    }
    let mut lines = Vec::new();
    loop {
        if display_width(rest) <= width {
            lines.push(rest);
            return lines;
        }
        let prefix = truncate_to_width(rest, width);
        let split = if rest[prefix.len()..].starts_with(' ') {
            // The row ends exactly on a word boundary.
            prefix.len()
        } else {
            match prefix
                .grapheme_indices(true)
                .rfind(|(_, grapheme)| grapheme.starts_with(' '))
                .map(|(idx, _)| idx)
            {
                Some(idx) => idx,
                // One word overflows the row: hard-break it. An empty prefix
                // means even the first glyph is wider than the row; emit that
                // glyph alone so the loop always advances.
                None if prefix.is_empty() => {
                    rest.graphemes(true).next().map_or(rest.len(), str::len)
                }
                None => prefix.len(),
            }
        };
        let (line, tail) = rest.split_at(split);
        lines.push(trim_end_space_graphemes(line));
        rest = trim_start_space_graphemes(tail);
        if rest.is_empty() {
            return lines;
        }
    }
}

fn trim_space_graphemes(text: &str) -> &str {
    trim_end_space_graphemes(trim_start_space_graphemes(text))
}

fn trim_start_space_graphemes(text: &str) -> &str {
    let start = text
        .grapheme_indices(true)
        .find(|(_, grapheme)| !grapheme.starts_with(' '))
        .map_or(text.len(), |(idx, _)| idx);
    &text[start..]
}

fn trim_end_space_graphemes(text: &str) -> &str {
    let end = text
        .grapheme_indices(true)
        .rfind(|(_, grapheme)| !grapheme.starts_with(' '))
        .map_or(0, |(idx, grapheme)| idx + grapheme.len());
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_width_counts_cells_not_chars_or_bytes() {
        assert_eq!(display_width("åβ"), 2, "narrow multi-byte chars");
        assert_eq!(display_width("日本"), 4, "wide CJK chars");
        assert_eq!(display_width("e\u{301}"), 1, "combining mark adds no cell");
        assert_eq!(display_width("🚀"), 2, "emoji are 2 cells");
        assert_eq!(
            display_width("👩\u{200d}👩\u{200d}👦"),
            2,
            "a ZWJ sequence renders as one 2-cell glyph"
        );
    }

    #[test]
    fn truncate_to_width_never_splits_a_grapheme_cluster() {
        assert_eq!(truncate_to_width("日本語", 6), "日本語");
        assert_eq!(truncate_to_width("日本語", 4), "日本");
        assert_eq!(truncate_to_width("日本語", 3), "日", "no half cells");
        assert_eq!(truncate_to_width("日本語", 0), "");
        assert_eq!(truncate_to_width("🚀X", 1), "", "no half emoji");
        assert_eq!(truncate_to_width("e\u{301}f", 1), "e\u{301}");
        assert_eq!(truncate_to_width("🇸🇪X", 1), "", "no half flag");
        assert_eq!(
            truncate_to_width("❤️X", 1),
            "",
            "no split variation-selector sequence"
        );
    }

    #[test]
    fn wrap_to_width_breaks_at_spaces_and_trims_row_edges() {
        assert_eq!(wrap_to_width("one two three", 20), vec!["one two three"]);
        assert_eq!(wrap_to_width("one two three", 7), vec!["one two", "three"]);
        assert_eq!(wrap_to_width("one  two", 3), vec!["one", "two"]);
        assert_eq!(wrap_to_width(" padded ", 20), vec!["padded"]);
    }

    #[test]
    fn wrap_to_width_hard_breaks_overlong_words() {
        assert_eq!(wrap_to_width("abcdef", 4), vec!["abcd", "ef"]);
        assert_eq!(wrap_to_width("日本語日本", 4), vec!["日本", "語日", "本"]);
        assert_eq!(
            wrap_to_width("🚀", 1),
            vec!["🚀"],
            "a glyph wider than the row is emitted alone, not dropped"
        );
        assert_eq!(wrap_to_width("🇸🇪X", 1), vec!["🇸🇪", "X"]);
        assert_eq!(wrap_to_width("❤️X", 1), vec!["❤️", "X"]);
    }

    #[test]
    fn wrap_to_width_trims_a_space_with_its_combining_mark() {
        assert_eq!(wrap_to_width("a \u{301}b", 1), vec!["a", "b"]);
    }

    #[test]
    fn wrap_to_width_preserves_explicit_lines() {
        assert_eq!(wrap_to_width("one\ntwo", 20), vec!["one", "two"]);
        assert_eq!(
            wrap_to_width("one\n\ntwo\n", 20),
            vec!["one", "", "two", ""]
        );
    }

    #[test]
    fn wrap_to_width_always_yields_a_row() {
        assert_eq!(wrap_to_width("", 10), vec![""]);
        assert_eq!(wrap_to_width("text", 0), vec!["text"]);
    }

    #[test]
    fn truncated_prefix_never_overflows_the_requested_width() {
        for text in [
            "ascii",
            "日本語",
            "🚀🚀",
            "👩\u{200d}👩\u{200d}👦!",
            "e\u{301}e\u{301}",
        ] {
            for width in 0..=6 {
                assert!(
                    display_width(truncate_to_width(text, width)) <= width,
                    "{text:?} truncated to {width} overflows"
                );
            }
        }
    }
}
