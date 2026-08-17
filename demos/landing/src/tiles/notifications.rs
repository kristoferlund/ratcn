//! Notification preferences tile and its local list state.

use ratatui::{
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    widgets::{Paragraph, Wrap},
};
use ratcn::{List, ListItem, runtime::DeclareCtx};

use crate::{AppMsg, AppState};

use super::shared::declare_tile_panel;

const NOTIFICATION_OPTIONS: [&str; 5] = [
    "Transaction alerts",
    "Security alerts",
    "Goal milestones",
    "Market updates",
    "Spam from third parties",
];

pub const ID: &str = "notifications";

pub struct State {
    focused: Option<&'static str>,
    selected: Vec<&'static str>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            focused: Some(NOTIFICATION_OPTIONS[0]),
            selected: vec![NOTIFICATION_OPTIONS[1], NOTIFICATION_OPTIONS[3]],
        }
    }
}

#[derive(Clone, Copy)]
pub enum Msg {
    FocusChanged(&'static str),
    Toggled(&'static str),
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focused) => self.focused = Some(focused),
            Msg::Toggled(value) => {
                self.focused = Some(value);
                if let Some(position) = self.selected.iter().position(|selected| *selected == value)
                {
                    self.selected.remove(position);
                } else {
                    self.selected.push(value);
                }
            }
        }
    }
}

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let controls_disabled = ctx.state().controls_disabled;
    let notifications = List::new(NOTIFICATION_OPTIONS.map(|label| ListItem::new(label, label)))
        .item_focus(
            |state: &AppState| state.notifications_state.focused,
            |focused, _| AppMsg::Notifications(Msg::FocusChanged(focused)),
        )
        .multi_selection(
            |state: &AppState, value| state.notifications_state.selected.contains(value),
            |value| AppMsg::Notifications(Msg::Toggled(value)),
        )
        .disabled(controls_disabled);

    let inner = declare_tile_panel(ctx, area, " alt+4 ");
    let [header_area, intro_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(NOTIFICATION_OPTIONS.len() as u16),
    ])
    .spacing(1)
    .areas(inner);
    ctx.paint(move |ctx| {
        let theme = ctx.theme;
        ctx.widget(
            Paragraph::new("Notifications").style(
                Style::default()
                    .fg(theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            header_area,
        );
        ctx.widget(
            Paragraph::new("Choose which email and push alerts you want to receive.")
                .style(Style::default().fg(theme.muted_foreground))
                .wrap(Wrap { trim: true }),
            intro_area,
        );
    });
    ctx.component("notifications", notifications, list_area);
}
