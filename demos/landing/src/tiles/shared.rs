use ratatui::{
    layout::{HorizontalAlignment::Left, Rect},
    style::Style,
    widgets::{Block, Padding},
};
use ratcn::runtime::RenderCtx;

use crate::{AppMsg, AppState};

pub fn render_tile_panel(
    ctx: &mut RenderCtx<'_, '_, AppState, AppMsg>,
    area: Rect,
    title: &'static str,
) -> Rect {
    render_tile_panel_with_padding(ctx, area, title, Padding::uniform(2))
}

pub fn render_tile_panel_with_padding(
    ctx: &mut RenderCtx<'_, '_, AppState, AppMsg>,
    area: Rect,
    title: &'static str,
    padding: Padding,
) -> Rect {
    // Every tile shares this chrome, so the accent choice lives here rather
    // than at each call site — that is what keeps the tiles agreeing on what
    // "focused" looks like.
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
    let inner = block.inner(area);
    ctx.render_widget(block, area);
    inner
}
