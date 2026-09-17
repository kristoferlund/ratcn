//! A Quake III asset checklist with a continuously moving download indicator.

use ratatui::{
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    widgets::{Paragraph, Wrap},
};
use ratcn::{List, ListItem, ProgressWidget, runtime::DeclareCtx};

use crate::{AppMsg, AppState};

use super::shared::declare_tile_panel;

const ASSET_OPTIONS: [&str; 3] = ["Tournament skins", "Powerup icons", "Quad damage glow"];

pub const ID: &str = "quake_download";

pub struct State {
    focused: Option<&'static str>,
    selected: Vec<&'static str>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            focused: Some(ASSET_OPTIONS[0]),
            selected: vec![ASSET_OPTIONS[0], ASSET_OPTIONS[1]],
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
    let disabled = ctx.state().controls_disabled;
    let assets = List::new(ASSET_OPTIONS.map(|label| ListItem::new(label, label)))
        .item_focus(
            |state: &AppState| state.quake_state.focused,
            |focused, _| AppMsg::Quake(Msg::FocusChanged(focused)),
        )
        .multi_selection(
            |state: &AppState, value| state.quake_state.selected.contains(value),
            |value| AppMsg::Quake(Msg::Toggled(value)),
        )
        .disabled(disabled);

    let inner = declare_tile_panel(ctx, area, " alt+8 ");
    let [
        title_area,
        _gap_one,
        intro_area,
        _gap_two,
        checks_area,
        _gap_three,
        progress_area,
        _rest,
    ] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Fill(1),
    ])
    .areas(inner);
    let theme = ctx.theme;

    ctx.paint_widget(
        Paragraph::new("Quake III setup").style(
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        title_area,
    );
    ctx.paint_widget(
        Paragraph::new("Prepare a local arena build before the next match.")
            .style(Style::default().fg(theme.muted_foreground))
            .wrap(Wrap { trim: true }),
        intro_area,
    );
    ctx.component("assets", assets, checks_area);
    ctx.paint_widget(
        ProgressWidget::new(download_ratio(demo_shared::monotonic_time()))
            .label("Downloading quake_x86.zip")
            .show_value(true)
            .themed(theme),
        progress_area,
    );
}

/// Sweep between 20% and 80% so the bar remains visibly in flight without a
/// distracting reset back to zero.
fn download_ratio(now: std::time::Duration) -> f64 {
    const HALF_CYCLE_MILLIS: u128 = 5_000;
    let position = now.as_millis() % (HALF_CYCLE_MILLIS * 2);
    let distance = if position <= HALF_CYCLE_MILLIS {
        position
    } else {
        HALF_CYCLE_MILLIS * 2 - position
    };
    0.2 + 0.6 * distance as f64 / HALF_CYCLE_MILLIS as f64
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{ASSET_OPTIONS, Msg, State, download_ratio};

    #[test]
    fn asset_navigation_preserves_selection_and_toggles_only_the_chosen_asset() {
        let mut state = State::default();
        assert_eq!(state.focused, Some(ASSET_OPTIONS[0]));
        assert_eq!(state.selected, ASSET_OPTIONS[..2]);

        state.update(Msg::FocusChanged(ASSET_OPTIONS[2]));
        assert_eq!(state.focused, Some(ASSET_OPTIONS[2]));
        assert_eq!(state.selected, ASSET_OPTIONS[..2]);
        state.update(Msg::Toggled(ASSET_OPTIONS[2]));
        assert_eq!(state.selected, ASSET_OPTIONS);
        state.update(Msg::Toggled(ASSET_OPTIONS[1]));
        assert_eq!(state.selected, [ASSET_OPTIONS[0], ASSET_OPTIONS[2]]);
        assert_eq!(state.focused, Some(ASSET_OPTIONS[1]));
    }

    #[test]
    fn the_download_pulse_reverses_without_dropping_back_to_zero() {
        assert_eq!(download_ratio(Duration::ZERO), 0.2);
        assert_eq!(download_ratio(Duration::from_secs(5)), 0.8);
        assert_eq!(download_ratio(Duration::from_secs(10)), 0.2);
    }
}
