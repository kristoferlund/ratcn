//! Shared marker vocabulary for value-keyed selection controls.
//!
//! [`List`](crate::List) and [`Select`](crate::Select) paint their rows
//! independently, but both use the same single- and multi-selection glyphs, the
//! same disabled-over-selected color precedence, and the same default row line.
//! Keeping those small visual rules here prevents the two components from
//! drifting without coupling either component to the other's paint widget.

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

/// Colors available when resolving one selection marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerColors {
    /// Color used when the item is disabled.
    pub disabled: Color,
    /// Color used when the item is selected and enabled.
    pub selected: Color,
    /// Color used when the item is unselected and enabled.
    pub unselected: Color,
}

/// The glyph pair one selection control paints: what a selected item shows and
/// what an unselected one does.
///
/// The presets name the two shapes a selection takes — a lone circle for
/// picking one of many, a box for ticking any number — and any other pair can
/// be supplied where a control offers its markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerGlyphs<'a> {
    /// The marker on a selected item.
    pub selected: &'a str,
    /// The marker on an unselected item.
    pub unselected: &'a str,
}

impl MarkerGlyphs<'_> {
    /// The circles a pick-one control paints by default.
    #[must_use]
    pub const fn radio() -> Self {
        Self {
            selected: "●",
            unselected: "○",
        }
    }

    /// The boxes a tick-any-number control paints by default.
    #[must_use]
    pub const fn checkbox() -> Self {
        Self {
            selected: "■",
            unselected: "□",
        }
    }
}

/// Marker glyph for a selected or unselected item.
#[must_use]
pub const fn marker(selected: bool, glyphs: MarkerGlyphs<'_>) -> &str {
    if selected {
        glyphs.selected
    } else {
        glyphs.unselected
    }
}

/// Marker color with disabled state taking precedence over selection.
#[must_use]
pub const fn color(disabled: bool, selected: bool, colors: MarkerColors) -> Color {
    if disabled {
        colors.disabled
    } else if selected {
        colors.selected
    } else {
        colors.unselected
    }
}

/// The default row of a selection control: a leading space, the [`marker`], a
/// space, then `label`.
///
/// One line, built here rather than in each control, so a list row and a select
/// option indent identically and their labels start in the same column. Only the
/// marker is colored — from [`color`], so disabled still wins over selected. The
/// label carries no color of its own, leaving it to inherit whatever row style
/// is painted beneath it.
///
/// `glyphs` picks the pair: [`MarkerGlyphs::radio`] where one option wins,
/// [`MarkerGlyphs::checkbox`] where any number can be ticked, or whatever the
/// app supplies.
#[must_use]
pub fn marker_line(
    label: &str,
    selected: bool,
    disabled: bool,
    colors: MarkerColors,
    glyphs: MarkerGlyphs<'_>,
) -> Line<'static> {
    let marker = marker(selected, glyphs);
    Line::from(vec![
        Span::styled(
            format!(" {marker}"),
            Style::new().fg(color(disabled, selected, colors)),
        ),
        Span::raw(" "),
        Span::raw(label.to_owned()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one row both selection controls draw: the label's column and the
    /// marker's color are what a change here would move.
    #[test]
    fn marker_line_indents_the_label_and_colors_only_the_marker() {
        let colors = MarkerColors {
            disabled: Color::DarkGray,
            selected: Color::Green,
            unselected: Color::Blue,
        };
        let line = marker_line("Alpha", true, false, colors, MarkerGlyphs::radio());

        assert_eq!(line.to_string(), " ● Alpha");
        assert_eq!(line.spans[0].style.fg, Some(Color::Green));
        assert_eq!(
            line.spans.last().expect("a label span").style.fg,
            None,
            "the label inherits the row style painted beneath it"
        );
        assert_eq!(
            marker_line("Alpha", true, true, colors, MarkerGlyphs::checkbox()).spans[0]
                .style
                .fg,
            Some(Color::DarkGray),
            "disabled wins over selected"
        );
        assert_eq!(
            marker_line("Alpha", true, false, colors, MarkerGlyphs::checkbox()).to_string(),
            " ■ Alpha"
        );
    }

    /// A control's markers are the app's to choose: the same call paints any
    /// pair it is handed.
    #[test]
    fn the_glyph_pair_is_yours_to_choose() {
        let glyphs = MarkerGlyphs {
            selected: "[x]",
            unselected: "[ ]",
        };
        let colors = MarkerColors {
            disabled: Color::DarkGray,
            selected: Color::Green,
            unselected: Color::Gray,
        };
        assert_eq!(
            marker_line("Vim bindings", true, false, colors, glyphs).to_string(),
            " [x] Vim bindings"
        );
    }
}
