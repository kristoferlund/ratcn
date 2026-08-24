//! A Quake III asset checklist with a continuously moving download indicator.

use ratatui::{
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    widgets::{Paragraph, Wrap},
};
use ratcn::{Checkbox, ProgressWidget, runtime::DeclareCtx};

use crate::{AppMsg, AppState};

use super::shared::declare_tile_panel;

pub const ID: &str = "quake_download";

pub struct State {
    tournament_skins: bool,
    powerup_icons: bool,
    quad_glow: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            tournament_skins: true,
            powerup_icons: true,
            quad_glow: false,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Msg {
    TournamentSkins(bool),
    PowerupIcons(bool),
    QuadGlow(bool),
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::TournamentSkins(checked) => self.tournament_skins = checked,
            Msg::PowerupIcons(checked) => self.powerup_icons = checked,
            Msg::QuadGlow(checked) => self.quad_glow = checked,
        }
    }
}

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let disabled = ctx.state().controls_disabled;
    let tournament_skins = Checkbox::new("Tournament skins")
        .checked(
            |state: &AppState| state.quake_state.tournament_skins,
            |checked| AppMsg::Quake(Msg::TournamentSkins(checked)),
        )
        .disabled(disabled);
    let powerup_icons = Checkbox::new("Powerup icons")
        .checked(
            |state: &AppState| state.quake_state.powerup_icons,
            |checked| AppMsg::Quake(Msg::PowerupIcons(checked)),
        )
        .disabled(disabled);
    let quad_glow = Checkbox::new("Quad damage glow")
        .checked(
            |state: &AppState| state.quake_state.quad_glow,
            |checked| AppMsg::Quake(Msg::QuadGlow(checked)),
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
    let [skins_area, powerups_area, quad_glow_area] =
        Layout::vertical([Constraint::Length(1); 3]).areas(checks_area);
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
    ctx.component("skins", tournament_skins, skins_area);
    ctx.component("powerups", powerup_icons, powerups_area);
    ctx.component("quad_glow", quad_glow, quad_glow_area);
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

    use super::download_ratio;

    #[test]
    fn the_download_pulse_reverses_without_dropping_back_to_zero() {
        assert_eq!(download_ratio(Duration::ZERO), 0.2);
        assert_eq!(download_ratio(Duration::from_secs(5)), 0.8);
        assert_eq!(download_ratio(Duration::from_secs(10)), 0.2);
    }
}
