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

/// Marker glyph for a selected or unselected item.
#[must_use]
pub const fn marker(selected: bool, multiple: bool) -> &'static str {
    match (multiple, selected) {
        (true, true) => "■",
        (true, false) => "□",
        (false, true) => "●",
        (false, false) => "○",
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
/// `multiple` picks the checkbox glyphs over the radio ones, as it does in
/// [`marker`].
#[must_use]
pub fn marker_line(
    label: &str,
    selected: bool,
    multiple: bool,
    disabled: bool,
    colors: MarkerColors,
) -> Line<'static> {
    let marker = marker(selected, multiple);
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
        let line = marker_line("Alpha", true, false, false, colors);

        assert_eq!(line.to_string(), " ● Alpha");
        assert_eq!(line.spans[0].style.fg, Some(Color::Green));
        assert_eq!(
            line.spans.last().expect("a label span").style.fg,
            None,
            "the label inherits the row style painted beneath it"
        );
        assert_eq!(
            marker_line("Alpha", true, true, true, colors).spans[0]
                .style
                .fg,
            Some(Color::DarkGray),
            "disabled wins over selected, and multiple picks the checkbox glyph"
        );
        assert_eq!(
            marker_line("Alpha", true, true, false, colors).to_string(),
            " ■ Alpha"
        );
    }
}
