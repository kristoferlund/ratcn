pub mod button_variants;
pub mod contributions;
pub mod notifications;
pub mod release;
pub mod shared;
pub mod themes;
pub mod tooltip;

use ratatui::layout::Rect;
use ratcn::runtime::{RenderCtx, ScopeOptions, TabWrap};

use crate::{AppMsg, AppState};

pub struct Tile {
    pub id: &'static str,
    /// A tile with controls is a Tab-trapping scope; a controls-free tile is
    /// a plain focusable leaf.
    pub has_controls: bool,
    pub render: fn(&mut RenderCtx<'_, '_, AppState, AppMsg>),
}

/// The one place that fixes tile order: grid position, alt+N focus keys, and
/// scope identity all derive from this table.
pub const TILES: [Tile; 6] = [
    Tile {
        id: themes::ID,
        has_controls: true,
        render: themes::render,
    },
    Tile {
        id: release::ID,
        has_controls: true,
        render: release::render,
    },
    Tile {
        id: button_variants::ID,
        has_controls: true,
        render: button_variants::render,
    },
    Tile {
        id: notifications::ID,
        has_controls: true,
        render: notifications::render,
    },
    Tile {
        id: contributions::ID,
        has_controls: false,
        render: contributions::render,
    },
    Tile {
        id: tooltip::ID,
        has_controls: true,
        render: tooltip::render,
    },
];

pub fn render(index: usize, ctx: &mut RenderCtx<'_, '_, AppState, AppMsg>, area: Rect) {
    let tile = &TILES[index];
    let options = if tile.has_controls {
        ScopeOptions::default().tab_wrap(TabWrap::Wrap)
    } else {
        ScopeOptions::default().focusable()
    };
    ctx.scope(tile.id, area, options, tile.render);
}
