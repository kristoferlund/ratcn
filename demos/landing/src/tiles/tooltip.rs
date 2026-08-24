//! Agent interaction settings, with a tooltip that summarizes the selection.

use ratatui::{
    layout::{Alignment, Constraint, Flex, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Wrap},
};
use ratcn::{Button, Cycle, Toast, Tooltip, TooltipSide, runtime::DeclareCtx};

use crate::{AppMsg, AppState};

use super::shared::declare_tile_panel;

pub const ID: &str = "tooltip";

const CHANGE_MODES: [&str; 2] = ["Review first", "Apply directly"];
const RETRY_POLICIES: [&str; 3] = ["One retry", "Back off", "Persistent"];
const UPDATE_CADENCES: [&str; 3] = ["Milestones", "Each action", "On request"];

const TIP: &str = "tip";
const TRIGGER: &str = "hover";

pub struct State {
    change_mode: usize,
    retry_policy: usize,
    update_cadence: usize,
}

#[derive(Clone, Copy)]
pub enum Msg {
    ChangeMode(usize),
    RetryPolicy(usize),
    UpdateCadence(usize),
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::ChangeMode(index) => self.change_mode = index,
            Msg::RetryPolicy(index) => self.retry_policy = index,
            Msg::UpdateCadence(index) => self.update_cadence = index,
        }
    }

    fn summary(&self) -> String {
        format!(
            "{} | {} | {}",
            CHANGE_MODES[self.change_mode],
            RETRY_POLICIES[self.retry_policy],
            UPDATE_CADENCES[self.update_cadence],
        )
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            change_mode: 0,
            retry_policy: 0,
            update_cadence: 0,
        }
    }
}

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let inner = declare_tile_panel(ctx, area, " alt+5 ");
    let disabled = ctx.state().controls_disabled;
    let summary = ctx.state().agent_settings_state.summary();
    let tooltip = Tooltip::new(summary)
        .side(TooltipSide::Top)
        .open_when(move |state: &AppState, hovered| hovered && !state.controls_disabled)
        .trigger(move |ctx| {
            let area = ctx.area();
            ctx.component(TRIGGER, button(disabled), area);
        });

    let [
        title_area,
        _gap_one,
        intro_area,
        _gap_two,
        settings_area,
        _gap_three,
        button_row,
        _rest,
    ] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);
    let [change_mode_area, retry_policy_area, update_cadence_area] =
        Layout::vertical([Constraint::Length(1); 3])
            .spacing(1)
            .areas(settings_area);
    let [button_area] = Layout::horizontal([Constraint::Length(button(false).width())])
        .flex(Flex::Center)
        .areas(button_row);
    let theme = ctx.theme;

    ctx.paint_widget(
        Paragraph::new("Agent settings").style(
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        title_area,
    );
    ctx.paint_widget(
        Paragraph::new("Determines how your agent interacts with the server.")
            .style(Style::default().fg(theme.muted_foreground))
            .wrap(Wrap { trim: true }),
        intro_area,
    );

    setting(
        ctx,
        "Change mode",
        CHANGE_MODES,
        |state| state.agent_settings_state.change_mode,
        |index| AppMsg::AgentSettings(Msg::ChangeMode(index)),
        change_mode_area,
        disabled,
    );
    setting(
        ctx,
        "Retry policy",
        RETRY_POLICIES,
        |state| state.agent_settings_state.retry_policy,
        |index| AppMsg::AgentSettings(Msg::RetryPolicy(index)),
        retry_policy_area,
        disabled,
    );
    setting(
        ctx,
        "Updates",
        UPDATE_CADENCES,
        |state| state.agent_settings_state.update_cadence,
        |index| AppMsg::AgentSettings(Msg::UpdateCadence(index)),
        update_cadence_area,
        disabled,
    );
    ctx.component(TIP, tooltip, button_area);
}

fn setting(
    ctx: &mut DeclareCtx<'_, AppState, AppMsg>,
    label: &'static str,
    options: impl IntoIterator<Item = &'static str>,
    selected: impl Fn(&AppState) -> usize + 'static,
    on_change: impl Fn(usize) -> AppMsg + 'static,
    area: ratatui::layout::Rect,
    disabled: bool,
) {
    ctx.paint_widget(
        Line::from(label).style(Style::default().fg(ctx.theme.foreground)),
        area,
    );
    ctx.component(
        label,
        Cycle::new(options)
            .selection(selected, on_change)
            .align(Alignment::Right)
            .disabled(disabled),
        area,
    );
}

fn button(disabled: bool) -> Button<AppMsg> {
    Button::new("Hover me")
        .on_press(|| AppMsg::Toast(Toast::info("Agent settings applied")))
        .disabled(disabled)
}
