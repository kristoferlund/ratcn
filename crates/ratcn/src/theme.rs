//! The base palette every component derives its colors from, and the one place
//! a declared style override is resolved against it.
//!
//! A [`Theme`] stores base colors only. Each component owns a style struct whose
//! `from_theme` derives every slot it paints from those bases (with the shifts in
//! [`color`](crate::color)), and a `style(...)` builder that replaces the whole
//! derivation with a closure. [`resolve_style`] is the fork between the two,
//! shared so a theme switch cannot reach some components and miss others.

use ratatui::style::Color;

use crate::color::dim;
use ratatui::symbols::border;

/// The style a component paints with: the override it was declared with, or the
/// colors derived from the active theme.
///
/// `custom` is the closure a component's `style(...)` builder stored. It is run
/// against the theme the frame is being painted with, never once at declaration —
/// which is what makes a style built from its argument follow a theme switch; a
/// fixed style ignores the argument. With nothing declared, `from_theme` — the
/// component's own derivation — answers instead.
///
/// Every built-in resolves in `paint`, where the colors are actually needed, bar
/// [`Select`](crate::Select), which also resolves in `render` because the style
/// is a prop of the panel it declares there.
#[must_use]
pub fn resolve_style<S>(
    custom: Option<&dyn Fn(&Theme) -> S>,
    theme: &Theme,
    from_theme: impl FnOnce(&Theme) -> S,
) -> S {
    match custom {
        Some(custom) => custom(theme),
        None => from_theme(theme),
    }
}

/// A border line style for component-local styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BorderStyle {
    /// Plain single lines: `┌─┐`.
    Single,
    /// Single lines with rounded corners: `╭─╮`.
    Rounded,
    /// Double lines: `╔═╗`.
    Double,
    /// Thick lines: `┏━┓`.
    Heavy,
}

impl BorderStyle {
    /// The matching ratatui border set, for drawing with a `Block`.
    #[must_use]
    pub const fn to_border_set(self) -> border::Set<'static> {
        match self {
            Self::Single => border::PLAIN,
            Self::Rounded => border::ROUNDED,
            Self::Double => border::DOUBLE,
            Self::Heavy => border::THICK,
        }
    }
}

/// Backing storage for [`Theme::presets`].
const PRESETS: [Theme; 7] = [
    Theme::default_dark(),
    Theme::terminal(),
    Theme::catppuccin(),
    Theme::gruvbox(),
    Theme::nord(),
    Theme::tokyo_night(),
    Theme::solarized(),
];

/// Preset focus rings are derived, not stored: the preset's `primary` dimmed
/// this much toward its `background`.
const RING_DIM: u16 = 20;

/// A theme is a small color palette. Every field names a *purpose*, not a
/// component: the same `primary` paints a default button, a selected list row,
/// and a bar; the same `field` paints any inset "well" (lists, panels, bars).
///
/// Focus and disabled variants are derived by the components that need them,
/// not stored here (see [`crate::color`]): a focused fill shifts a fixed amount
/// in the direction that makes it stand out — a bright fill darkens, a dark fill
/// (secondary, the inset `field`) lightens — and a disabled fill dims toward the
/// surface.
///
/// `accent` is the palette's vivid signature color (Catppuccin mauve, Gruvbox
/// orange, Solarized magenta, …), kept for the occasional creative splash — a
/// notification, a highlighted border — distinct from `primary`, the workhorse
/// accent components reach for by default.
///
/// # Authoring a palette
///
/// The type is `#[non_exhaustive]`, so a role can be added without breaking
/// every app that names a palette. That rules out a struct literal; start from
/// a preset and assign the fields you want instead. The fields stay public and
/// the presets are `const`, so this still works in a `const` context:
///
/// ```
/// use ratatui::style::Color;
/// use ratcn::Theme;
///
/// const fn operations() -> Theme {
///     let mut theme = Theme::default_dark();
///     theme.name = "Operations";
///     theme.accent = Color::LightCyan;
///     theme
/// }
///
/// const THEME: Theme = operations();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Theme {
    /// Display name, for a theme picker.
    pub name: &'static str,

    // Text
    /// Ordinary text.
    pub foreground: Color,
    /// De-emphasized text: placeholders, descriptions, disabled labels.
    pub muted_foreground: Color,

    // Surfaces, back to front
    /// The furthest-back surface, behind everything else.
    pub background: Color,
    /// Raised surfaces that sit above the background — dialogs and toasts.
    pub surface: Color,
    /// Control surfaces — lists, select panels, and chart backdrops.
    pub field: Color,

    // Accents
    /// The workhorse accent: default buttons, selected rows, bars.
    pub primary: Color,
    /// Text drawn on top of `primary`, so it must contrast with it.
    pub primary_foreground: Color,
    /// The quieter accent, for secondary buttons and unselected tabs.
    pub secondary: Color,
    /// Text drawn on top of `secondary`.
    pub secondary_foreground: Color,
    /// The palette's vivid signature color, held back for occasional emphasis
    /// rather than used as a second `primary`.
    pub accent: Color,
    /// Destructive actions and error states.
    pub destructive: Color,
    /// Text drawn on top of `destructive`.
    pub destructive_foreground: Color,
    /// Warnings — less severe than `destructive`.
    pub warning: Color,

    // Lines and caret
    /// Borders and separators at rest.
    pub border: Color,
    /// The focus accent around a container — a focused pane's border, the
    /// modal dialog's frame. The shadcn `ring` role. Presets derive it from
    /// `primary`, dimmed 20% toward `background`.
    pub ring: Color,
    /// A text caret. No component ships with one today; the role is kept so
    /// text-entry components can be themed consistently when they land.
    pub cursor: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_dark()
    }
}

impl Theme {
    /// A dark default theme inspired by shadcn/ui's default OKLCH palette,
    /// translated to opaque sRGB terminal colors.
    #[must_use]
    pub const fn default_dark() -> Self {
        let primary = Color::Rgb(229, 229, 229);
        let background = Color::Rgb(10, 10, 10);
        Self {
            name: "Default",
            foreground: Color::Rgb(250, 250, 250),
            muted_foreground: Color::Rgb(161, 161, 161),
            background,
            surface: Color::Rgb(18, 18, 18),
            field: Color::Rgb(31, 31, 31),
            primary,
            primary_foreground: Color::Rgb(23, 23, 23),
            secondary: Color::Rgb(38, 38, 38),
            secondary_foreground: Color::Rgb(250, 250, 250),
            accent: Color::Rgb(139, 92, 246),
            destructive: Color::Rgb(255, 100, 103),
            destructive_foreground: Color::Rgb(250, 250, 250),
            warning: Color::Rgb(250, 204, 21),
            border: Color::Rgb(94, 94, 94),
            ring: dim(primary, background, RING_DIM),
            cursor: Color::Rgb(115, 115, 115),
        }
    }

    /// Terminal-friendly colors: inherit the outer terminal background, but use
    /// concrete neutral surfaces for component wells. [`Color::Reset`] carries
    /// no RGB channels, so derived states like focused list backgrounds
    /// need real surface colors to lighten from. Named colors (`LightBlue`,
    /// `Yellow`, …) keep the terminal palette at rest; their derived
    /// focus/hover/disabled states resolve through
    /// [`resolve_rgb`](crate::color::resolve_rgb)'s fixed VGA approximation.
    #[must_use]
    pub const fn terminal() -> Self {
        let primary = Color::LightBlue;
        let background = Color::Reset;
        Self {
            name: "Terminal",
            foreground: Color::Reset,
            muted_foreground: Color::Gray,
            background,
            surface: Color::Rgb(18, 18, 18),
            field: Color::Rgb(31, 31, 31),
            primary,
            primary_foreground: Color::Black,
            secondary: Color::Rgb(38, 38, 38),
            secondary_foreground: Color::Reset,
            accent: Color::LightMagenta,
            destructive: Color::LightRed,
            destructive_foreground: Color::Black,
            warning: Color::Yellow,
            border: Color::DarkGray,
            ring: dim(primary, background, RING_DIM),
            cursor: Color::LightBlue,
        }
    }

    /// The Catppuccin Mocha palette: soft pastels on a deep indigo background.
    #[must_use]
    pub const fn catppuccin() -> Self {
        let primary = Color::Rgb(137, 180, 250);
        let background = Color::Rgb(17, 17, 27);
        Self {
            name: "Catppuccin",
            foreground: Color::Rgb(205, 214, 244),
            muted_foreground: Color::Rgb(166, 173, 200),
            background,
            surface: Color::Rgb(30, 30, 46),
            field: Color::Rgb(43, 43, 61),
            primary,
            primary_foreground: Color::Rgb(17, 17, 27),
            secondary: Color::Rgb(49, 50, 68),
            secondary_foreground: Color::Rgb(205, 214, 244),
            accent: Color::Rgb(203, 166, 247),
            destructive: Color::Rgb(243, 139, 168),
            destructive_foreground: Color::Rgb(17, 17, 27),
            warning: Color::Rgb(250, 179, 135),
            border: Color::Rgb(94, 97, 122),
            ring: dim(primary, background, RING_DIM),
            cursor: Color::Rgb(137, 180, 250),
        }
    }

    /// The Gruvbox dark palette: warm, low-contrast retro tones.
    #[must_use]
    pub const fn gruvbox() -> Self {
        let primary = Color::Rgb(250, 189, 47);
        let background = Color::Rgb(40, 40, 40);
        Self {
            name: "Gruvbox",
            foreground: Color::Rgb(235, 219, 178),
            muted_foreground: Color::Rgb(168, 153, 132),
            background,
            surface: Color::Rgb(60, 56, 54),
            field: Color::Rgb(73, 67, 64),
            primary,
            primary_foreground: Color::Rgb(40, 40, 40),
            secondary: Color::Rgb(80, 73, 69),
            secondary_foreground: Color::Rgb(235, 219, 178),
            accent: Color::Rgb(254, 128, 25),
            destructive: Color::Rgb(251, 73, 52),
            destructive_foreground: Color::Rgb(40, 40, 40),
            warning: Color::Rgb(250, 189, 47),
            border: Color::Rgb(124, 112, 102),
            ring: dim(primary, background, RING_DIM),
            cursor: Color::Rgb(250, 189, 47),
        }
    }

    /// The Nord palette: cool desaturated blues.
    #[must_use]
    pub const fn nord() -> Self {
        let primary = Color::Rgb(136, 192, 208);
        let background = Color::Rgb(46, 52, 64);
        Self {
            name: "Nord",
            foreground: Color::Rgb(216, 222, 233),
            muted_foreground: Color::Rgb(129, 161, 193),
            background,
            surface: Color::Rgb(59, 66, 82),
            field: Color::Rgb(64, 73, 90),
            primary,
            primary_foreground: Color::Rgb(46, 52, 64),
            secondary: Color::Rgb(67, 76, 94),
            secondary_foreground: Color::Rgb(216, 222, 233),
            accent: Color::Rgb(180, 142, 173),
            destructive: Color::Rgb(191, 97, 106),
            destructive_foreground: Color::Rgb(46, 52, 64),
            warning: Color::Rgb(235, 203, 139),
            border: Color::Rgb(112, 126, 155),
            ring: dim(primary, background, RING_DIM),
            cursor: Color::Rgb(136, 192, 208),
        }
    }

    /// The Tokyo Night palette: saturated blues and purples on near-black.
    #[must_use]
    pub const fn tokyo_night() -> Self {
        let primary = Color::Rgb(122, 162, 247);
        let background = Color::Rgb(26, 27, 38);
        Self {
            name: "Tokyo Night",
            foreground: Color::Rgb(192, 202, 245),
            muted_foreground: Color::Rgb(120, 134, 180),
            background,
            surface: Color::Rgb(36, 40, 59),
            field: Color::Rgb(55, 61, 89),
            primary,
            primary_foreground: Color::Rgb(26, 27, 38),
            secondary: Color::Rgb(65, 72, 104),
            secondary_foreground: Color::Rgb(192, 202, 245),
            accent: Color::Rgb(187, 154, 247),
            destructive: Color::Rgb(247, 118, 142),
            destructive_foreground: Color::Rgb(26, 27, 38),
            warning: Color::Rgb(224, 175, 104),
            border: Color::Rgb(92, 102, 146),
            ring: dim(primary, background, RING_DIM),
            cursor: Color::Rgb(122, 162, 247),
        }
    }

    /// The Solarized Dark palette: low-contrast, carefully balanced.
    #[must_use]
    pub const fn solarized() -> Self {
        let primary = Color::Rgb(39, 139, 211);
        let background = Color::Rgb(0, 20, 26);
        Self {
            name: "Solarized",
            foreground: Color::Rgb(159, 171, 173),
            muted_foreground: Color::Rgb(88, 110, 117),
            background,
            surface: Color::Rgb(0, 45, 56),
            field: Color::Rgb(0, 58, 63),
            primary,
            primary_foreground: Color::Rgb(253, 246, 227),
            secondary: Color::Rgb(7, 54, 66),
            secondary_foreground: Color::Rgb(173, 184, 184),
            accent: Color::Rgb(211, 54, 130),
            destructive: Color::Rgb(220, 49, 46),
            destructive_foreground: Color::Rgb(253, 246, 227),
            warning: Color::Rgb(181, 137, 0),
            border: Color::Rgb(94, 117, 125),
            ring: dim(primary, background, RING_DIM),
            cursor: Color::Rgb(42, 162, 152),
        }
    }

    /// Every built-in theme, in a stable order — for a theme picker or a
    /// cycle-themes hotkey.
    ///
    /// The slice is returned rather than a fixed-size array so that adding a
    /// preset does not break callers: index it, iterate it, or take its
    /// `len()`, but do not depend on the count.
    #[must_use]
    pub const fn presets() -> &'static [Self] {
        &PRESETS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{FIELD_FOCUS_LIGHTEN, lighten};

    /// Relative luminance, per WCAG 2.1.
    fn luminance(color: Color) -> f64 {
        let Color::Rgb(r, g, b) = color else {
            panic!("expected an rgb color, got {color:?}");
        };
        let channel = |v: u8| {
            let c = f64::from(v) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    // A border is a UI boundary, so WCAG 2.1 non-text contrast (1.4.11) asks for
    // 3:1 against what it sits on. Every rgb preset was below that once — the
    // field borders were nearly invisible on a dark background — so this pins
    // the floor for future palette edits.
    #[test]
    fn preset_borders_meet_non_text_contrast_against_their_background() {
        for theme in Theme::presets() {
            // The terminal preset inherits the user's own ANSI palette, so its
            // contrast is not ours to measure.
            if theme.name == "Terminal" {
                continue;
            }
            let ratio = contrast(theme.border, theme.background);
            assert!(
                ratio >= 3.0,
                "{} border contrast is {ratio:.2}:1, below the 3:1 floor",
                theme.name
            );
        }
    }

    #[test]
    fn preset_fields_are_distinct_on_backgrounds_and_surfaces() {
        for theme in Theme::presets() {
            // The terminal background inherits an unknown user palette.
            if theme.name == "Terminal" {
                continue;
            }
            for (role, backdrop) in [("background", theme.background), ("surface", theme.surface)] {
                let ratio = contrast(theme.field, backdrop);
                assert!(
                    ratio >= 1.1,
                    "{} field contrast on {role} is {ratio:.2}:1, below the 1.1:1 floor",
                    theme.name
                );
            }
        }
    }

    #[test]
    fn terminal_theme_has_derived_component_surfaces() {
        let theme = Theme::terminal();

        assert_eq!(theme.background, Color::Reset);
        assert!(matches!(theme.surface, Color::Rgb(_, _, _)));
        assert!(matches!(theme.field, Color::Rgb(_, _, _)));
        assert_ne!(lighten(theme.field, FIELD_FOCUS_LIGHTEN), theme.field);
    }
}
