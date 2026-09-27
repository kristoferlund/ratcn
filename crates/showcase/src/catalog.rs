//! Catalog demos and the trait that lets the showcase drive one without
//! knowing its type. The landing demo is hosted separately on the main page.

use std::time::Duration;

use ratatui::{buffer::Buffer, layout::Rect};
use ratcn::{Theme, runtime::Event};

/// One demo, in the shape a host can drive through a trait object.
///
/// [`demo_shared::Demo`] cannot become one: the two facts a host needs — the
/// theme a demo paints with, and whether it follows the terminal instead — are
/// associated consts, and a const is not dispatchable. This is the same
/// contract with those turned into a method.
/// The three [`demo_shared::Demo`] methods keep that trait's contracts; only
/// `theme` is new.
pub trait Embedded {
    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme);

    fn handle_event(&mut self, event: Event) -> bool;

    fn wake(&self) -> Option<Duration>;

    /// The theme this demo paints with, given what the terminal resolves to.
    fn theme(&self, terminal: &Theme) -> Theme;
}

impl<D: demo_shared::Demo> Embedded for D {
    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        demo_shared::Demo::draw(self, buffer, area, theme);
    }

    fn handle_event(&mut self, event: Event) -> bool {
        demo_shared::Demo::handle_event(self, event)
    }

    fn wake(&self) -> Option<Duration> {
        demo_shared::Demo::wake(self)
    }

    /// The answer `demo_shared::run` gives a demo on its own: it opens an
    /// adaptive session only for an adaptive demo, so anything else never sees
    /// the terminal's colors and paints with its own preset.
    fn theme(&self, terminal: &Theme) -> Theme {
        if D::ADAPTIVE { *terminal } else { D::THEME }
    }
}

/// One row of the catalog.
pub struct Entry {
    /// The crate name, which is also what `cargo run -p <name>` takes.
    pub name: &'static str,
    /// Builds the demo. The host calls it on first use and not before.
    open: fn() -> Box<dyn Embedded>,
}

impl Entry {
    /// Construct this demo.
    pub fn open(&self) -> Box<dyn Embedded> {
        (self.open)()
    }
}

/// Every demo except the landing preview, alphabetical by crate name, which
/// is also the order the nav list shows them in.
pub static ENTRIES: &[Entry] = &[
    Entry {
        name: "barchart",
        open: || Box::new(barchart::Chart),
    },
    Entry {
        name: "barchart-horizontal",
        open: || Box::new(barchart_horizontal::Chart),
    },
    Entry {
        name: "button-large",
        open: || Box::new(button_large::App::new()),
    },
    Entry {
        name: "button-small",
        open: || Box::new(button_small::App::new()),
    },
    Entry {
        name: "checkbox",
        open: || Box::new(checkbox::App::new()),
    },
    Entry {
        name: "cycle",
        open: || Box::new(cycle::App::new()),
    },
    Entry {
        name: "dialog",
        open: || Box::new(dialog::App::new()),
    },
    Entry {
        name: "drag",
        open: || Box::new(drag::App::new()),
    },
    Entry {
        name: "effects",
        open: || Box::new(effects::App::new()),
    },
    Entry {
        name: "kanban",
        open: || Box::new(kanban::App::new()),
    },
    Entry {
        name: "ledger93",
        open: || Box::new(ledger93::App::new()),
    },
    Entry {
        name: "list",
        open: || Box::new(list::App::new()),
    },
    Entry {
        name: "list-multi",
        open: || Box::new(list_multi::App::new()),
    },
    Entry {
        name: "list-people",
        open: || Box::new(list_people::App::new()),
    },
    Entry {
        name: "panels",
        open: || Box::new(panels::App::new()),
    },
    Entry {
        name: "progress",
        open: || Box::new(progress::Progress),
    },
    Entry {
        name: "scroll-area",
        open: || Box::new(scroll_area::App::new()),
    },
    Entry {
        name: "select",
        open: || Box::new(select::App::new()),
    },
    Entry {
        name: "tabs-automatic",
        open: || Box::new(tabs_automatic::App::new()),
    },
    Entry {
        name: "tabs-basic",
        open: || Box::new(tabs_basic::App::new()),
    },
    Entry {
        name: "tabs-disabled",
        open: || Box::new(tabs_disabled::App::new()),
    },
    Entry {
        name: "tabs-large",
        open: || Box::new(tabs_large::App::new()),
    },
    Entry {
        name: "toast",
        open: || Box::new(toast::App::new()),
    },
    Entry {
        name: "tooltip",
        open: || Box::new(tooltip::App::new()),
    },
    Entry {
        name: "wizard",
        open: || Box::new(wizard::App::new()),
    },
];

/// Cells the longest demo name needs.
pub fn widest_name() -> u16 {
    ENTRIES
        .iter()
        .map(|entry| entry.name.chars().count() as u16)
        .max()
        .expect("the catalog is not empty")
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs, path::Path};

    use super::ENTRIES;

    #[test]
    fn list_demos_keep_the_default_dark_background_in_a_light_terminal() {
        let mut terminal_theme = ratcn::Theme::default_dark();
        terminal_theme.background = ratatui::style::Color::White;
        let background = ratcn::Theme::default_dark().background;
        assert_ne!(terminal_theme.background, background);
        for name in ["list", "list-multi", "list-people"] {
            let mut demo = ENTRIES
                .iter()
                .find(|entry| entry.name == name)
                .unwrap()
                .open();
            let theme = demo.theme(&terminal_theme);
            let area = ratatui::layout::Rect::new(0, 0, 60, 20);
            let mut buffer = ratatui::buffer::Buffer::empty(area);
            demo.draw(&mut buffer, area, &theme);
            assert_eq!(
                buffer[(0, 0)].bg,
                background,
                "{name} inherited the terminal background"
            );
        }
    }

    #[test]
    fn the_catalog_is_sorted_and_lists_each_demo_once() {
        let names: Vec<&str> = ENTRIES.iter().map(|entry| entry.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "the nav list shows the table's order");

        let unique: BTreeSet<&str> = names.iter().copied().collect();
        assert_eq!(unique.len(), names.len(), "a demo is listed twice");
    }

    /// The repository registers a demo by putting a `Trunk.toml` in its
    /// directory and nowhere else — `scripts/build-demos.sh` discovers the docs
    /// build the same way. A new demo missing from the catalog would otherwise
    /// go unnoticed, so the table is checked against the directory rather than
    /// against a second list. The main page hosts the landing demo separately.
    #[test]
    fn every_demo_except_the_landing_preview_is_in_the_catalog() {
        let demos = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demos");
        let mut found: BTreeSet<String> = fs::read_dir(&demos)
            .expect("the demos directory is beside the workspace root")
            .map(|entry| entry.expect("a readable directory entry").path())
            .filter(|path| path.join("Trunk.toml").is_file())
            .map(|path| {
                path.file_name()
                    .expect("a demo directory has a name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(
            found.remove("landing"),
            "the main page's landing demo is missing under {demos:?}"
        );

        let listed: BTreeSet<String> = ENTRIES.iter().map(|entry| entry.name.to_owned()).collect();
        assert_eq!(
            found, listed,
            "the catalog and the demos directory disagree"
        );
    }
}
