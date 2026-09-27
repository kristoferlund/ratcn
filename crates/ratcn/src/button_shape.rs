//! The shared pixels of the button idiom: the filled shape's colors and how it
//! paints — half-block cap rows around a centered, filled label row — and the
//! width formula.
//!
//! `Button` paints its filled variants with these; `Tabs` paints every tab
//! with them, because a tab is painted as a button. Sharing the code here keeps
//! the two looks from drifting without one component depending on the other.

use std::borrow::Cow;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::Widget,
};

use crate::Theme;
use crate::color::{DISABLED_DIM, FOCUS_SHIFT, HOVER_SHIFT, dim};
use crate::text_width::{display_width, display_width_u16, truncate_to_width};

/// The colors of a filled shape in each interaction state: a label on a fill,
/// which is also the color of the caps.
///
/// Disabled wins first, then hovered, then focused, then the resting colors.
/// Hover beating focus is what keeps pointing at an already-focused control
/// visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilledStyle {
    /// Label color at rest.
    pub foreground: Color,
    /// Fill at rest.
    pub background: Color,
    /// Label color while focused.
    pub focused_foreground: Color,
    /// Fill while focused.
    pub focused_background: Color,
    /// Label color while hovered.
    pub hovered_foreground: Color,
    /// Fill while hovered.
    pub hovered_background: Color,
    /// Label color while disabled.
    pub disabled_foreground: Color,
    /// Fill while disabled.
    pub disabled_background: Color,
}

impl FilledStyle {
    /// A `fill` labelled in `foreground`, as a theme paints it: focus and hover
    /// shift the fill a fixed amount toward `shift`, and disabled keeps its hue
    /// dimmed toward the theme's surface, with the muted label.
    ///
    /// `shift` is where the fill reads as moving: toward the end the screen
    /// sits at for a loud fill that should read as pressed, away from it for a
    /// quiet one that should read as raised. The label stays put, since the
    /// fill already carries the state.
    #[must_use]
    pub const fn themed(fill: Color, foreground: Color, shift: Color, theme: &Theme) -> Self {
        Self {
            foreground,
            background: fill,
            focused_foreground: foreground,
            focused_background: dim(fill, shift, FOCUS_SHIFT),
            hovered_foreground: foreground,
            hovered_background: dim(fill, shift, HOVER_SHIFT),
            disabled_foreground: theme.muted_foreground,
            disabled_background: dim(fill, theme.surface, DISABLED_DIM),
        }
    }

    /// The label color and fill for one paint, from the control's state.
    #[must_use]
    pub const fn resolve(&self, focused: bool, hovered: bool, disabled: bool) -> (Color, Color) {
        if disabled {
            (self.disabled_foreground, self.disabled_background)
        } else if hovered {
            (self.hovered_foreground, self.hovered_background)
        } else if focused {
            (self.focused_foreground, self.focused_background)
        } else {
            (self.foreground, self.background)
        }
    }
}

/// The style a label paints in over `background`. A [`Color::Reset`]
/// background is left unset, so the surface the control sits on shows through
/// instead of the terminal default.
#[must_use]
pub fn label_style(foreground: Color, background: Color) -> Style {
    let style = Style::default().fg(foreground);
    if background == Color::Reset {
        style
    } else {
        style.bg(background)
    }
}

/// Paint the filled shape into `area`: `label` centered on a row of `fill`,
/// with a cap row above and below when `large`. `area` is the shape's own
/// rect, one row tall or three.
pub fn paint_filled_shape(
    label: &str,
    large: bool,
    foreground: Color,
    fill: Color,
    area: Rect,
    buf: &mut Buffer,
) {
    let width = usize::from(area.width);
    if large {
        for (symbol, y) in [(TOP_CAP, area.y), (BOTTOM_CAP, area.y + 2)] {
            Line::from(cap_row(fill, symbol, width))
                .style(Style::default().fg(fill))
                .render(Rect::new(area.x, y, area.width, 1), buf);
        }
    }
    Line::from(filled_middle(label, width))
        .style(label_style(foreground, fill))
        .render(
            Rect::new(area.x, area.y + u16::from(large), area.width, 1),
            buf,
        );
}

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
