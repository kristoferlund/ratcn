//! The choices the wizard collects, and the lines they turn into.
//!
//! A step writes here; later steps and the summary read it. The theme step also
//! writes the palette the whole app renders with, which is why the choice lives
//! here rather than inside that step.

use ratcn::Theme;

/// Which host the reader is building for. Picks the ratcn feature they need.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backend {
    #[default]
    Terminal,
    Browser,
}

impl Backend {
    pub const ALL: [Backend; 2] = [Backend::Terminal, Backend::Browser];

    pub const fn label(self) -> &'static str {
        match self {
            Backend::Terminal => "Terminal app",
            Backend::Browser => "Browser app",
        }
    }

    pub const fn feature(self) -> &'static str {
        match self {
            Backend::Terminal => "crossterm",
            Backend::Browser => "ratzilla",
        }
    }
}

/// The preset constructor behind each theme name, so the theme step can show the
/// line a reader would actually write rather than a name they would have to
/// translate. Covered for every preset by a test below.
const THEME_CONSTRUCTORS: [(&str, &str); 7] = [
    ("Default", "default_dark"),
    ("Terminal", "terminal"),
    ("Catppuccin", "catppuccin"),
    ("Gruvbox", "gruvbox"),
    ("Nord", "nord"),
    ("Tokyo Night", "tokyo_night"),
    ("Solarized", "solarized"),
];

#[derive(Debug, Clone, Copy)]
pub struct Choices {
    pub backend: Backend,
    pub theme: &'static str,
}

impl Default for Choices {
    fn default() -> Self {
        Self {
            backend: Backend::default(),
            theme: Theme::default_dark().name,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ChoiceMsg {
    SetBackend(Backend),
    SetTheme(&'static str),
}

impl Choices {
    pub fn update(&mut self, msg: ChoiceMsg) {
        match msg {
            ChoiceMsg::SetBackend(backend) => self.backend = backend,
            ChoiceMsg::SetTheme(theme) => self.theme = theme,
        }
    }

    /// The palette every step renders with.
    pub fn palette(&self) -> Theme {
        Theme::presets()
            .iter()
            .find(|theme| theme.name == self.theme)
            .copied()
            .unwrap_or_default()
    }

    /// The install command for the chosen backend.
    pub fn cargo_add(&self) -> String {
        format!("cargo add ratcn --features {}", self.backend.feature())
    }

    /// The line that produces the chosen palette.
    pub fn theme_line(&self) -> String {
        format!("let theme = Theme::{}();", constructor(self.theme))
    }
}

fn constructor(name: &str) -> &'static str {
    THEME_CONSTRUCTORS
        .iter()
        .find(|(theme, _)| *theme == name)
        .map_or("default_dark", |(_, constructor)| *constructor)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The theme step prints `Theme::<constructor>()` as copyable code. A preset
    /// missing from the table would silently print `Theme::default_dark()` next
    /// to a differently-coloured screen, so every preset must be listed.
    #[test]
    fn every_preset_has_a_constructor() {
        for theme in Theme::presets() {
            assert!(
                THEME_CONSTRUCTORS
                    .iter()
                    .any(|(name, _)| *name == theme.name),
                "theme preset {:?} has no constructor in THEME_CONSTRUCTORS",
                theme.name
            );
        }
    }

    /// The backend choice is only worth collecting because it changes the
    /// command the reader copies.
    #[test]
    fn the_backend_choice_picks_the_feature_in_the_install_command() {
        let mut choices = Choices::default();

        assert_eq!(choices.cargo_add(), "cargo add ratcn --features crossterm");

        choices.update(ChoiceMsg::SetBackend(Backend::Browser));

        assert_eq!(choices.cargo_add(), "cargo add ratcn --features ratzilla");
    }

    #[test]
    fn the_theme_choice_selects_both_the_palette_and_the_line_that_makes_it() {
        let mut choices = Choices::default();

        choices.update(ChoiceMsg::SetTheme("Tokyo Night"));

        assert_eq!(choices.palette(), Theme::tokyo_night());
        assert_eq!(choices.theme_line(), "let theme = Theme::tokyo_night();");
    }
}
