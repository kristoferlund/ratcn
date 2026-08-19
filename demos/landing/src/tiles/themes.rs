//! Theme picker tile and its focused/selected rows.

use std::sync::OnceLock;

use ratatui::{
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    widgets::{Paragraph, Wrap},
};
use ratcn::{List, ListItem, Theme, runtime::DeclareCtx};

use crate::{AppMsg, AppState};

use super::shared::declare_tile_panel;

pub const ID: &str = "themes";

/// `resolved` first, then every preset it does not already stand for.
///
/// A name is the picker's identity — it is what a row is keyed and matched by —
/// so a preset the resolved theme duplicates by name is dropped rather than
/// listed twice. That happens exactly when the terminal could not be asked and
/// the resolved theme *is* a preset; when it was asked, the solved theme is
/// named `Adaptive`, nothing is dropped, and the list is one longer.
fn ordered(resolved: Theme) -> Vec<Theme> {
    std::iter::once(resolved)
        .chain(
            Theme::presets()
                .iter()
                .filter(|preset| preset.name != resolved.name)
                .copied(),
        )
        .collect()
}

/// The picker's themes, settled once for the process.
fn themes() -> &'static [Theme] {
    static THEMES: OnceLock<Vec<Theme>> = OnceLock::new();
    THEMES.get_or_init(|| ordered(*demo_shared::theme()))
}

pub struct State {
    focused: Option<&'static str>,
    selected: &'static str,
}

impl Default for State {
    fn default() -> Self {
        Self {
            focused: Some(themes()[0].name),
            selected: themes()[0].name,
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
        themes()
            .iter()
            .find(|theme| theme.name == self.selected)
            .copied()
            .expect("selected theme is always declared")
    }
}

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let controls_disabled = ctx.state().controls_disabled;
    let picker = List::new(
        themes()
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
        Constraint::Length(themes().len() as u16),
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
        Paragraph::new(
            "Ratcn includes seven preset themes, supports custom themes, and solves \
             one from your terminal's colors.",
        )
        .style(Style::default().fg(theme.muted_foreground))
        .wrap(Wrap { trim: true }),
        intro_area,
    );
    ctx.component("themes", picker, list_area);
}

#[cfg(test)]
mod tests {
    use super::{State, ordered, themes};
    use ratatui::style::Color;
    use ratcn::Theme;

    fn names(themes: &[Theme]) -> Vec<&'static str> {
        themes.iter().map(|theme| theme.name).collect()
    }

    fn presets() -> Vec<&'static str> {
        names(Theme::presets())
    }

    /// A theme no preset can be: solved from a terminal's own colors, so it is
    /// named `Adaptive` and shares its name with nothing in `presets()`. This
    /// is the case the tests cannot reach through [`themes`], because without a
    /// terminal to ask the resolved theme *is* a preset.
    fn solved() -> Theme {
        Theme::adaptive(Color::Rgb(253, 246, 227), Color::Rgb(101, 123, 131), None)
    }

    #[test]
    fn a_solved_theme_leads_the_list_and_costs_no_preset() {
        let listed = ordered(solved());

        assert_eq!(
            listed.first().map(|theme| theme.name),
            Some("Adaptive"),
            "the theme this terminal resolved to is the one the picker opens on"
        );
        assert_eq!(
            names(&listed[1..]),
            presets(),
            "every preset still follows it, in the order `presets()` fixed"
        );
    }

    #[test]
    fn a_resolved_theme_that_is_already_a_preset_replaces_it_rather_than_joining_it() {
        // The fallback case: the terminal could not be asked, so the resolved
        // theme is one of the presets and must not appear twice.
        let duplicated = Theme::presets()[2];
        let listed = ordered(duplicated);
        let listed = names(&listed);

        assert_eq!(listed.first(), Some(&duplicated.name), "it still leads");
        assert_eq!(
            listed.len(),
            Theme::presets().len(),
            "it replaced its own entry rather than lengthening the list: {listed:?}"
        );
        let mut unique = listed.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            listed.len(),
            unique.len(),
            "a name is what a row is keyed by, so it can only appear once: {listed:?}"
        );
        for preset in presets() {
            assert!(listed.contains(&preset), "{preset} is still listed");
        }
    }

    #[test]
    fn the_picker_opens_on_the_theme_at_the_head_of_its_own_list() {
        // A wiring pin only: with no terminal to ask, the resolved theme is
        // itself a preset, so this cannot tell `ordered` apart from `presets()`
        // by construction. What `ordered` actually does is pinned above.
        assert_eq!(State::default().selected, themes()[0].name);
        assert_eq!(themes(), ordered(*demo_shared::theme()));
    }
}
