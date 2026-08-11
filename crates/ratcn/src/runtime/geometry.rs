//! Generic rect geometry components share: border hit-testing and wrapped-text
//! height. None of it is specific to any one component — see
//! [`drag`](super::drag) for dragging-specific geometry.
//!
//! Anything ratatui already answers stays with ratatui: shrinking an area by a
//! margin is `Rect::inner(Margin::new(x, y))`, not a helper here.

use ratatui::layout::{Position, Rect};

use crate::text_width::wrap_to_width;

/// Whether `(column, row)` falls on the outermost ring of cells in `area` —
/// the usual hit-test for a draggable or clickable border.
///
/// False for anything outside `area` and for a zero-width or zero-height area.
#[must_use]
pub fn is_border(area: Rect, column: u16, row: u16) -> bool {
    if !area.contains(Position { x: column, y: row }) || area.width == 0 || area.height == 0 {
        return false;
    }
    let right = area.x.saturating_add(area.width.saturating_sub(1));
    let bottom = area.y.saturating_add(area.height.saturating_sub(1));
    column == area.x || column == right || row == area.y || row == bottom
}

/// Number of lines `text` needs when word-wrapped to `width` terminal cells.
/// Explicit line breaks are preserved and words wider than a row are broken at
/// grapheme cluster boundaries. Empty text is 0 lines; a width of 0 is treated
/// as 1.
#[must_use]
pub fn wrapped_height(text: &str, width: u16) -> u16 {
    if text.is_empty() {
        return 0;
    }
    u16::try_from(wrap_to_width(text, usize::from(width.max(1))).len()).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_border_matches_only_the_edge_cells() {
        let area = Rect::new(2, 2, 4, 3);
        assert!(is_border(area, 2, 2), "top-left corner");
        assert!(is_border(area, 5, 2), "top-right corner");
        assert!(is_border(area, 3, 4), "bottom edge");
        assert!(!is_border(area, 3, 3), "interior");
        assert!(!is_border(area, 0, 0), "outside the area");
    }

    #[test]
    fn is_border_handles_edge_coordinates_without_overflowing() {
        let area = Rect::new(u16::MAX - 1, u16::MAX - 1, 1, 1);
        assert!(is_border(area, u16::MAX - 1, u16::MAX - 1));

        let overflowing_area = Rect::new(u16::MAX, u16::MAX, 1, 1);
        assert!(!is_border(overflowing_area, u16::MAX, u16::MAX));
    }

    #[test]
    fn wrapped_height_wraps_greedily_at_the_given_width() {
        assert_eq!(wrapped_height("", 10), 0);
        assert_eq!(wrapped_height("hello world", 20), 1);
        assert_eq!(wrapped_height("hello world", 8), 2);
    }

    #[test]
    fn wrapped_height_measures_words_in_cells_not_chars() {
        // "日本語" and "テスト" are 6 cells each (3 chars). By cells they need
        // 13 columns on one line; a char count would cram them into 8.
        assert_eq!(wrapped_height("日本語 テスト", 13), 1);
        assert_eq!(wrapped_height("日本語 テスト", 8), 2);
    }

    #[test]
    fn wrapped_height_counts_explicit_lines_and_hard_wrapped_words() {
        assert_eq!(wrapped_height("one\ntwo", 20), 2);
        assert_eq!(wrapped_height("abcdefghijklmnopqrstu", 10), 3);
    }
}
