//! Tab navigation state and the tab row that edits it.
//!
//! This is orchestration state — it belongs to the app shell, not to any view.

use ratcn::{Tab, Tabs};

use crate::app::{AppState, Msg};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Ledger,
    Report,
    Settings,
}

/// The selected tab is open; the focused tab is only the keyboard highlight.
#[derive(Debug, Default)]
pub struct Nav {
    pub selected: Screen,
    pub focused: Screen,
}

#[derive(Debug, Clone, Copy)]
pub enum NavMsg {
    Focused(Screen),
    Selected(Screen),
}

impl Nav {
    pub fn update(&mut self, msg: NavMsg) {
        match msg {
            NavMsg::Focused(screen) => self.focused = screen,
            NavMsg::Selected(screen) => {
                self.focused = screen;
                self.selected = screen;
            }
        }
    }
}

pub fn tabs() -> Tabs<Screen, AppState, Msg> {
    Tabs::new([
        Tab::new(Screen::Ledger, "Ledger"),
        Tab::new(Screen::Report, "Report"),
        Tab::new(Screen::Settings, "Settings"),
    ])
    .item_focus(
        |s: &AppState| Some(s.nav.focused),
        |screen| Msg::Nav(NavMsg::Focused(screen)),
    )
    .selection(
        |s: &AppState| Some(s.nav.selected),
        |screen| Msg::Nav(NavMsg::Selected(screen)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_selection_message_aligns_selection_and_item_focus() {
        let mut nav = Nav::default();

        nav.update(NavMsg::Selected(Screen::Settings));

        assert_eq!(nav.focused, Screen::Settings);
        assert_eq!(nav.selected, Screen::Settings);
    }
}
