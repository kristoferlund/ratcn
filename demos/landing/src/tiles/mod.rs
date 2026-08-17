pub mod button_variants;
pub mod contributions;
pub mod notifications;
pub mod release;
pub mod shared;
pub mod themes;
pub mod tooltip;

use ratatui::layout::Rect;
use ratcn::runtime::{DeclareCtx, ScopeOptions, TabWrap};

use crate::{AppMsg, AppState};

pub struct Tile {
    pub id: &'static str,
    /// A tile with controls is a Tab-trapping scope; a controls-free tile is
    /// a plain focusable leaf.
    pub has_controls: bool,
    pub declare: fn(&mut DeclareCtx<'_, AppState, AppMsg>),
}

/// The one place that fixes tile order: grid position, alt+N focus keys, and
/// scope identity all derive from this table.
pub const TILES: [Tile; 6] = [
    Tile {
        id: themes::ID,
        has_controls: true,
        declare: themes::declare,
    },
    Tile {
        id: release::ID,
        has_controls: true,
        declare: release::declare,
    },
    Tile {
        id: button_variants::ID,
        has_controls: true,
        declare: button_variants::declare,
    },
    Tile {
        id: notifications::ID,
        has_controls: true,
        declare: notifications::declare,
    },
    Tile {
        id: contributions::ID,
        has_controls: false,
        declare: contributions::declare,
    },
    Tile {
        id: tooltip::ID,
        has_controls: true,
        declare: tooltip::declare,
    },
];

pub fn declare(index: usize, ctx: &mut DeclareCtx<'_, AppState, AppMsg>, area: Rect) {
    let tile = &TILES[index];
    let options = if tile.has_controls {
        ScopeOptions::default().tab_wrap(TabWrap::Wrap)
    } else {
        ScopeOptions::default().focusable()
    };
    ctx.scope(tile.id, area, options, tile.declare);
}
