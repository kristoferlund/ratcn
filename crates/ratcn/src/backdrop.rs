use crate::color::{dim, resolve_rgb};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

/// How far each channel moves toward the backdrop color, as a percentage.
/// Dimming past a midpoint blend keeps the base layer readable behind a modal.
const DIM_PERCENT: u16 = 62;

/// Dims an already-rendered background area before drawing an overlay.
///
/// Unlike applying [`Modifier::DIM`] directly, this blends both foreground and
/// background colors for RGB/named colors. That keeps filled widgets whose
/// shapes use foreground glyphs and background fills visually consistent.
pub(crate) fn dim_background(buffer: &mut Buffer, area: Rect, background: Color) {
    if resolve_rgb(background).is_none() {
        buffer.set_style(area, Style::default().add_modifier(Modifier::DIM));
        return;
    }

    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            let Some(cell) = buffer.cell_mut((x, y)) else {
                continue;
            };
            cell.fg = dim(cell.fg, background, DIM_PERCENT);
            cell.bg = dim(cell.bg, background, DIM_PERCENT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::dim_background;
    use ratatui::{buffer::Buffer, layout::Rect, style::Color};

    /// The one number the backdrop has, pinned to what it produces. A modal's
    /// base layer has to sit far enough back to read as behind the dialog, and
    /// stay near enough to read at all.
    #[test]
    fn both_of_a_cells_colors_land_most_of_the_way_to_the_backdrop() {
        let area = Rect::new(0, 0, 1, 1);
        let mut buffer = Buffer::empty(area);
        let cell = buffer
            .cell_mut((0, 0))
            .expect("the buffer covers this cell");
        cell.fg = Color::Rgb(250, 250, 250);
        cell.bg = Color::Rgb(30, 30, 30);

        dim_background(&mut buffer, area, Color::Rgb(10, 10, 10));

        let cell = buffer.cell((0, 0)).expect("the buffer covers this cell");
        assert_eq!(cell.fg, Color::Rgb(101, 101, 101));
        assert_eq!(cell.bg, Color::Rgb(18, 18, 18));
    }
}
