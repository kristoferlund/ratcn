//! Step 2: pick a host. A Select whose choice rewrites the install command.
//!
//! The open flag and the option cursor are local to this step. The chosen
//! backend is shared state, applied by the app shell and read by the summary.

use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
    widgets::{Paragraph, Wrap},
};
use ratcn::{ListItem, Select, runtime::RenderCtx};

use crate::app::{AppState, Msg as AppMsg};
use crate::shared::{Backend, ChoiceMsg};
use crate::steps;

pub const SCREEN_ID: &str = "step_backend";
pub const SELECT_ID: &str = "backend_select";

#[derive(Debug)]
pub struct State {
    pub cursor: Option<Backend>,
    pub open: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            cursor: Some(Backend::default()),
            open: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Msg {
    Focused(Backend),
    OpenChanged(bool),
    /// The panel closes on its own choice; the value itself is shared state.
    Committed(Backend),
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focused(backend) => self.cursor = Some(backend),
            Msg::OpenChanged(open) => self.open = open,
            Msg::Committed(backend) => {
                self.cursor = Some(backend);
                self.open = false;
            }
        }
    }
}

pub fn render(ctx: &mut RenderCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let state = ctx.state();
    let theme = ctx.theme;

    let backend = Select::new(Backend::ALL.map(|backend| ListItem::new(backend, backend.label())))
        .open(
            |s: &AppState| s.backend.open,
            |open| AppMsg::Backend(Msg::OpenChanged(open)),
        )
        .item_focus(
            |s: &AppState| s.backend.cursor,
            |backend| AppMsg::Backend(Msg::Focused(backend)),
        )
        .selection(
            |s: &AppState| Some(s.choices.backend),
            |backend| AppMsg::Choose(ChoiceMsg::SetBackend(backend)),
        );

    let commands = state
        .choices
        .dependency_commands()
        .iter()
        .map(|command| steps::command(theme, *command))
        .collect::<Vec<_>>();
    let inner = steps::render_panel(ctx, area, Some("Pick a backend"));

    let [intro, select_area, commands_area] = inner.layout(
        &Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .spacing(1),
    );

    ctx.paint(move |ctx| {
        ctx.render_widget(
            Paragraph::new("One feature per host, none on by default.")
                .style(Style::default().fg(ctx.theme.muted_foreground))
                .wrap(Wrap { trim: true }),
            intro,
        );
    });
    ctx.render_component(SELECT_ID, backend, select_area);
    ctx.paint(move |ctx| {
        ctx.render_widget(
            Paragraph::new(commands).wrap(Wrap { trim: false }),
            commands_area,
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Choosing an option is what closes the panel — the shared value is applied
    /// elsewhere, so this step would otherwise stay open over the next screen.
    #[test]
    fn committing_a_choice_closes_the_panel() {
        let mut state = State::default();
        state.update(Msg::OpenChanged(true));

        state.update(Msg::Committed(Backend::Browser));

        assert!(!state.open);
        assert_eq!(state.cursor, Some(Backend::Browser));
    }
}
