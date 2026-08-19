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
/// [`Select`](crate::Select), which also resolves in `declare` because the
/// style is a prop of the panel it declares there.
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
///
/// The softening is small because a ring is a boundary — it has to clear 3:1
/// against both the background it is drawn on and the surface it frames, and
/// dimming toward the background is the ring spending exactly the contrast it
/// exists to have. A mid-luminance accent (Solarized's blue) runs out first.
const RING_DIM: u16 = 10;

/// A theme is a small color palette. Every field names a *purpose*, not a
/// component: the same `primary` paints a default button, a selected list row,
/// and a bar; the same `field` paints any inset "well" (lists, panels, bars).
///
/// Focus and disabled variants are derived by the components that need them,
/// not stored here (see [`crate::color`]): a focused fill shifts a fixed amount
/// in the direction that makes it stand out — a bright fill darkens, a dark fill
/// (secondary, the inset `field`) lightens — and a disabled control dims toward
/// the layer behind it: a button toward the `surface`, a well toward the
/// `background` it already sits close to.
///
/// # The well ladder
///
/// `field` is the bottom rung of a four-level ladder every well is painted
/// from, and it sits *close* to `background` — one tone above it, not a slab
/// that competes with the screen. A component lifts from there: focused is
/// lighter than at rest, focused-and-hovered lighter still, and the cursor row
/// inside a list is the lightest. The lift amounts live in [`crate::color`]
/// ([`FIELD_FOCUS_LIGHTEN`](crate::color::FIELD_FOCUS_LIGHTEN),
/// [`FIELD_HOVER_LIGHTEN`](crate::color::FIELD_HOVER_LIGHTEN)), so a palette
/// that gets `background` and `field` right gets the whole ladder right.
///
/// Text has to stay readable on every rung, which is what caps how far `field`
/// may sit from `background`: each rung spends contrast the palette can only
/// spend once. The built-in presets hold their `field` between 1.10:1 and
/// 1.35:1 of `background`, and every preset is checked against that window and
/// against WCAG text-contrast floors by the crate's own tests.
///
/// `surface` — dialogs and toasts — sits *between* the two: raised off the
/// background enough to read as a layer, still under the well, so a list
/// declared inside a dialog keeps the same "lifted out of what it sits on"
/// relationship it has on the screen. The whole order, back to front, is
/// `background` < `surface` < `field` < the rungs above it.
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
    /// `primary`, dimmed 10% toward `background`.
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
            // The same tone `primary_foreground` uses, for the same reason:
            // both are labels on a bright fill. Dark text holds 6.21:1 on this
            // red and 4.88:1 once the pointer darkens it; a near-white label
            // bottoms out at 2.77:1, and though it climbs to 3.52:1 as the fill
            // darkens, it is climbing from under the floor.
            destructive_foreground: Color::Rgb(23, 23, 23),
            warning: Color::Rgb(250, 204, 21),
            border: Color::Rgb(94, 94, 94),
            ring: dim(primary, background, RING_DIM),
            cursor: Color::Rgb(115, 115, 115),
        }
    }

    /// Terminal-friendly colors: inherit the outer terminal background, but use
    /// concrete neutral surfaces for component wells. [`Color::Reset`] carries
    /// no RGB channels, so derived states like focused list backgrounds
    /// need real surface colors to lighten from. Named colors (`Gray`,
    /// `Yellow`, …) keep the terminal palette at rest; their derived
    /// focus/hover/disabled states resolve through
    /// [`resolve_rgb`](crate::color::resolve_rgb)'s fixed VGA approximation.
    ///
    /// That approximation is why `primary` is neutral here rather than a hue.
    /// A named accent only survives as long as nothing derives from it: the
    /// moment a default button is focused, `primary` is resolved to VGA and
    /// darkened, which replaces whatever blue the terminal had with a fixed
    /// `#0000e6`. A neutral primary degrades into a neutral gray instead, which
    /// reads as "pressed" against any palette — and it mirrors
    /// [`default_dark`](Self::default_dark), whose `primary` is a near-white.
    /// It is the terminal's bright white rather than its white so that it stays
    /// distinct from `muted_foreground`, which markers are drawn against.
    ///
    /// The well surfaces are the part this preset cannot make adaptive. They
    /// are concrete dark RGB, so a light terminal gets dark wells; the
    /// alternative — [`Color::Reset`] wells — costs every focus, hover, and
    /// cursor-row fill, because there is nothing to lighten.
    #[must_use]
    pub const fn terminal() -> Self {
        let primary = Color::White;
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
    ///
    /// Tones from `catppuccin/palette`'s `palette.json`, in the flavor's own
    /// layering order: `crust`, `mantle`, `base`, then the `surface` tones for
    /// interactive fills.
    ///
    /// `surface` is off-palette, 30% of the way from `crust` to `base`. The
    /// flavor's own middle tone, `mantle`, leaves only 1.07:1 between a dialog
    /// and a well drawn inside it, and the well is the layer with no frame to
    /// fall back on. `crust`→`base` is 1.14:1 in total, so the split is tight
    /// either way; this preset is the one that pins both floors.
    #[must_use]
    pub const fn catppuccin() -> Self {
        let primary = Color::Rgb(137, 180, 250); // blue #89b4fa
        let background = Color::Rgb(17, 17, 27); // crust #11111b
        let field = Color::Rgb(30, 30, 46); // base #1e1e2e
        Self {
            name: "Catppuccin",
            foreground: Color::Rgb(205, 214, 244), // text #cdd6f4
            muted_foreground: Color::Rgb(166, 173, 200), // subtext0 #a6adc8
            background,
            surface: dim(background, field, 30),
            field,
            primary,
            primary_foreground: Color::Rgb(17, 17, 27), // crust #11111b
            secondary: Color::Rgb(49, 50, 68),          // surface0 #313244
            secondary_foreground: Color::Rgb(205, 214, 244), // text #cdd6f4
            accent: Color::Rgb(203, 166, 247),          // mauve #cba6f7
            destructive: Color::Rgb(243, 139, 168),     // red #f38ba8
            destructive_foreground: Color::Rgb(17, 17, 27), // crust #11111b
            warning: Color::Rgb(250, 179, 135),         // peach #fab387
            border: Color::Rgb(108, 112, 134),          // overlay0 #6c7086
            ring: dim(primary, background, RING_DIM),
            cursor: Color::Rgb(137, 180, 250), // blue #89b4fa
        }
    }

    /// The Gruvbox dark palette: warm, low-contrast retro tones.
    ///
    /// Tones from `morhetz/gruvbox`'s `colors/gruvbox.vim`, which names the
    /// dark ramp `dark0`…`dark4` and the light ramp `light0`…`light4`, and uses
    /// the *bright* accent variants in dark mode.
    ///
    /// `muted_foreground` is `light2` (the palette's `fg2`) rather than the
    /// dimmer `light4` (`fg4`), which drops under 4.5:1 against the upper rungs
    /// of the well ladder. Gruvbox's own comment color is dimmer still —
    /// `gray` `#928374` — and is not used here at all.
    ///
    /// `bright_red` is mid-luminance, so no tone in the palette labels it at
    /// 4.5:1 across a button's states: `dark0_hard`, the darkest there is,
    /// holds 4.77:1 falling to 3.78:1, and the light end is worse still at
    /// 3.03:1. Gruvbox's own `ErrorMsg` — `dark0` on `bright_red` — is one of
    /// the pairs that misses.
    ///
    /// `neutral_red` under `light0` would hold 4.82:1, but `destructive` is
    /// also the toast's error accent, and there it is a frame and an icon on
    /// the toast's own surface: the brighter red is 3.82:1 there and the
    /// neutral one 2.40:1. One role, two jobs, and only the bright red serves
    /// both at all — so the button label takes the documented exception rather
    /// than the palette taking a red it does not use in dark mode.
    #[must_use]
    pub const fn gruvbox() -> Self {
        let primary = Color::Rgb(250, 189, 47); // bright yellow #fabd2f
        let background = Color::Rgb(40, 40, 40); // dark0 #282828
        Self {
            name: "Gruvbox",
            foreground: Color::Rgb(235, 219, 178), // light1 #ebdbb2
            muted_foreground: Color::Rgb(213, 196, 161), // light2 #d5c4a1
            background,
            surface: Color::Rgb(50, 48, 47), // dark0_soft #32302f
            field: Color::Rgb(60, 56, 54),   // dark1 #3c3836
            primary,
            primary_foreground: Color::Rgb(40, 40, 40), // dark0 #282828
            secondary: Color::Rgb(80, 73, 69),          // dark2 #504945
            secondary_foreground: Color::Rgb(235, 219, 178), // light1 #ebdbb2
            accent: Color::Rgb(254, 128, 25),           // bright orange #fe8019
            destructive: Color::Rgb(251, 73, 52),       // bright red #fb4934
            // dark0_hard, the palette's darkest tone, rather than dark0: the
            // extra step buys the whole trajectory 0.4:1. See the note above.
            destructive_foreground: Color::Rgb(29, 32, 33), // dark0_hard #1d2021
            warning: Color::Rgb(250, 189, 47),              // bright yellow #fabd2f
            border: Color::Rgb(124, 111, 100),              // dark4 #7c6f64
            ring: dim(primary, background, RING_DIM),
            cursor: Color::Rgb(250, 189, 47), // bright yellow #fabd2f
        }
    }

    /// The Nord palette: cool desaturated blues.
    ///
    /// Tones from `nordtheme/nord`'s `src/nord.css`. Polar Night `nord0` is the
    /// origin background and `nord1` the elevated UI tone the palette suggests
    /// for raised surfaces; `nord2` and `nord3` are the two above it. Snow
    /// Storm `nord4` is primary text and `nord6` the brightest.
    ///
    /// Text uses `nord6`/`nord4` rather than `nord4`/`nord9`. Nord's ramp jumps
    /// straight from Polar Night to Snow Storm with no dim neutral in between,
    /// so the two emphases have to come from the light end; `nord9` is a Frost
    /// *accent*, and reading list rows in it both misuses the role and lands
    /// under 4.5:1.
    ///
    /// Nord has no tone between `nord0` and `nord1`, so `surface` is the
    /// midpoint of the two: a dialog has to be raised off the background and
    /// still stay under the well.
    ///
    /// `nord11` is the one red Nord ships and it sits mid-luminance, so no tone
    /// on either side of it reaches 4.5:1 as a label — the palette cannot serve
    /// this pair, and the crate's contrast test says so by name. The light side
    /// is chosen because it is the side that improves as the fill darkens under
    /// focus and the pointer.
    #[must_use]
    pub const fn nord() -> Self {
        let primary = Color::Rgb(136, 192, 208); // nord8 #88c0d0
        let background = Color::Rgb(46, 52, 64); // nord0 #2e3440
        let field = Color::Rgb(59, 66, 82); // nord1 #3b4252
        Self {
            name: "Nord",
            foreground: Color::Rgb(236, 239, 244), // nord6 #eceff4
            muted_foreground: Color::Rgb(216, 222, 233), // nord4 #d8dee9
            background,
            surface: dim(background, field, 50),
            field,
            primary,
            primary_foreground: Color::Rgb(46, 52, 64), // nord0 #2e3440
            secondary: Color::Rgb(76, 86, 106),         // nord3 #4c566a
            secondary_foreground: Color::Rgb(236, 239, 244), // nord6 #eceff4
            accent: Color::Rgb(180, 142, 173),          // nord15 #b48ead
            destructive: Color::Rgb(191, 97, 106),      // nord11 #bf616a
            // The light side of nord11, which is the better of two bad sides:
            // nord0 is 3.05:1 and falls to 2.45:1 under the pointer, nord6 is
            // 3.55:1 and climbs to 4.42:1. See the note above.
            destructive_foreground: Color::Rgb(236, 239, 244), // nord6 #eceff4
            warning: Color::Rgb(235, 203, 139),                // nord13 #ebcb8b
            // Off-palette, tuned for the 3:1 a boundary needs on nord0: no Nord
            // tone lands in the window. nord3, the palette's own line color, is
            // 1.693:1 there, and the next tone up is nord4, which is text.
            border: Color::Rgb(112, 126, 155),
            ring: dim(primary, background, RING_DIM),
            cursor: Color::Rgb(136, 192, 208), // nord8 #88c0d0
        }
    }

    /// The Tokyo Night palette: saturated blues and purples on near-black.
    ///
    /// Tones from `folke/tokyonight.nvim`'s `night` variant
    /// (`lua/tokyonight/colors/storm.lua` with the `night.lua` overrides).
    ///
    /// `surface` is the midpoint of `bg` and `bg_highlight` rather than the
    /// variant's `bg_float`. Floats in tokyonight are *darker* than the editor
    /// background, which a dialog here cannot be: the modal backdrop blends the
    /// screen toward `background`, so a darker dialog would sink into the very
    /// thing it floats over.
    ///
    /// `secondary` is `bg_visual`, the variant's selection fill, rather than
    /// `terminal_black`. It is the only background tone dark enough to keep a
    /// hovered secondary label at 4.5:1 once the fill lightens, while still
    /// reading as a raised control against `bg`.
    #[must_use]
    pub const fn tokyo_night() -> Self {
        let primary = Color::Rgb(122, 162, 247); // blue #7aa2f7
        let background = Color::Rgb(26, 27, 38); // bg #1a1b26
        let field = Color::Rgb(41, 46, 66); // bg_highlight #292e42
        Self {
            name: "Tokyo Night",
            foreground: Color::Rgb(192, 202, 245), // fg #c0caf5
            muted_foreground: Color::Rgb(169, 177, 214), // fg_dark #a9b1d6
            background,
            surface: dim(background, field, 50),
            field,
            primary,
            primary_foreground: Color::Rgb(26, 27, 38), // bg #1a1b26
            secondary: Color::Rgb(40, 52, 87),          // bg_visual #283457
            secondary_foreground: Color::Rgb(192, 202, 245), // fg #c0caf5
            accent: Color::Rgb(187, 154, 247),          // magenta #bb9af7
            destructive: Color::Rgb(247, 118, 142),     // red #f7768e
            destructive_foreground: Color::Rgb(26, 27, 38), // bg #1a1b26
            warning: Color::Rgb(224, 175, 104),         // yellow #e0af68
            border: Color::Rgb(115, 122, 162),          // dark5 #737aa2
            ring: dim(primary, background, RING_DIM),
            cursor: Color::Rgb(122, 162, 247), // blue #7aa2f7
        }
    }

    /// The Solarized Dark palette: low-contrast, carefully balanced.
    ///
    /// Tones from Ethan Schoonover's `altercation/solarized` README. Dark mode
    /// uses `base03` as background and `base02` as its one background
    /// highlight — the palette ships no third background tone, so the well is
    /// `base02`, `surface` is off-palette 30% of the way between them, and the
    /// ladder above the well is derived. `base03`→`base02` is 1.16:1 in total
    /// and the well takes the larger share of it, since a dialog has a ring
    /// frame to be found by and a well has nothing.
    ///
    /// Text is shifted one rung up the ramp from Schoonover's own dark mapping
    /// (`base2`/`base1` where he uses `base1`/`base0`). His mapping puts body
    /// text at 4.75:1 on `base03` — right at the WCAG floor with no headroom —
    /// and every rung of the well ladder spends some of it. The tones are the
    /// palette's; only which rung plays which role moved.
    #[must_use]
    pub const fn solarized() -> Self {
        let primary = Color::Rgb(38, 139, 210); // blue #268bd2
        let background = Color::Rgb(0, 43, 54); // base03 #002b36
        let field = Color::Rgb(7, 54, 66); // base02 #073642
        Self {
            name: "Solarized",
            foreground: Color::Rgb(238, 232, 213), // base2 #eee8d5
            muted_foreground: Color::Rgb(147, 161, 161), // base1 #93a1a1
            background,
            surface: dim(background, field, 30),
            field,
            primary,
            // base3, the light-mode background, not base03. Nothing clears
            // 4.5:1 on this blue, so the choice is which direction the button
            // moves under the pointer: base3 climbs 3.41 → 4.10 → 4.99 as the
            // fill darkens, while base03 starts at 4.08 and falls to 2.78.
            primary_foreground: Color::Rgb(253, 246, 227),
            secondary: Color::Rgb(88, 110, 117), // base01 #586e75
            secondary_foreground: Color::Rgb(253, 246, 227), // base3 #fdf6e3
            accent: Color::Rgb(211, 54, 130),    // magenta #d33682
            destructive: Color::Rgb(220, 50, 47), // red #dc322f
            destructive_foreground: Color::Rgb(253, 246, 227), // base3 #fdf6e3
            warning: Color::Rgb(181, 137, 0),    // yellow #b58900
            border: Color::Rgb(101, 123, 131),   // base00 #657b83
            ring: dim(primary, background, RING_DIM),
            cursor: Color::Rgb(42, 161, 152), // cyan #2aa198
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
    use crate::color::{FIELD_FOCUS_LIGHTEN, FOCUS_DARKEN, darken, lighten, resolve_rgb};
    use crate::{ButtonStyle, ButtonVariant, DialogStyle, ListStyle, TabsStyle, ToasterStyle};

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

    /// A border is a UI boundary, so WCAG 2.1 non-text contrast (1.4.11) asks
    /// for 3:1 against what it sits on. Every rgb preset was below that once —
    /// the field borders were nearly invisible on a dark background — so this
    /// pins the floor for future palette edits.
    ///
    /// The ring is measured against the surface as well, because a dialog frame
    /// is drawn around one and the dialog title is painted in it. That title is
    /// the one piece of text held to the boundary floor rather than the text
    /// floor: it is the ring doing double duty, and holding it to 4.5:1 would
    /// mean no palette could tint its dialog frames with its own accent.
    ///
    /// The border is measured against the background only. Against the surface
    /// it is a weaker line — `default_dark`, the reference, is 2.89:1 there —
    /// and pinning that would mean retuning the theme the others are tuned
    /// against.
    #[test]
    fn preset_lines_meet_non_text_contrast_where_they_are_drawn() {
        for theme in measurable_presets() {
            for (role, line, backdrop) in [
                ("border", theme.border, theme.background),
                ("ring", theme.ring, theme.background),
                ("ring on the surface it frames", theme.ring, theme.surface),
            ] {
                let ratio = contrast(line, backdrop);
                assert!(
                    ratio >= 3.0,
                    "{} {role} contrast is {ratio:.2}:1, below the 3:1 floor",
                    theme.name
                );
            }
        }
    }

    /// Every preset whose colors are ours to measure. The terminal preset
    /// inherits the user's own ANSI palette, so none of these floors apply
    /// to it.
    fn measurable_presets() -> impl Iterator<Item = &'static Theme> {
        Theme::presets()
            .iter()
            .filter(|theme| theme.name != "Terminal")
    }

    /// A well at rest must read as a distinct region without competing with the
    /// screen behind it. `default_dark` — the reference the other presets are
    /// tuned against — sits at 1.20:1, and every rung above it spends contrast
    /// the text still needs, so the window is narrow on both sides: below 1.10
    /// the well disappears, above 1.35 it becomes a slab and the ladder on top
    /// of it runs out of room.
    #[test]
    fn preset_wells_sit_close_to_their_background() {
        for theme in measurable_presets() {
            let ratio = contrast(theme.field, theme.background);
            assert!(
                (1.10..=1.35).contains(&ratio),
                "{} field is {ratio:.2}:1 on its background, outside the 1.10..=1.35 window",
                theme.name
            );
        }
    }

    /// A raised surface is read against a backdrop the modal layer has already
    /// blended toward `background`, so it has to differ from `background`
    /// itself — a dialog painted in the background color sinks into the very
    /// thing it floats over. It also has to stay *under* the well: a list
    /// declared inside a dialog is lifted out of the dialog exactly as it is
    /// lifted out of the screen, and a surface above the well would invert
    /// that and leave the list looking sunken into the dialog.
    #[test]
    fn preset_surfaces_sit_between_the_background_and_the_well() {
        for theme in measurable_presets() {
            // The two steps carry different floors because they are found
            // differently. A dialog has a ring frame around it, pinned at 3:1
            // above, so its fill only has to be a different color; a well has
            // no frame at all, so its fill is the whole separation.
            for (lower, upper, floor, pair) in [
                (
                    theme.background,
                    theme.surface,
                    1.03,
                    "surface over background",
                ),
                (theme.surface, theme.field, 1.10, "well over surface"),
            ] {
                let ratio = contrast(lower, upper);
                assert!(
                    ratio >= floor,
                    "{}: {pair} is {ratio:.3}:1, below the {floor}:1 floor",
                    theme.name
                );
                let lifted = luminance(upper) > luminance(lower);
                assert_eq!(
                    lifted,
                    luminance(theme.background) < 0.5,
                    "{}: {pair} is lifted the wrong way",
                    theme.name
                );
            }
        }
    }

    /// The four rungs a well is painted from, in the order a list climbs them,
    /// taken from the component itself so the ladder under test is the one that
    /// actually gets painted.
    fn well_ladder(theme: &Theme) -> [(&'static str, Color); 4] {
        let style = ListStyle::from_theme(theme);
        [
            ("well at rest", style.background),
            ("focused well", style.focused_background),
            ("hovered well", style.hovered_background),
            ("cursor row", style.focused_row_background),
        ]
    }

    /// Each rung is a step further from the background, in whichever direction
    /// the background is not: a dark theme climbs toward white, a light one
    /// descends toward black. Steps below 1.05:1 are there in the numbers but
    /// not on the screen, which would leave focus or hover with no visible
    /// answer.
    #[test]
    fn preset_well_ladders_step_away_from_the_background_in_order() {
        for theme in measurable_presets() {
            let light_theme = luminance(theme.background) > 0.5;
            let mut previous = ("background", theme.background);
            for (rung, color) in well_ladder(theme) {
                let climbs = luminance(color) > luminance(previous.1);
                assert_eq!(
                    climbs, !light_theme,
                    "{}: {rung} {color:?} moves the wrong way from {} {:?}",
                    theme.name, previous.0, previous.1
                );
                let step = contrast(color, previous.1);
                assert!(
                    step >= 1.05,
                    "{}: {rung} is only {step:.3}:1 from {}, an invisible step",
                    theme.name,
                    previous.0
                );
                previous = (rung, color);
            }
        }
    }

    /// The floor a pair must clear. 4.5:1 is WCAG 2.1's normal-text guidance
    /// (1.4.3), and it is what every preset is held to except where the palette
    /// itself cannot serve the pair. Both exceptions below are palette facts,
    /// not tuning that stopped early, and each names the number it settles for.
    ///
    /// Solarized is narrow everywhere: Schoonover's own dark body pair, `base0`
    /// on `base03`, is 4.75:1, so the palette starts with almost no headroom
    /// and every lifted fill spends some of it. Its two worst pairs are a
    /// default button's label at rest, 3.41:1 on the undarkened blue, and a
    /// ghost button's label under the pointer, 3.42:1.
    ///
    /// Nord and Gruvbox are narrow in one place each, and it is the same place:
    /// a mid-luminance red that no tone of their own can label at 4.5:1 from
    /// either side. Nord ships one red at all; Gruvbox's other reds cost more
    /// elsewhere than they buy here, since `destructive` is a toast's frame and
    /// icon as well as a button's fill. `nord6` on `nord11` is 3.55:1 at its
    /// worst and `dark0_hard` on `bright_red` 3.78:1.
    ///
    /// The Nord and Gruvbox exception is scoped by pair *name*, so a pair added
    /// later under a name starting the same way would inherit it. Name new
    /// pairs accordingly, or key this differently.
    fn contrast_floor(theme: &Theme, pair: &str) -> f64 {
        match theme.name {
            "Nord" | "Gruvbox" if pair.starts_with("destructive button") => 3.5,
            "Solarized" => 3.4,
            _ => 4.5,
        }
    }

    /// Every foreground-on-fill pair a themed component paints as text, named by
    /// what a reader would call it — including the fills the components derive
    /// for focus and hover, which is where a label's contrast is spent, and
    /// which are taken from the real style constructors rather than recomputed.
    ///
    /// One kind of pair is deliberately absent: disabled pairs, covered by
    /// `preset_disabled_wells_read_as_disabled` instead. WCAG 1.4.3 exempts
    /// inactive components, and a disabled control that met the text floor
    /// would not look disabled.
    fn text_pairs(theme: &Theme) -> Vec<(String, Color, Color)> {
        let mut pairs = surface_text_pairs(theme);
        pairs.append(&mut well_text_pairs(theme));
        pairs.append(&mut control_text_pairs(theme));
        pairs
    }

    /// The pairs painted straight onto a theme's own layers.
    fn surface_text_pairs(theme: &Theme) -> Vec<(String, Color, Color)> {
        let dialog = DialogStyle::from_theme(theme);
        let toast = ToasterStyle::from_theme(theme);
        vec![
            (
                "dialog description on its box".to_owned(),
                dialog.description_foreground,
                dialog.background,
            ),
            (
                "body text on background".to_owned(),
                theme.foreground,
                theme.background,
            ),
            (
                "muted text on background".to_owned(),
                theme.muted_foreground,
                theme.background,
            ),
            (
                "body text on surface".to_owned(),
                theme.foreground,
                theme.surface,
            ),
            (
                "muted text on surface".to_owned(),
                theme.muted_foreground,
                theme.surface,
            ),
            ("toast title".to_owned(), toast.foreground, toast.background),
            (
                "toast body".to_owned(),
                toast.muted_foreground,
                toast.background,
            ),
        ]
    }

    /// The pairs a list paints, over all four rungs of its well.
    fn well_text_pairs(theme: &Theme) -> Vec<(String, Color, Color)> {
        let style = ListStyle::from_theme(theme);
        let mut pairs = vec![
            (
                "cursor row text".to_owned(),
                style.focused_foreground,
                style.focused_row_background,
            ),
            (
                "selected cursor row text".to_owned(),
                style.selected_focused_foreground,
                style.selected_focused_background,
            ),
        ];
        // Ordinary and selected rows both keep whichever backdrop the list
        // has, so each of the first three rungs carries two row colors.
        for (rung, fill) in &well_ladder(theme)[..3] {
            pairs.push((format!("list row on {rung}"), style.foreground, *fill));
            pairs.push((
                format!("selected row on {rung}"),
                style.selected_foreground,
                *fill,
            ));
        }
        pairs
    }

    /// The pairs a control paints, at rest and on the fills it derives for
    /// focus and hover.
    ///
    /// A button's label stays put while its fill shifts under focus and the
    /// pointer, so the fill is moving toward the label it has to stay legible
    /// against — which is the half of the derivation nothing measured before.
    fn control_text_pairs(theme: &Theme) -> Vec<(String, Color, Color)> {
        let tabs = TabsStyle::from_theme(theme);
        let mut pairs = Vec::new();
        for (variant, name) in [
            (ButtonVariant::Default, "default button"),
            (ButtonVariant::Secondary, "secondary button"),
            (ButtonVariant::Destructive, "destructive button"),
            (ButtonVariant::Ghost, "ghost button"),
            (ButtonVariant::Outline, "outline button"),
        ] {
            let button = ButtonStyle::from_theme(theme, variant);
            for (state, foreground, background) in [
                ("at rest", button.foreground, button.background),
                (
                    "focused",
                    button.focused_foreground,
                    button.focused_background,
                ),
                (
                    "hovered",
                    button.hovered_foreground,
                    button.hovered_background,
                ),
            ] {
                pairs.push((format!("{name} label {state}"), foreground, background));
            }
        }
        for (label, foreground, background) in [
            (
                "selected tab",
                tabs.selected_foreground,
                tabs.selected_background,
            ),
            (
                "selected tab focused",
                tabs.selected_focused_foreground,
                tabs.selected_focused_background,
            ),
            (
                "selected tab hovered",
                tabs.selected_hovered_foreground,
                tabs.selected_hovered_background,
            ),
            ("tab", tabs.foreground, tabs.background),
            (
                "tab focused",
                tabs.focused_foreground,
                tabs.focused_background,
            ),
            (
                "tab hovered",
                tabs.hovered_foreground,
                tabs.hovered_background,
            ),
        ] {
            pairs.push((format!("{label} label"), foreground, background));
        }
        pairs
    }

    /// A toast's severity color is its frame and its one-character icon, not its
    /// text — the title and body are covered as text above. Both are graphical,
    /// so WCAG 1.4.11's 3:1 is the floor, measured on the surface the icon sits
    /// on.
    ///
    /// Nord's `nord11` is the exception, and a palette-level one: it is 3.05:1
    /// on `nord0` itself, so *any* lifted surface puts it under 3:1, and it is
    /// the only red Nord ships.
    fn severity_accents(theme: &Theme) -> Vec<(&'static str, Color, Color)> {
        let toast = ToasterStyle::from_theme(theme);
        let mut accents = vec![
            ("success", toast.success, toast.background),
            ("info", toast.info, toast.background),
            ("warning", toast.warning, toast.background),
            ("loading", toast.loading, toast.background),
        ];
        if theme.name != "Nord" {
            accents.push(("error", toast.error, toast.background));
        }
        accents
    }

    #[test]
    fn preset_toast_severity_accents_meet_non_text_contrast_on_the_toast() {
        for theme in measurable_presets() {
            for (severity, accent, background) in severity_accents(theme) {
                let ratio = contrast(accent, background);
                assert!(
                    ratio >= 3.0,
                    "{}: the {severity} accent is {ratio:.2}:1 on a toast, below the 3:1 floor",
                    theme.name
                );
            }
        }
    }

    /// A control's three fills have to be three visibly different fills, or
    /// focus and hover have no answer. This is the button counterpart of the
    /// well ladder's step check, and it is what caps
    /// [`FOCUS_DARKEN`](crate::color::FOCUS_DARKEN) and its siblings from
    /// below while the label contrast caps them from above.
    #[test]
    fn preset_button_fills_step_visibly_between_states() {
        for theme in measurable_presets() {
            for variant in [
                ButtonVariant::Default,
                ButtonVariant::Secondary,
                ButtonVariant::Destructive,
            ] {
                let button = ButtonStyle::from_theme(theme, variant);
                for (step, from, to) in [
                    ("focus", button.background, button.focused_background),
                    (
                        "hover",
                        button.focused_background,
                        button.hovered_background,
                    ),
                ] {
                    let ratio = contrast(from, to);
                    assert!(
                        ratio >= 1.05,
                        "{} {variant:?} button: the {step} step is {ratio:.3}:1, an invisible change",
                        theme.name
                    );
                }
            }
        }
    }

    /// A disabled well has to look disabled, which needs both halves to move: a
    /// fill alone reads as "unfocused", and text alone leaves a well that still
    /// invites a click. Both dim toward the background, and the floors here are
    /// the same 1.05:1 step the ladder uses — enough to see.
    ///
    /// The pair they form is held to 2:1 rather than the text floor. Disabled
    /// text is exempt from WCAG 1.4.3 and is supposed to recede; what it may
    /// not do is disappear.
    #[test]
    fn preset_disabled_wells_read_as_disabled() {
        for theme in measurable_presets() {
            let style = ListStyle::from_theme(theme);
            for (half, enabled, disabled) in [
                ("fill", style.background, style.disabled_background),
                ("text", style.foreground, style.disabled_foreground),
            ] {
                let ratio = contrast(enabled, disabled);
                assert!(
                    ratio >= 1.05,
                    "{}: the disabled {half} is {ratio:.3}:1 from the enabled one, no change",
                    theme.name
                );
                assert!(
                    contrast(disabled, theme.background) < contrast(enabled, theme.background),
                    "{}: the disabled {half} moved away from the background, not toward it",
                    theme.name
                );
            }
            let legibility = contrast(style.disabled_foreground, style.disabled_background);
            assert!(
                legibility >= 2.0,
                "{}: disabled rows are {legibility:.2}:1, past receding into gone",
                theme.name
            );
        }
    }

    /// Text is the thing a palette is allowed to get wrong quietly: a fill that
    /// is one tone off still looks deliberate, while text one tone off is just
    /// hard to read. So every pair a component paints is measured, not only the
    /// ones on the background.
    #[test]
    fn preset_text_stays_readable_on_every_fill_it_is_painted_on() {
        for theme in measurable_presets() {
            for (pair, foreground, background) in text_pairs(theme) {
                let floor = contrast_floor(theme, &pair);
                let ratio = contrast(foreground, background);
                assert!(
                    ratio >= floor,
                    "{}: {pair} is {ratio:.2}:1, below the {floor}:1 floor",
                    theme.name
                );
            }
        }
    }

    /// The preset the contrast floors cannot reach, checked for the one thing
    /// they would have caught: a derived state that derives to nothing. Every
    /// state here is a blend, and a blend against [`Color::Reset`] returns its
    /// input, so each one has to be shown to have actually moved.
    #[test]
    fn terminal_theme_has_derived_component_surfaces() {
        let theme = Theme::terminal();
        let style = ListStyle::from_theme(&theme);

        assert_eq!(theme.background, Color::Reset);
        assert!(matches!(theme.surface, Color::Rgb(_, _, _)));
        assert!(matches!(theme.field, Color::Rgb(_, _, _)));
        assert_ne!(lighten(theme.field, FIELD_FOCUS_LIGHTEN), theme.field);
        assert_ne!(
            style.disabled_background, style.background,
            "a disabled terminal well is the same fill as an enabled one"
        );
        assert_ne!(
            style.disabled_foreground, style.foreground,
            "disabled terminal rows are the same text color as enabled ones"
        );
    }

    /// A named accent survives only until something derives from it: focus
    /// resolves it through the VGA approximation, which replaces the terminal's
    /// own shade with a fixed one. A neutral primary makes that harmless — the
    /// derived states stay gray — so the preset's primary has to have no hue to
    /// lose.
    ///
    /// Being neutral is not enough on its own, which is why the second half is
    /// here: a list draws its selected marker in `primary` and its unselected
    /// one in `muted_foreground`, so the two neutrals the terminal palette
    /// offers cannot be the same one.
    #[test]
    fn terminal_primary_is_neutral_and_distinct_from_muted_text() {
        let theme = Theme::terminal();

        for color in [theme.primary, darken(theme.primary, FOCUS_DARKEN)] {
            let (r, g, b) = resolve_rgb(color).expect("the terminal primary resolves to channels");
            assert!(
                r == g && g == b,
                "terminal primary derived {color:?}, which carries a hue",
            );
        }
        assert_ne!(
            theme.primary, theme.muted_foreground,
            "the terminal primary and its muted text are the same color, so a \
             selected marker cannot be told from an unselected one"
        );
    }

    /// `destructive` is the one role in this preset that keeps a hue, because
    /// the hue *is* the role — a red button is red on every palette. The cost
    /// is that its derived fills resolve through the VGA table before they
    /// darken, so focus lands on a fixed `#f00000` and hover on `#e00000`
    /// whatever red the terminal draws at rest.
    ///
    /// What can still be checked is which side of that red the label sits on.
    /// Black holds 4.17:1 at its worst against the approximation and white
    /// 3.998:1, while the same label on `Color::Red` — the terminal's *dark*
    /// red — collapses to 1.70:1. The floor is 4:1 rather than 4.5:1 because
    /// the table is a stand-in for a palette this preset cannot read: it says
    /// the choice is the right side of the red, not that any user's terminal
    /// meets a ratio. The margin it turns on is thousandths, so the failure
    /// prints thousandths.
    #[test]
    fn terminal_destructive_label_is_on_the_readable_side_of_its_red() {
        let theme = Theme::terminal();
        let button = ButtonStyle::from_theme(&theme, ButtonVariant::Destructive);

        for (state, fill) in [
            ("at rest", button.background),
            ("focused", button.focused_background),
            ("hovered", button.hovered_background),
        ] {
            let (label, fill) = (resolved(theme.destructive_foreground), resolved(fill));
            let ratio = contrast(label, fill);
            assert!(
                ratio >= 4.0,
                "the terminal destructive label is {ratio:.3}:1 {state}, below the 4:1 the \
                 fallback table can promise"
            );
        }
    }

    /// A named color as the channels the crate would derive from it. Named
    /// colors are the terminal preset's whole point, and they are exactly what
    /// [`luminance`] cannot read.
    fn resolved(color: Color) -> Color {
        let (r, g, b) = resolve_rgb(color).expect("a named or rgb color resolves to channels");
        Color::Rgb(r, g, b)
    }
}
