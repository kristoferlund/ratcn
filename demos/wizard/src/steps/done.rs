//! Step 4: the end. Nothing to choose — just what the choices added up to.

use ratatui::{
    layout::{Constraint, Flex, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::Paragraph,
};
use ratcn::runtime::RenderCtx;

use crate::app::{AppState, Msg as AppMsg};
use crate::steps;

pub const SCREEN_ID: &str = "step_done";

pub fn render(ctx: &mut RenderCtx<'_, '_, AppState, AppMsg>) {
    let area = ctx.area();
    let state = ctx.state();
    let theme = ctx.theme;

    let headline = Line::from("Happy development!").centered().style(
        Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    );
    let recap = vec![
        steps::command(theme, state.choices.cargo_add()),
        steps::code(theme, state.choices.theme_line()),
    ];
    // The two lines are different lengths, so centering each one separately
    // would stagger them. Center the block, left-align inside it.
    let recap_width = recap.iter().map(Line::width).max().unwrap_or(0) as u16;

    let inner = steps::render_panel(ctx, area, theme, None);
    let [block] = Layout::vertical([Constraint::Length(4)])
        .flex(Flex::Center)
        .areas(inner);
    let [headline_area, recap_area] =
        block.layout(&Layout::vertical([Constraint::Length(1), Constraint::Length(2)]).spacing(1));
    let [recap_area] = Layout::horizontal([Constraint::Length(recap_width)])
        .flex(Flex::Center)
        .areas(recap_area);

    ctx.render_widget(Paragraph::new(headline), headline_area);
    ctx.render_widget(Paragraph::new(recap), recap_area);
}
