//! Step 4: the end. Nothing to choose — just what the choices added up to.

use ratatui::widgets::{Paragraph, Wrap};
use ratcn::runtime::RenderCtx;

use crate::app::{AppState, Msg as AppMsg};
use crate::steps;

pub const SCREEN_ID: &str = "step_done";

pub fn render(ctx: &mut RenderCtx<'_, '_, AppState, AppMsg>) {
    let area = ctx.area();
    let state = ctx.state();
    let theme = ctx.theme;

    let mut commands = vec![
        steps::command(theme, "cargo new my-app"),
        steps::command(theme, "cd my-app"),
    ];
    commands.extend(
        state
            .choices
            .dependency_commands()
            .iter()
            .map(|command| steps::command(theme, *command)),
    );
    commands.push(steps::code(theme, state.choices.theme_line()));

    let inner = steps::render_panel(ctx, area, Some("Happy development!"));
    ctx.paint(move |ctx| {
        ctx.render_widget(Paragraph::new(commands).wrap(Wrap { trim: false }), inner);
    });
}
