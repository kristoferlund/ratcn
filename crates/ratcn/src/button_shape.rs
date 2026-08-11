//! The shared pixels of the button idiom: half-block cap rows, the centered
//! filled middle row, and the width formula.
//!
//! `Button` paints with these directly; `Tabs` uses the same vocabulary
//! because a tab is painted as a button. Sharing the code here keeps the two
//! looks from drifting without one component depending on the other.

use std::borrow::Cow;

use ratatui::style::Color;

use crate::text_width::{display_width, display_width_u16, truncate_to_width};

/// Glyph for the top cap row of the large shape.
pub const TOP_CAP: &str = "▄";
/// Glyph for the bottom cap row of the large shape.
pub const BOTTOM_CAP: &str = "▀";

/// A cap row `width` cells wide, to be styled with the fill as foreground.
///
/// The half-block glyphs paint in the fill color; their other half leaves the
/// cell background untouched, so it inherits whatever surface the control sits
/// on (a pane, a dialog). A cap whose fill is [`Color::Reset`] would paint as
/// the terminal foreground rather than blend away, so it renders blank
/// instead.
#[must_use]
pub fn cap_row(fill: Color, symbol: &str, width: usize) -> String {
    if fill == Color::Reset {
        " ".repeat(width)
    } else {
        symbol.repeat(width)
    }
}

/// The label centered in `width` cells with spaces.
///
/// The padding is what carries the fill: a `Line` styles only the cells it
/// renders, so every cell up to `width` must be part of the string. A label
/// too wide for the row is truncated on a grapheme cluster boundary and padded
/// back out — truncation never splits a wide char's cells, so the prefix can
/// come up short of `width`.
#[must_use]
pub fn filled_middle(label: &str, width: usize) -> Cow<'_, str> {
    let label_width = display_width(label);
    if label_width == width {
        return Cow::Borrowed(label);
    }
    if width < label_width {
        let truncated = truncate_to_width(label, width);
        let mut middle = String::with_capacity(width);
        middle.push_str(truncated);
        middle.extend(std::iter::repeat_n(' ', width - display_width(truncated)));
        return Cow::Owned(middle);
    }
    let remaining = width - label_width;
    let left = remaining / 2;
    let right = remaining - left;
    let mut middle = String::with_capacity(left + label.len() + right);
    middle.extend(std::iter::repeat_n(' ', left));
    middle.push_str(label);
    middle.extend(std::iter::repeat_n(' ', right));
    Cow::Owned(middle)
}

/// Columns the shape needs: the label in terminal cells, plus two cells of
/// padding on each side.
#[must_use]
pub fn shape_width(label: &str) -> u16 {
    display_width_u16(label).saturating_add(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filled_middle_centers_and_fills_every_cell() {
        assert_eq!(filled_middle("ok", 6).as_ref(), "  ok  ");
        assert_eq!(filled_middle("ok", 7).as_ref(), "  ok   ");
        assert_eq!(filled_middle("ok", 2).as_ref(), "ok");
    }

    #[test]
    fn too_narrow_rows_truncate_and_pad_to_exact_width() {
        // "日" is two cells wide; a 3-cell row fits one glyph plus a pad space,
        // so the fill still spans the whole row.
        assert_eq!(filled_middle("日本", 3).as_ref(), "日 ");
        assert_eq!(filled_middle("wide", 3).as_ref(), "wid");
    }

    #[test]
    fn reset_fill_renders_blank_caps() {
        assert_eq!(cap_row(Color::Reset, TOP_CAP, 3), "   ");
        assert_eq!(cap_row(Color::Rgb(1, 2, 3), BOTTOM_CAP, 3), "▀▀▀");
    }
}
