//! Cell compositing shared by layer canvases and projected viewport paint.

use ratatui::{
    buffer::{Buffer, CellDiffOption, CellWidth},
    layout::{Position, Rect},
};

/// Copy mapped cells through a destination clip, blanking a glyph that would
/// cross its right edge. The clip is the composite boundary, not the paint
/// rectangle: a style-only paint may touch part of an otherwise intact glyph.
pub(super) fn copy_cells(
    source: &Buffer,
    destination: &mut Buffer,
    positions: impl Iterator<Item = (Position, Position)>,
    clip: Rect,
) {
    let clip = clip.intersection(destination.area);
    for (from, to) in positions {
        if !clip.contains(to) {
            continue;
        }
        let Some(cell) = source.cell(from) else {
            continue;
        };
        let target = &mut destination[to];
        *target = cell.clone();
        if cell.cell_width() > clip.right() - to.x {
            target.set_symbol(" ").set_diff_option(CellDiffOption::None);
        }
    }
}
