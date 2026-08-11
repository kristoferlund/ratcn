//! One module per step. Each step owns its local state, messages, update logic,
//! and view construction; the app shell only wires them together.

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Padding},
};
use ratcn::{Theme, runtime::RenderCtx};

pub const PANEL_PADDING: Padding = Padding::new(2, 2, 1, 1);

/// The container chrome every step shares: a bordered block whose border picks
/// up focus and hover.
///
/// Keeping it here rather than at each call site is what makes the steps agree
/// on what "focused" looks like — the accent choice is an app-level convention,
/// so the app owns it.
pub fn render_panel<S, M>(
    ctx: &mut RenderCtx<'_, '_, S, M>,
    area: Rect,
    theme: &Theme,
    title: Option<&str>,
) -> Rect {
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
        block = block.title(format!(" {title} "));
    }
    let inner = block.inner(area);
    ctx.render_widget(block, area);
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
