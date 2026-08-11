//! Shared marker vocabulary for value-keyed selection controls.
//!
//! [`List`](crate::List) and [`Select`](crate::Select) paint their rows
//! independently, but both use the same single- and multi-selection glyphs and
//! the same disabled-over-selected color precedence. Keeping those small visual
//! rules here prevents the two components from drifting without coupling either
//! component to the other's paint widget.

use ratatui::style::Color;

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
