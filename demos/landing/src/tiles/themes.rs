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

pub struct State {
    focused: Option<&'static str>,
    selected: &'static str,
}

impl Default for State {
    fn default() -> Self {
        let opening = ordered(demo_shared::theme())[0].name;
        Self {
            focused: Some(opening),
            selected: opening,
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

    /// The theme the picker is showing, as of this frame.
    pub fn theme(&self) -> Theme {
        selected_theme(self.selected, demo_shared::theme())
    }
}

/// Which theme the row named `selected` stands for, given what the terminal
/// currently resolves to.
///
/// The resolved row is read live, so a terminal that re-themes carries the app
/// with it — but only while that row is the selection. A preset the user picked
/// by hand is their choice, and a flip of the terminal does not take it back;
/// it changes what the resolved row would give them if they went back to it.
fn selected_theme(selected: &str, resolved: Theme) -> Theme {
    if selected == resolved.name {
        return resolved;
    }
    Theme::presets()
        .iter()
        .find(|preset| preset.name == selected)
        .copied()
        .unwrap_or(resolved)
}

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let controls_disabled = ctx.state().controls_disabled;
    // Built per frame, never cached: on a terminal that reports its own theme
    // changes the resolved entry is a different color after every flip, and a
    // list settled at startup would keep showing the colors it was born with.
    // Its *name* does not move — a solved theme is always `Adaptive` — so a
    // selection made against an earlier list still matches.
    let themes = ordered(demo_shared::theme());
    let picker = List::new(
        themes
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
        Constraint::Length(themes.len() as u16),
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
            "Seven preset themes, custom themes, and one solved from your \
             terminal's colors when it answers.",
        )
        .style(Style::default().fg(theme.muted_foreground))
        .wrap(Wrap { trim: true }),
        intro_area,
    );
    ctx.component("themes", picker, list_area);
}

#[cfg(test)]
mod tests {
    use super::{State, ordered, selected_theme};
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
    /// is the case the tests cannot reach through [`demo_shared::theme`],
    /// because without a terminal to ask the resolved theme *is* a preset.
    fn solved() -> Theme {
        Theme::adaptive(Color::Rgb(253, 246, 227), Color::Rgb(101, 123, 131), None)
    }

    /// The same terminal after the user flips it the other way.
    fn solved_dark() -> Theme {
        Theme::adaptive(Color::Rgb(26, 27, 38), Color::Rgb(192, 202, 245), None)
    }

    #[test]
    fn the_resolved_row_follows_the_terminal_wherever_it_goes() {
        // Its name never moves — a solved theme is always `Adaptive` — so the
        // selection made against the light one still names the dark one.
        let light = solved();
        let dark = solved_dark();
        assert_eq!(light.name, dark.name);

        assert_eq!(selected_theme(light.name, light), light);
        assert_eq!(
            selected_theme(light.name, dark),
            dark,
            "the row is read live: the app wears whatever the terminal is now"
        );
    }

    #[test]
    fn a_preset_the_user_picked_is_not_taken_back_by_the_terminal() {
        let nord = Theme::nord();

        assert_eq!(
            selected_theme(nord.name, solved()),
            nord,
            "their choice stands while the terminal is light"
        );
        assert_eq!(
            selected_theme(nord.name, solved_dark()),
            nord,
            "and still stands after it flips: the flip changed what `Adaptive` \
             offers, not what they chose"
        );
    }

    #[test]
    fn the_resolved_row_outranks_a_preset_that_shares_its_name() {
        // The row is the terminal's, whatever it is called. Today a solved
        // theme is always `Adaptive` and no preset can collide with it, so
        // without this the lookup would happen to give the same answer by
        // falling through to the resolved theme anyway — and would stop doing
        // so the moment a resolved theme took a preset's name.
        let mut impostor = solved();
        impostor.name = Theme::nord().name;

        assert_eq!(
            selected_theme(impostor.name, impostor),
            impostor,
            "the selection named the resolved row, so it gets the resolved row"
        );
        assert_ne!(selected_theme(impostor.name, impostor), Theme::nord());
    }

    #[test]
    fn a_selection_that_names_nothing_falls_back_to_the_resolved_theme() {
        // Unreachable through the picker, which only ever selects a row it
        // listed — but the answer has to be a theme either way.
        assert_eq!(selected_theme("no such theme", solved()), solved());
    }

    #[test]
    fn going_back_to_the_resolved_row_returns_the_terminals_own_theme() {
        // Driven through `selected_theme` with a resolved theme no preset can
        // be: in-process the resolved theme *is* a preset, so a round trip
        // through `State` would prove nothing about the branch it takes.
        let resolved = solved();

        assert_eq!(selected_theme(Theme::nord().name, resolved), Theme::nord());
        assert_eq!(selected_theme(resolved.name, resolved), resolved);
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
        assert_eq!(
            State::default().selected,
            ordered(demo_shared::theme())[0].name
        );
    }
}
