//! Step 3: pick a palette. The choice re-colors the wizard as it is made.
//!
//! The row cursor is local to this step. The selected theme is shared state —
//! the app shell renders every frame with it, so the whole screen follows.

use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
    widgets::{Paragraph, Wrap},
};
use ratcn::{List, ListItem, Theme, runtime::RenderCtx};

use crate::app::{AppState, Msg as AppMsg};
use crate::shared::ChoiceMsg;
use crate::steps;

pub const SCREEN_ID: &str = "step_theme";
pub const LIST_ID: &str = "theme_list";

#[derive(Debug)]
pub struct State {
    pub cursor: Option<&'static str>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            cursor: Some(Theme::default_dark().name),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Msg {
    Focused(&'static str),
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focused(theme) => self.cursor = Some(theme),
        }
    }
}

pub fn render(ctx: &mut RenderCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let state = ctx.state();
    let theme = ctx.theme;

    let presets = List::new(
        Theme::presets()
            .iter()
            .map(|preset| ListItem::new(preset.name, preset.name)),
    )
    .item_focus(
        |s: &AppState| s.theme.cursor,
        |preset, _| AppMsg::Theme(Msg::Focused(preset)),
    )
    .selection(
        |s: &AppState| Some(s.choices.theme),
        |preset| AppMsg::Choose(ChoiceMsg::SetTheme(preset)),
    );

    let line = steps::code(theme, state.choices.theme_line());
    let inner = steps::render_panel(ctx, area, Some("Pick a theme"));

    let [intro, list_area, code_area] = inner.layout(
        &Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(Theme::presets().len() as u16),
            Constraint::Min(0),
        ])
        .spacing(1),
    );

    ctx.paint(move |ctx| {
        ctx.render_widget(
            Paragraph::new("Seven presets ship with ratcn.")
                .style(Style::default().fg(ctx.theme.muted_foreground))
                .wrap(Wrap { trim: true }),
            intro,
        );
    });
    ctx.render_component(LIST_ID, presets, list_area);
    ctx.paint(move |ctx| ctx.render_widget(Paragraph::new(line), code_area));
}
