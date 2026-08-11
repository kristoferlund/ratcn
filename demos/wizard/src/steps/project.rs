//! Step 1: create the crate. Nothing to choose, so nothing to own.

use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
    widgets::{Paragraph, Wrap},
};
use ratcn::runtime::RenderCtx;

use crate::app::{AppState, Msg as AppMsg};
use crate::steps;

pub const SCREEN_ID: &str = "step_project";

pub fn render(ctx: &mut RenderCtx<'_, '_, AppState, AppMsg>) {
    let area = ctx.area();
    let theme = ctx.theme;
    let inner = steps::render_panel(ctx, area, theme, Some("Create a project"));

    let [intro, commands] =
        inner.layout(&Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).spacing(1));

    ctx.render_widget(
        Paragraph::new("A plain binary crate is all ratcn needs.")
            .style(Style::default().fg(theme.muted_foreground))
            .wrap(Wrap { trim: true }),
        intro,
    );
    ctx.render_widget(
        Paragraph::new(vec![
            steps::command(theme, "cargo new my-app"),
            steps::command(theme, "cargo add ratatui"),
        ]),
        commands,
    );
}
