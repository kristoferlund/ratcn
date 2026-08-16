//! One module per tab. Each tab owns its local state, messages, update logic,
//! and view construction; the app shell only wires them together.

use ratatui::{
    layout::Rect,
    style::Style,
    widgets::{Block, Padding},
};
use ratcn::runtime::RenderCtx;

pub const PANEL_PADDING: Padding = Padding::new(2, 2, 1, 1);

/// The container chrome every view shares: a bordered block whose border picks
/// up focus and hover.
///
/// Keeping it here rather than at each call site is what makes the panes agree
/// on what "focused" looks like — the accent choice is an app-level convention,
/// so the app owns it.
pub fn render_panel<S, M>(
    ctx: &mut RenderCtx<'_, '_, S, M>,
    area: Rect,
    title: Option<&str>,
) -> Rect {
    // Borders and padding fix the inner rect; the border color does not, so
    // the block is rebuilt where focus and hover are known.
    let inner = Block::bordered().padding(PANEL_PADDING).inner(area);
    let title = title.map(ToOwned::to_owned);
    ctx.paint(move |ctx| {
        let theme = ctx.theme;
        let border = if ctx.contains_focus {
            theme.ring
        } else if ctx.contains_hover {
            theme.primary
        } else {
            theme.border
        };
        let mut block = Block::bordered()
            .border_style(Style::default().fg(border))
            .style(Style::default().fg(theme.foreground).bg(theme.background))
            .padding(PANEL_PADDING);
        if let Some(title) = title {
            block = block.title(title.to_owned());
        }
        debug_assert_eq!(
            block.inner(area),
            inner,
            "the painted block's inner rect must match the one the layout used"
        );
        ctx.render_widget(block, area);
    });
    inner
}

pub mod ledger;
pub mod report;
pub mod settings;
