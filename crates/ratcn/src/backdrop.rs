use crate::color::resolve_rgb;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

/// Dims an already-rendered background area before drawing an overlay.
///
/// Unlike applying [`Modifier::DIM`] directly, this blends both foreground and
/// background colors for RGB/named colors. That keeps filled widgets whose
/// shapes use foreground glyphs and background fills visually consistent.
pub(crate) fn dim_background(buffer: &mut Buffer, area: Rect, background: Color) {
    let Some(target) = resolve_rgb(background) else {
        buffer.set_style(area, Style::default().add_modifier(Modifier::DIM));
        return;
    };

    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            let Some(cell) = buffer.cell_mut((x, y)) else {
                continue;
            };
            cell.fg = dim_color(cell.fg, target);
            cell.bg = dim_color(cell.bg, target);
        }
    }
}

fn dim_color(color: Color, target: (u8, u8, u8)) -> Color {
    let Some((r, g, b)) = resolve_rgb(color) else {
        return color;
    };

    Color::Rgb(
        blend_channel(r, target.0),
        blend_channel(g, target.1),
        blend_channel(b, target.2),
    )
}

/// How far each channel moves toward the backdrop color, in eighths.
/// 5/8 dims noticeably more than a midpoint blend while keeping the base
/// layer readable behind a modal.
const DIM_EIGHTHS: u16 = 5;

fn blend_channel(channel: u8, target: u8) -> u8 {
    let blended =
        (u16::from(channel) * (8 - DIM_EIGHTHS) + u16::from(target) * DIM_EIGHTHS + 4) / 8;
    u8::try_from(blended).expect("a weighted average of two u8 channels fits in u8")
}
