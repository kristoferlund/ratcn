//! One module per step. Each step owns its local state, messages, update logic,
//! and view construction; the app shell only wires them together.

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding},
};
use ratcn::{Theme, runtime::DeclareCtx};

pub const PANEL_PADDING: Padding = Padding::new(2, 2, 1, 1);

/// The container chrome every step shares: a bordered block whose border picks
/// up focus and hover.
///
/// Keeping it here rather than at each call site is what makes the steps agree
/// on what "focused" looks like — the accent choice is an app-level convention,
/// so the app owns it.
pub fn declare_panel<S, M>(
    ctx: &mut DeclareCtx<'_, S, M>,
    area: Rect,
    title: Option<&str>,
) -> Rect {
    // Borders and padding fix the inner rect; the border color does not, so
    // the block is rebuilt where focus and hover are known.
    let inner = Block::bordered().padding(PANEL_PADDING).inner(area);
    let title = title.map(ToOwned::to_owned);
    ctx.paint(move |ctx| {
        let theme = ctx.theme;
        let border = if ctx.contains_focus() {
            theme.ring
        } else if ctx.contains_hover() {
            theme.primary
        } else {
            theme.border
        };
        let mut block = Block::bordered()
            .border_style(Style::default().fg(border))
            .style(Style::default().fg(theme.foreground).bg(theme.background))
            .padding(PANEL_PADDING);
        if let Some(title) = title {
            block = block.title(format!(" {title} "));
        }
        debug_assert_eq!(
            block.inner(area),
            inner,
            "the painted block's inner rect must match the one the layout used"
        );
        ctx.widget(block, area);
    });
    inner
}

/// One shell command, prompt included, so it reads as something to copy.
pub fn command(theme: &Theme, text: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled("$ ", Style::default().fg(theme.muted_foreground)),
        Span::styled(text.into(), Style::default().fg(theme.accent)),
    ])
}

/// One line of Rust, for a step whose choice lands in code rather than a shell.
pub fn code(theme: &Theme, text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(text.into(), Style::default().fg(theme.accent)))
}

pub mod backend;
pub mod done;
pub mod project;
pub mod theme;
