//! Theme picker tile and its focused/selected rows.

use ratatui::{
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    widgets::{Paragraph, Wrap},
};
use ratcn::{List, ListItem, Theme, runtime::DeclareCtx};

use crate::{AppMsg, AppState};

use super::shared::declare_tile_panel;

pub const ID: &str = "themes";
const THEMES: &[Theme] = Theme::presets();

pub struct State {
    focused: Option<&'static str>,
    selected: &'static str,
}

impl Default for State {
    fn default() -> Self {
        Self {
            focused: Some(THEMES[0].name),
            selected: THEMES[0].name,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Msg {
    FocusChanged(&'static str),
    Selected(&'static str),
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::FocusChanged(focused) => self.focused = Some(focused),
            Msg::Selected(selected) => {
                self.selected = selected;
                self.focused = Some(selected);
            }
        }
    }

    pub fn theme(&self) -> Theme {
        THEMES
            .iter()
            .find(|theme| theme.name == self.selected)
            .copied()
            .expect("selected theme is always declared")
    }
}

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let controls_disabled = ctx.state().controls_disabled;
    let themes = List::new(
        THEMES
            .iter()
            .map(|theme| ListItem::new(theme.name, theme.name)),
    )
    .item_focus(
        |state: &AppState| state.themes_state.focused,
        |focused, _| AppMsg::Themes(Msg::FocusChanged(focused)),
    )
    .selection(
        |state: &AppState| Some(state.themes_state.selected),
        |selected| AppMsg::Themes(Msg::Selected(selected)),
    )
    .disabled(controls_disabled);

    let inner = declare_tile_panel(ctx, area, " alt+1 ");
    let [header_area, intro_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(THEMES.len() as u16),
    ])
    .spacing(1)
    .areas(inner);
    let theme = ctx.theme;
    ctx.paint_widget(
        Paragraph::new("Themes").style(
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        header_area,
    );
    ctx.paint_widget(
        Paragraph::new("Ratcn includes seven preset themes and supports custom themes.")
            .style(Style::default().fg(theme.muted_foreground))
            .wrap(Wrap { trim: true }),
        intro_area,
    );
    ctx.component("themes", themes, list_area);
}
