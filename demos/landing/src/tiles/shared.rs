use ratatui::{
    layout::{HorizontalAlignment::Left, Rect},
    style::Style,
    widgets::{Block, Padding},
};
use ratcn::runtime::DeclareCtx;

use crate::{AppMsg, AppState};

pub fn declare_tile_panel(
    ctx: &mut DeclareCtx<'_, AppState, AppMsg>,
    area: Rect,
    title: &'static str,
) -> Rect {
    declare_tile_panel_with_padding(ctx, area, title, Padding::uniform(2))
}

pub fn declare_tile_panel_with_padding(
    ctx: &mut DeclareCtx<'_, AppState, AppMsg>,
    area: Rect,
    title: &'static str,
    padding: Padding,
) -> Rect {
    // The inner rect is a function of the borders and the padding alone, so
    // it is available now, while the colors the block paints with are not:
    // they follow focus and hover, which only settle after the whole tree is
    // declared.
    let inner = Block::bordered().padding(padding).inner(area);
    ctx.paint(move |ctx| {
        // Every tile shares this chrome, so the accent choice lives here
        // rather than at each call site — that is what keeps the tiles
        // agreeing on what "focused" looks like.
        let border = if ctx.contains_focus {
            ctx.theme.ring
        } else if ctx.contains_hover {
            ctx.theme.primary
        } else {
            ctx.theme.border
        };
        let block = Block::bordered()
            .border_style(Style::default().fg(border))
            .style(
                Style::default()
                    .fg(ctx.theme.foreground)
                    .bg(ctx.theme.background),
            )
            .title(title)
            .title_alignment(Left)
            .padding(padding);
        debug_assert_eq!(
            block.inner(area),
            inner,
            "the painted block's inner rect must match the one the layout used"
        );
        ctx.widget(block, area);
    });
    inner
}
