//! The base palette every component derives its colors from, and the one place
//! a declared style override is resolved against it.
//!
//! A [`Theme`] stores base colors only. Each component owns a style struct whose
//! `from_theme` derives every slot it paints from those bases (with the shifts in
//! [`color`](crate::color)), and a `style(...)` builder that replaces the whole
//! derivation with a closure. [`resolve_style`] is the fork between the two,
//! shared so a theme switch cannot reach some components and miss others.

use ratatui::style::Color;

use crate::color::{
    DISABLED_DIM, FIELD_FOCUS_SHIFT, FIELD_HOVER_SHIFT, FOCUS_SHIFT, HOVER_SHIFT, ROW_FOCUS_SHIFT,
    away_from, blendable, contrast, dim, luminance, nearest_to, resolve_rgb,
};
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
/// in a direction read off `background` rather than written into the component
/// — a well climbs away from the screen, a filled control deepens toward it,
/// and on a light theme both of those are the other way round in absolute terms
/// — and a disabled control dims toward the layer behind it: a button toward
/// the `surface`, a well toward the `background` it already sits close to.
///
/// # The well ladder
///
/// `field` is the bottom rung of a four-level ladder every well is painted
/// from, and it sits *close* to `background` — one tone off it, not a slab
/// that competes with the screen. A component lifts from there: focused is one
/// step further from the background than at rest, focused-and-hovered further
/// still, and the cursor row inside a list is the furthest. The lift amounts
/// live in [`crate::color`]
/// ([`crate::color::FIELD_FOCUS_SHIFT`],
/// [`crate::color::FIELD_HOVER_SHIFT`]), and the direction
/// comes from [`crate::color::away_from`], so a palette that gets
/// `background` and `field` right gets the whole ladder right on either
/// polarity.
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
    /// [`resolve_rgb`]'s fixed VGA approximation.
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

    /// A theme solved around a background and a foreground someone else chose —
    /// the terminal's own, a browser's computed style, a user setting.
    ///
    /// The presets answer "what does Gruvbox look like". This answers a
    /// different question: given only *these two colors*, what palette keeps
    /// every relationship the crate's contrast floors demand? Everything is
    /// solved rather than picked — the well against the 1.10..=1.35 window, the
    /// surface into the gap below it, text and lines against their floors — in
    /// the same integer blend arithmetic the components paint with, so a color
    /// that solves to a floor here measures to the same floor there.
    ///
    /// Polarity is not a parameter. A light background derives a light theme:
    /// wells sit *darker* than the screen and deepen from there, filled
    /// controls brighten as they take focus, and text darkens. Nothing in the
    /// call changes; [`crate::color::away_from`] reads it off the
    /// background.
    ///
    /// `palette16` is the terminal's own ANSI colors, if the caller has them,
    /// used for the three roles a neutral cannot fill — error, warning, and the
    /// signature accent. They are repaired toward the readable end until they
    /// clear their floors, because a palette's red is chosen to be *seen as
    /// text on the background*, not to be a fill with a label on it. Without
    /// them the same three roles come from fixed seeds and take the same
    /// repair.
    ///
    /// # Total
    ///
    /// A derived theme is named `"Adaptive"`, which is not one of the names the
    /// tests grant a contrast exception, so the solvers have to actually land —
    /// and no input has been found that stops them. That is a swept property,
    /// not a proof: the crate's own sweep derives a theme from every preset's
    /// pair, the four polarity corners, the low-contrast pairs real terminals
    /// report, the pairs that once broke an invariant, an RGB lattice, and a
    /// seeded walk between its points — each with and without a terminal
    /// palette to take hues from — and holds every one of them to every floor
    /// the presets are held to.
    ///
    /// Totality is not free, and where it cannot be had by solving a color it
    /// is had by refusing the background: a screen too close to the middle of
    /// the ramp is deepened toward its own end until the rest becomes solvable
    /// (see below). Colors with no channels to read ([`Color::Reset`],
    /// `Indexed`) are replaced by [`default_dark`](Self::default_dark)'s pair
    /// before anything is solved, which is the only sense in which the function
    /// can refuse an input outright.
    ///
    /// # A low-contrast pair is not honored verbatim
    ///
    /// A terminal may report a pair that cannot carry a UI: Solarized Light's
    /// real background and foreground are 4.13:1, and a well's top rung eats
    /// that to 2.26:1. The foreground is therefore pushed away from the
    /// background until it holds 4.5:1 on every fill it will be painted on, on
    /// whichever side can get there. Body text ends up further from what the
    /// terminal reported than anything else in the theme; the alternative is a
    /// list whose rows cannot be read.
    ///
    /// ```
    /// use ratatui::style::Color;
    /// use ratcn::Theme;
    ///
    /// fn luminance(color: Color) -> f64 {
    ///     ratcn::color::luminance(color).expect("a derived theme is rgb")
    /// }
    ///
    /// // A light terminal: the well is darker than the screen behind it.
    /// let theme = Theme::adaptive(Color::Rgb(253, 246, 227), Color::Rgb(101, 123, 131), None);
    /// assert!(luminance(theme.field) < luminance(theme.background));
    ///
    /// // A dark one: the same call, the other way round.
    /// let theme = Theme::adaptive(Color::Rgb(10, 10, 10), Color::Rgb(250, 250, 250), None);
    /// assert!(luminance(theme.field) > luminance(theme.background));
    /// ```
    #[must_use]
    pub fn adaptive(background: Color, foreground: Color, palette16: Option<&[Color; 16]>) -> Self {
        let fallback = Self::default_dark();
        let queried_background = blendable(background, fallback.background);
        let queried_text = blendable(foreground, fallback.foreground);
        let away = away_from(queried_background);
        let near = nearest_to(queried_background);

        // A background near the middle of the ramp cannot carry a UI: the well
        // ladder spends contrast climbing away from it, and text has to clear
        // both the background and the rung furthest from it. On `#0040ff` those
        // two demands point opposite ways and neither end can serve both — the
        // ladder leaves 3.31:1 above it and only 3.17:1 below. So the
        // background is deepened toward its own end until the pair becomes
        // solvable, keeping the hue and the polarity the terminal reported
        // while buying back the room the ladder needs. A background that was
        // already workable is not moved at all.
        let (layers, hues) = (0..=100)
            .map(|deepen| dim(queried_background, near, deepen))
            .find_map(|background| {
                let layers = Layers::solve(background, queried_text, away)?;
                // The hued roles are solved inside the walk, not after it: a
                // mid-gray screen leaves a delete button no shade of red that
                // holds the boundary floor in all three of its states, and the
                // answer to that is a deeper screen, not a worse red.
                let hues = Hues::solve(palette16, &layers, away, near)?;
                Some((layers, hues))
            })
            // Unreachable in practice — the walk ends at an extreme, and an
            // extreme always solves — but written as a value rather than an
            // unwrap so the function has no panic to document.
            .unwrap_or((
                Layers {
                    background: fallback.background,
                    surface: fallback.surface,
                    field: fallback.field,
                    secondary: fallback.secondary,
                    primary: fallback.primary,
                    ring: fallback.ring,
                    foreground: fallback.foreground,
                    muted_foreground: fallback.muted_foreground,
                },
                Hues {
                    accent: fallback.accent,
                    destructive: fallback.destructive,
                    warning: fallback.warning,
                },
            ));

        let labels = [layers.background, layers.foreground];

        let mut theme = fallback;
        theme.name = "Adaptive";
        theme.foreground = layers.foreground;
        theme.muted_foreground = layers.muted_foreground;
        theme.background = layers.background;
        theme.surface = layers.surface;
        theme.field = layers.field;
        theme.primary = layers.primary;
        theme.primary_foreground = best_label(layers.primary, near, &labels);
        theme.secondary = layers.secondary;
        theme.secondary_foreground = best_label(layers.secondary, away, &labels);
        theme.accent = hues.accent;
        theme.destructive = hues.destructive;
        theme.destructive_foreground = best_label(hues.destructive, near, &labels);
        theme.warning = hues.warning;
        theme.border = first_blend(layers.background, away, |line| {
            contrast_or_zero(line, layers.background) >= LINE_FLOOR
        });
        theme.ring = layers.ring;
        theme.cursor = layers.foreground;
        theme
    }

    /// Every built-in theme, in a stable order — for a theme picker or a
    /// cycle-themes hotkey.
    ///
    /// The slice is returned rather than a fixed-size array so that adding a
    /// preset does not break callers: index it, iterate it, or take its
    /// `len()`, but do not depend on the count.
    ///
    /// [`adaptive`](Self::adaptive) is deliberately absent: it is solved per
    /// terminal rather than authored, so there is no one theme to list.
    #[must_use]
    pub const fn presets() -> &'static [Self] {
        &PRESETS
    }
}

// The floors a solved theme is built to. They are the same numbers the tests
// hold every theme to; naming them here is what lets the derivation aim at a
// floor instead of at a color someone liked.

/// How close a well may sit to the background before it disappears into it,
/// and how far before it becomes a slab the text budget cannot afford. Every
/// theme in the crate is held to this window, and a solved one aims at
/// [`WELL_TARGET`] inside it.
const WELL_MIN: f64 = 1.10;
/// The far side of the well window.
const WELL_MAX: f64 = 1.35;

/// Where a solved well sits inside the window: in the middle, so the surface
/// has room to split off below it and the rungs have room to climb above it.
/// At the bottom the split stops fitting; at the top the text budget runs out.
const WELL_TARGET: f64 = 1.20;

/// What a dialog needs against the screen: only enough to be a different
/// color, because a dialog also has a ring around it, pinned at [`LINE_FLOOR`].
const SURFACE_MIN: f64 = 1.03;

/// The smallest change that is a change. Below this a focus or hover state is
/// in the numbers but not on the screen.
const VISIBLE_STEP: f64 = 1.05;

/// How far muted text has to sit from body text to be a second register rather
/// than a rounding error.
///
/// Higher than [`VISIBLE_STEP`], because two fills a step apart are read as one
/// surface changing state while two *texts* a step apart are read as the same
/// text. It is set just under the tightest a preset gets — Nord has the least
/// room between its two Snow Storm tones, at 1.17:1.
const MUTED_STEP: f64 = 1.15;

/// WCAG 2.1 non-text contrast (1.4.11), what a border or a ring has to clear
/// against what it is drawn on.
const LINE_FLOOR: f64 = 3.0;

/// WCAG 2.1 normal text (1.4.3), what every label has to clear against every
/// fill it can land on.
const TEXT_FLOOR: f64 = 4.5;

/// What a disabled row keeps. WCAG exempts inactive controls, so this is not a
/// text floor — it is the line between receding and gone.
const DISABLED_LEGIBILITY: f64 = 2.0;

/// How far a neutral fill sits from the background. Near enough to the far end
/// that a label of the background's own tone reads on it, which is what makes
/// `primary` a neutral rather than a hue: a solved theme has no reason to
/// prefer one of the terminal's accents, and every reason to avoid one whose
/// derived states it cannot predict.
const NEUTRAL_FILL_SHIFT: u16 = 90;

/// How far a quiet fill (`secondary`) sits from the background — far enough to
/// read as a control, near enough to stay quiet next to `primary`.
const QUIET_FILL_SHIFT: u16 = 13;

/// The contrast between two colors, or 0.0 when one cannot be measured — a
/// value no floor accepts, so an unreadable color can never satisfy a solver.
fn contrast_or_zero(a: Color, b: Color) -> f64 {
    contrast(a, b).unwrap_or(0.0)
}

/// The first blend of `from` toward `toward` that satisfies `ok`, or the whole
/// way when nothing does.
///
/// Solving by walking the same integer percentages [`dim`] blends in is the
/// point: a color found here is a color the components can reach, rounded
/// exactly as they round it.
fn first_blend(from: Color, toward: Color, ok: impl Fn(Color) -> bool) -> Color {
    (0..=100)
        .map(|amount| dim(from, toward, amount))
        .find(|blend| ok(*blend))
        .unwrap_or_else(|| dim(from, toward, 100))
}

/// Every neutral a theme is built from, solved together because they constrain
/// each other.
///
/// The well is placed against the background, the rungs climb from the well,
/// and the text has to clear the furthest of them — while `primary` has to
/// carry a boundary floor of its own, since the focus ring is derived from it.
/// [`Layers::solve`] answering `None` is what tells the caller this background
/// cannot carry a theme, whichever of those demands is the one it cannot meet.
struct Layers {
    background: Color,
    surface: Color,
    field: Color,
    secondary: Color,
    primary: Color,
    ring: Color,
    foreground: Color,
    muted_foreground: Color,
}

impl Layers {
    fn solve(background: Color, queried_text: Color, away: Color) -> Option<Self> {
        let field = solve_well(background, away);
        let surface = solve_surface(background, field);
        let rungs = well_rungs(field, away);
        // A saturated background fails here rather than later: a channel
        // already at the ceiling cannot move, so `#0080ff` climbs its ladder in
        // steps of 1.04:1 — present in the arithmetic, invisible on screen.
        // Refusing the background is what sends the caller back to deepen it.
        if !(WELL_MIN..=WELL_MAX).contains(&contrast_or_zero(field, background))
            || !climbs_visibly(background, &[field, rungs[0], rungs[1], rungs[2]])
        {
            return None;
        }

        // Text lands on more than the well: the screen itself and the raised
        // surface carry it too, and the well's three rungs are the fills that
        // sit furthest from the background.
        //
        // A ghost button's focus and hover fills are not listed. They climb
        // from the quiet fill by [`FOCUS_SHIFT`] and [`HOVER_SHIFT`], which
        // stays inside the headroom body text is already pushed past for muted
        // text to exist below it — the derived sweep is what holds that.
        let secondary = dim(background, away, QUIET_FILL_SHIFT);
        let backdrops = [background, surface, rungs[0], rungs[1], rungs[2]];

        // One predicate answers for all the text in the theme. Disabled rows
        // are in it because they are derived from muted text and dimmed again
        // from there — the pair that runs out first on a saturated background,
        // where there is room to dim long past the point the disabled twin
        // survives.
        let disabled_well = dim(field, background, DISABLED_DIM);
        let measured = Backdrops::new(&backdrops);
        let readable = |text: Color| {
            measured.worst(text) >= TEXT_FLOOR
                && contrast_or_zero(dim(text, background, DISABLED_DIM), disabled_well)
                    >= DISABLED_LEGIBILITY
        };
        // Body text is pushed away from the background until it holds, and far
        // enough past that for muted text to still exist: taking the least push
        // that clears the floor leaves body text sitting exactly on it, with no
        // room underneath, and every muted role — list rows at rest,
        // placeholders, dialog descriptions, toast bodies — collapses onto it.
        // Coarse to fine again, for the same reason: the least push that works
        // is what keeps body text closest to what the terminal reported, both
        // ends have to be tried, and asking every amount of both is the
        // expensive half of solving a theme.
        let solved_text = |amount: u16| {
            [Color::White, Color::Black]
                .into_iter()
                .map(|end| dim(queried_text, end, amount))
                .filter(|text| readable(*text))
                .find_map(|text| Some((text, recedes_from(text, background, &readable)?)))
        };
        let coarse = (0..=20)
            .map(|step| step * 5)
            .find(|amount| solved_text(*amount).is_some())?;
        let (foreground, muted_foreground) =
            (coarse.saturating_sub(4)..=coarse).find_map(solved_text)?;

        // `primary` is placed, not searched for: the background shifted
        // [`NEUTRAL_FILL_SHIFT`] toward the far end, which is what makes it a
        // neutral rather than one of the terminal's accents.
        //
        // What holds its floors is the deepening walk this solver runs inside.
        // The walk's other gate is [`Hues::solve`], where a red has to hold
        // [`LINE_FLOOR`] on the surface across all three of its fill states,
        // and every background near the middle of the ramp fails that gate
        // first — so by the time a background is accepted at all, it is deep
        // enough that the placed fill has room for a ring and a label. The `?`
        // below is defence-in-depth: it puts ring-solvability into the same
        // solvability predicate, so a background that placed a primary with no
        // workable ring would be deepened rather than returned.
        let primary = dim(background, away, NEUTRAL_FILL_SHIFT);
        let ring = ring_of(primary, background, surface)?;

        Some(Self {
            background,
            surface,
            field,
            secondary,
            primary,
            ring,
            foreground,
            muted_foreground,
        })
    }
}

/// Muted text for a given body text: the dimmest blend toward the background
/// that still reads, still recedes, and is still visibly a different color.
///
/// Contrast against the worst of several backdrops is not monotone in the
/// blend: `#00ff40` dips below the floor partway and comes back, so a single
/// walk stopping at the first failure returns a "muted" text more contrasty
/// than the body text it came from. The walk is therefore coarse to
/// fine — a pass in tens finds the dimmest decade that holds, and a fine pass
/// inside it takes amounts while they hold. Both passes can be stepped over by
/// a dip, so the answer may sit a few channel steps short of what checking all
/// hundred amounts would find (measured: 43 of 4000 backgrounds, median 3
/// steps). What it cannot be is invalid — every amount returned is one
/// `holds` accepted.
fn recedes_from(
    foreground: Color,
    background: Color,
    readable: &impl Fn(Color) -> bool,
) -> Option<Color> {
    let reach = contrast_or_zero(foreground, background);
    let holds = |muted: Color| {
        readable(muted)
            && contrast_or_zero(muted, background) < reach
            && contrast_or_zero(muted, foreground) >= MUTED_STEP
    };
    // Coarse first, then refine. The answer wanted is the dimmest blend that
    // holds, and this is asked once per candidate body text while a theme is
    // being solved — walking all hundred amounts every time made the sweep an
    // order of magnitude slower than the whole rest of the suite. A coarse pass
    // finds the window, a fine pass finds the edge inside it, and anything the
    // coarse pass steps over is a slightly less dim muted, never an invalid one.
    let coarse = (0..=10)
        .rev()
        .map(|step| step * 10)
        .find(|amount| holds(dim(foreground, background, *amount)))?;
    (coarse..coarse + 10)
        .map(|amount| dim(foreground, background, amount))
        .take_while(|muted| holds(*muted))
        .last()
        .or_else(|| Some(dim(foreground, background, coarse)))
}

/// A focus ring for a given `primary`: softened toward the background like a
/// preset's, but only as far as it can go while still reading as a boundary
/// against both the screen and the surface it frames.
///
/// `None` means no softening works, including none at all — which is a fact
/// about `primary`, and why `primary` is solved against this rather than placed
/// and hoped for.
fn ring_of(primary: Color, background: Color, surface: Color) -> Option<Color> {
    (0..=RING_DIM)
        .map(|softening| dim(primary, background, softening))
        .take_while(|ring| {
            contrast_or_zero(*ring, background) >= LINE_FLOOR
                && contrast_or_zero(*ring, surface) >= LINE_FLOOR
        })
        .last()
}

/// The well: the first tone away from the background that reaches
/// [`WELL_TARGET`]. The window's floor is 1.10 and this aims past it, because
/// the surface has to fit underneath.
fn solve_well(background: Color, away: Color) -> Color {
    first_blend(background, away, |field| {
        contrast_or_zero(field, background) >= WELL_TARGET
    })
}

/// The surface: the first tone between the background and the well that is
/// distinct from the background and still leaves the well distinct from it.
///
/// Blending toward the well rather than toward an extreme is what keeps it
/// *between* them on either polarity, which is the ordering the tests assert.
fn solve_surface(background: Color, field: Color) -> Color {
    first_blend(background, field, |surface| {
        contrast_or_zero(surface, background) >= SURFACE_MIN
            && contrast_or_zero(field, surface) >= WELL_MIN
    })
}

/// Whether each of these is a visible step further from the background than the
/// one before it — the ladder's own invariant, checked while it is being solved
/// rather than after it is painted.
fn climbs_visibly(background: Color, ladder: &[Color]) -> bool {
    let mut previous = background;
    for rung in ladder {
        if contrast_or_zero(*rung, previous) < VISIBLE_STEP {
            return false;
        }
        previous = *rung;
    }
    true
}

/// The three fills a list derives from a well, in the order it climbs them.
/// Solving against these rather than against the well is the difference between
/// text that reads at rest and text that reads on the cursor row.
fn well_rungs(field: Color, away: Color) -> [Color; 3] {
    let focused = dim(field, away, FIELD_FOCUS_SHIFT);
    [
        focused,
        dim(field, away, FIELD_HOVER_SHIFT),
        dim(focused, away, ROW_FOCUS_SHIFT),
    ]
}

/// The worst contrast `text` has against any of `backdrops`.
///
/// The backdrops' luminances are computed once and kept, because this is the
/// crate's innermost loop: solving a theme asks it for hundreds of candidate
/// texts against the same handful of fills, and each answer is otherwise three
/// `powf` calls per channel per side.
struct Backdrops {
    luminances: Vec<Option<f64>>,
}

impl Backdrops {
    fn new(backdrops: &[Color]) -> Self {
        Self {
            luminances: backdrops.iter().copied().map(luminance).collect(),
        }
    }

    fn worst(&self, text: Color) -> f64 {
        let Some(text) = luminance(text) else {
            return 0.0;
        };
        self.luminances
            .iter()
            .map(|backdrop| match backdrop {
                Some(backdrop) => (text.max(*backdrop) + 0.05) / (text.min(*backdrop) + 0.05),
                // The convention [`contrast_or_zero`] keeps: a backdrop with no
                // channels to measure scores a ratio no floor accepts, so an
                // unmeasurable color can never satisfy a solver. Every backdrop
                // a solved theme passes here is derived rgb, so this is the
                // arm that says what the crate would do rather than one it
                // reaches.
                None => 0.0,
            })
            .fold(f64::INFINITY, f64::min)
    }
}

/// The worst contrast `text` has against any of `backdrops`, for the callers
/// that ask once rather than in a loop — allocation-free, because the hue
/// repair asks it a hundred times per role.
fn worst_against(text: Color, backdrops: &[Color]) -> f64 {
    let Some(text) = luminance(text) else {
        return 0.0;
    };
    backdrops
        .iter()
        .filter_map(|backdrop| luminance(*backdrop))
        .map(|backdrop| (text.max(backdrop) + 0.05) / (text.min(backdrop) + 0.05))
        .fold(f64::INFINITY, f64::min)
}

/// The three fills a control paints: at rest, focused, and hovered.
fn fill_states(fill: Color, pressed: Color) -> [Color; 3] {
    [
        fill,
        dim(fill, pressed, FOCUS_SHIFT),
        dim(fill, pressed, HOVER_SHIFT),
    ]
}

/// Whether a fill's three states are three visibly different fills.
///
/// A fill can fail this by being too close to the end it is pressed toward, and
/// a saturated one fails it from further away than a neutral does: blending
/// `#ff0000` toward white moves two channels that barely carry luminance and
/// leaves the third at the ceiling, so a light theme's red steps 1.01:1 where
/// its neutrals step 1.29:1. The derivation therefore has to pick a hue that
/// can move, rather than trusting that any red will do.
fn steps_visibly(fill: Color, pressed: Color) -> bool {
    let states = fill_states(fill, pressed);
    contrast_or_zero(states[0], states[1]) >= 1.05 && contrast_or_zero(states[1], states[2]) >= 1.05
}

/// A label for a fill: the first candidate that holds [`TEXT_FLOOR`] across all
/// three of the control's fills, preferring the theme's own tones over the
/// extremes.
///
/// It satisfices rather than maximizes — the candidates are offered in the
/// order the palette would rather use, and the search stops at the first that
/// clears, so a label made of the theme's own background wins over a starker
/// one that measures higher. When nothing clears, the best of them is returned
/// and the caller's own floor check is what refuses the theme.
///
/// Every candidate is scored across the whole trajectory, because a label's
/// worst moment is not always at rest: the fill moves toward the screen's own
/// end as it gains focus, so a label on that side loses contrast exactly when
/// the control is being used.
fn best_label(fill: Color, pressed: Color, candidates: &[Color]) -> Color {
    let states = fill_states(fill, pressed);
    let measured = Backdrops::new(&states);
    let score = |label: Color| measured.worst(label);
    // The extremes are written as channels, not as `Color::White`/`Color::Black`:
    // those are named colors, measured here through the VGA table but painted
    // as whatever the terminal's ANSI 0 and 15 happen to be — `#073642` on a
    // Solarized palette. A label proven at 6.2:1 has to be the label that gets
    // drawn.
    let extremes = [Color::Rgb(255, 255, 255), Color::Rgb(0, 0, 0)];
    let mut best = candidates.first().copied().unwrap_or(extremes[0]);
    for candidate in candidates.iter().copied().chain(extremes) {
        if score(candidate) > score(best) {
            best = candidate;
        }
        if score(best) >= TEXT_FLOOR {
            break;
        }
    }
    best
}

/// The three roles a neutral cannot fill, solved from the terminal's palette
/// when there is one.
struct Hues {
    accent: Color,
    destructive: Color,
    warning: Color,
}

impl Hues {
    /// ANSI bright red, yellow, and magenta — the palette entries whose meaning
    /// is fixed by convention rather than by taste.
    ///
    /// The convention is not universal: Solarized repurposes the bright half,
    /// putting orange in slot 9 and a base gray in slot 11, so `warning`
    /// derives to a gray that clears every floor and stays distinct while
    /// reading as no warning at all. Trusting the slot may need a saturation
    /// sanity-check once the palette arrives over OSC 4.
    const RED: usize = 9;
    const YELLOW: usize = 11;
    const MAGENTA: usize = 13;

    fn solve(
        palette16: Option<&[Color; 16]>,
        layers: &Layers,
        away: Color,
        near: Color,
    ) -> Option<Self> {
        let from_palette = |index: usize, seed: Color| {
            palette16
                .and_then(|palette| resolve_rgb(palette[index]).map(|_| palette[index]))
                .unwrap_or(seed)
        };
        // A palette's own accents are chosen to be read as text on the
        // background, which is a weaker requirement than being a fill with a
        // label on it. Each is walked toward the readable end until it clears
        // the floors it has to clear, so an unusable hue becomes a usable one
        // of the same family rather than being replaced by a neutral.
        //
        // A hue that is also a fill has to hold the boundary floor in all three
        // of its states, not just at rest. Focus and hover walk a fill back
        // toward the screen, and on a light theme that is the direction the
        // page already is: every real light terminal palette measured put a
        // hovered delete button under 3:1 on its own surface when only the
        // resting state was solved for.
        let repair = |hue: Color, is_a_fill: bool, taken: &[Color]| {
            (0..=100)
                .map(|amount| dim(hue, away, amount))
                .find(|candidate| {
                    let candidate = *candidate;
                    if taken.contains(&candidate) {
                        return false;
                    }
                    let states = fill_states(candidate, near);
                    let shown = if is_a_fill { &states[..] } else { &states[..1] };
                    shown
                        .iter()
                        .all(|state| contrast_or_zero(*state, layers.surface) >= LINE_FLOOR)
                        && (!is_a_fill
                            || (steps_visibly(candidate, near)
                                && worst_against(
                                    best_label(candidate, near, &[layers.background]),
                                    &states,
                                ) >= TEXT_FLOOR))
                })
        };
        // Solved in turn, each refusing the answers already given: a palette
        // whose red slot holds a yellow repairs both roles to the same color
        // otherwise, and an error that looks like a warning is worse than
        // either being slightly off-hue.
        let destructive = repair(
            from_palette(Self::RED, Color::Rgb(255, 100, 103)),
            true,
            &[],
        )?;
        let warning = repair(
            from_palette(Self::YELLOW, Color::Rgb(250, 204, 21)),
            false,
            &[destructive],
        )?;
        let accent = repair(
            from_palette(Self::MAGENTA, Color::Rgb(139, 92, 246)),
            false,
            &[destructive, warning],
        )?;
        Some(Self {
            accent,
            destructive,
            warning,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ButtonStyle, ButtonVariant, DialogStyle, ListStyle, SelectStyle, TabsStyle, ToasterStyle,
    };

    /// The crate's own luminance, insisting on a color it can read. Every
    /// theme measured here paints in colors the derivation solved against the
    /// same function, so an unreadable one is a bug in the caller, not a case
    /// to tolerate.
    fn luminance(color: Color) -> f64 {
        crate::color::luminance(color).expect("a measured theme color has channels")
    }

    fn contrast(a: Color, b: Color) -> f64 {
        crate::color::contrast(a, b).expect("a measured theme pair has channels")
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
    fn check_lines(theme: &Theme) {
        for (role, line, backdrop) in [
            ("border", theme.border, theme.background),
            ("ring", theme.ring, theme.background),
            ("ring on the surface it frames", theme.ring, theme.surface),
        ] {
            let ratio = contrast(line, backdrop);
            assert!(
                ratio >= LINE_FLOOR,
                "{} {role} contrast is {ratio:.2}:1, below the {LINE_FLOOR}:1 floor",
                theme.name
            );
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

    /// The floors are the numbers they name.
    ///
    /// Every check above measures against a constant, which keeps the
    /// derivation and the tests from drifting apart — but it also means a
    /// weakened constant weakens both sides at once and nothing notices. These
    /// are the citations, written down where a mutation has to argue with them.
    #[test]
    fn the_floors_are_the_numbers_they_name() {
        // WCAG 2.1 SC 1.4.3, normal text.
        assert!((TEXT_FLOOR - 4.5).abs() < f64::EPSILON);
        // WCAG 2.1 SC 1.4.11, user interface components and graphical objects.
        assert!((LINE_FLOOR - 3.0).abs() < f64::EPSILON);
        // The crate's own: the well window, its aim, the dialog split, the
        // smallest visible change, and what a disabled row keeps.
        assert!((WELL_MIN - 1.10).abs() < f64::EPSILON);
        assert!((WELL_MAX - 1.35).abs() < f64::EPSILON);
        assert!((WELL_TARGET - 1.20).abs() < f64::EPSILON);
        assert!((SURFACE_MIN - 1.03).abs() < f64::EPSILON);
        assert!((VISIBLE_STEP - 1.05).abs() < f64::EPSILON);
        assert!((MUTED_STEP - 1.15).abs() < f64::EPSILON);
        assert!((DISABLED_LEGIBILITY - 2.0).abs() < f64::EPSILON);
    }

    /// Every invariant a theme has to hold, whatever produced it.
    ///
    /// The checks are written against one theme rather than looped over the
    /// presets so that a theme nobody authored can be held to the same bar. A
    /// preset is a claim about taste that has to survive measurement; a derived
    /// theme is only ever the measurement, so if these pass for every input the
    /// derivation is correct in the only sense the crate defines.
    fn check_every_invariant(theme: &Theme) {
        check_lines(theme);
        check_well_window(theme);
        check_surface_ordering(theme);
        check_well_ladder(theme);
        check_severity_accents(theme);
        check_fill_states_stay_on_the_page(theme);
        check_control_fills(theme);
        check_select_tracks_the_list(theme);
        check_muted_recedes(theme);
        check_accents_are_distinct(theme);
        check_cursor_is_visible(theme);
        check_disabled_wells(theme);
        check_text(theme);
    }

    #[test]
    fn every_preset_holds_every_invariant() {
        for theme in measurable_presets() {
            check_every_invariant(theme);
        }
    }

    /// A terminal palette of the 16 named colors, which resolve through the VGA
    /// table — the worst realistic input, since its red is `#ff0000` and its
    /// yellow `#ffff00`.
    fn vga_palette() -> [Color; 16] {
        [
            Color::Black,
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::Gray,
            Color::DarkGray,
            Color::LightRed,
            Color::LightGreen,
            Color::LightYellow,
            Color::LightBlue,
            Color::LightMagenta,
            Color::LightCyan,
            Color::White,
        ]
    }

    /// A palette with one hue in all three slots the derivation reads.
    ///
    /// Terminals really do ship these — monochrome and near-monochrome themes
    /// put the same tone in most of the sixteen — and without one the roles
    /// repair to the same color and nothing says an error may not look exactly
    /// like a warning.
    fn monochrome_palette() -> [Color; 16] {
        [Color::Rgb(200, 180, 40); 16]
    }

    /// A palette in the register terminals actually ship: muted, mid-luminance
    /// hues, which are harder than the VGA ones because they start closer to
    /// the floors they have to clear.
    fn muted_palette() -> [Color; 16] {
        let hue = |r, g, b| Color::Rgb(r, g, b);
        [
            hue(40, 42, 54),
            hue(191, 97, 106),
            hue(163, 190, 140),
            hue(235, 203, 139),
            hue(94, 129, 172),
            hue(180, 142, 173),
            hue(136, 192, 208),
            hue(216, 222, 233),
            hue(76, 86, 106),
            hue(191, 97, 106),
            hue(163, 190, 140),
            hue(235, 203, 139),
            hue(129, 161, 193),
            hue(180, 142, 173),
            hue(143, 188, 187),
            hue(236, 239, 244),
        ]
    }

    /// The (background, foreground) pairs the derivation is swept over.
    ///
    /// The named cases are the ones with a story: every preset's own pair, the
    /// four corners of the polarity square, Solarized Light exactly as a
    /// terminal reports it, and pairs no palette should be able to impose — a
    /// foreground equal to its background, two grays a hair apart, and colors
    /// with no channels at all. The lattice behind them is there to catch what
    /// nobody thought to name.
    fn derivation_grid() -> Vec<(Color, Color)> {
        let mut cases: Vec<(Color, Color)> = Theme::presets()
            .iter()
            .map(|theme| (theme.background, theme.foreground))
            .collect();
        cases.extend([
            (Color::Rgb(0, 0, 0), Color::Rgb(255, 255, 255)),
            (Color::Rgb(255, 255, 255), Color::Rgb(0, 0, 0)),
            (Color::Rgb(0, 0, 0), Color::Rgb(0, 0, 0)),
            (Color::Rgb(255, 255, 255), Color::Rgb(255, 255, 255)),
            // Solarized Light's real pair, 4.13:1 — the case that decided the
            // low-contrast policy.
            (Color::Rgb(253, 246, 227), Color::Rgb(101, 123, 131)),
            // Solarized Dark's real pair, 4.75:1.
            (Color::Rgb(0, 43, 54), Color::Rgb(131, 148, 150)),
            (Color::Rgb(128, 128, 128), Color::Rgb(138, 138, 138)),
            (Color::Rgb(48, 48, 48), Color::Rgb(48, 48, 48)),
            (Color::Rgb(118, 118, 118), Color::Rgb(120, 120, 120)),
            (Color::Reset, Color::Reset),
            (Color::Indexed(4), Color::Indexed(7)),
        ]);
        // The pairs that were found violating an invariant before the solvers
        // learned to answer for them, kept as named cases so a regression is
        // reported by name rather than by lattice coordinate.
        cases.extend([
            (Color::Rgb(177, 179, 15), Color::Rgb(0, 0, 0)),
            (Color::Rgb(55, 42, 65), Color::Rgb(255, 255, 255)),
            (Color::Rgb(60, 58, 77), Color::Rgb(255, 255, 255)),
            (Color::Rgb(0, 255, 64), Color::Rgb(0, 0, 0)),
            (Color::Rgb(0, 64, 255), Color::Rgb(255, 191, 0)),
            (Color::Rgb(0, 128, 255), Color::Rgb(255, 127, 0)),
            (Color::Rgb(46, 52, 64), Color::Rgb(236, 239, 244)),
        ]);
        let steps = [0_u8, 64, 128, 192, 255];
        for red in steps {
            for green in steps {
                for blue in steps {
                    let background = Color::Rgb(red, green, blue);
                    cases.push((background, Color::Rgb(255 - red, 255 - green, 255 - blue)));
                    cases.push((background, Color::Rgb(128, 128, 128)));
                }
            }
        }
        // A seeded walk over the space between the lattice points. The seed is
        // a literal so the suite runs the same pairs on every machine and every
        // day: a sweep that changed what it covered between runs would turn a
        // regression into a coin flip.
        //
        // The walk is short because its job is discovery, and what it has
        // discovered is already above it under its own name. Every pair it
        // ever failed on is a named case; the walk is what finds the next one,
        // which costs the suite little when it is this long and would cost it
        // most of its runtime if it were the sweep.
        let mut seed = 0x2026_0819_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            u8::try_from(seed % 256).unwrap_or(0)
        };
        for _ in 0..200 {
            cases.push((
                Color::Rgb(next(), next(), next()),
                Color::Rgb(next(), next(), next()),
            ));
        }
        cases
    }

    /// The screen stays the caller's, whatever the solvers had to do to make it
    /// workable.
    ///
    /// Passing the floors is not enough on its own: a derivation that gave up
    /// and returned the crate's own background would pass every one of them
    /// while throwing away the color the terminal actually reported. So the
    /// derived background has to be the queried one, or the queried one
    /// deepened toward its own end.
    ///
    /// The expected deepening is spelled out here with its own literal
    /// comparison rather than by calling [`nearest_to`]. Sharing that function
    /// with the derivation would make this test agree with whatever the
    /// derivation believes: moving the light/dark threshold off 0.5 would move
    /// both sides together and prove nothing.
    fn check_background_is_the_one_asked_for(theme: &Theme, queried: Color) {
        let Some(queried) = resolve_rgb(queried).map(|(r, g, b)| Color::Rgb(r, g, b)) else {
            // A color with no channels is the one input replaced outright,
            // because there is nothing to deepen.
            assert_eq!(
                theme.background,
                Theme::default_dark().background,
                "{}: an unreadable background derived something other than the crate's own",
                theme.name
            );
            return;
        };
        let own_end = if luminance(queried) > 0.5 {
            Color::Rgb(255, 255, 255)
        } else {
            Color::Rgb(0, 0, 0)
        };
        let deepened = (0..=100)
            .map(|amount| dim(queried, own_end, amount))
            .any(|candidate| candidate == theme.background);
        assert!(
            deepened,
            "{}: derived {:?} from a queried {queried:?}, which is neither it nor a deepening \
             of it toward {own_end:?}",
            theme.name, theme.background
        );
        assert_eq!(
            luminance(theme.background) > 0.5,
            luminance(queried) > 0.5,
            "{}: derived a {} background from a {} one",
            theme.name,
            if luminance(theme.background) > 0.5 {
                "light"
            } else {
                "dark"
            },
            if luminance(queried) > 0.5 {
                "light"
            } else {
                "dark"
            },
        );
    }

    /// A solved theme is made of channels, not names.
    ///
    /// Every floor above is measured through the VGA table, which is a
    /// stand-in: `Color::White` measures as `#ffffff` and paints as whatever
    /// the terminal's ANSI 15 is — `#fdf6e3` on a Solarized palette. A named
    /// color anywhere in a derived theme means the ratio that was proven is not
    /// the ratio on the screen, which is the one thing this whole derivation
    /// exists to avoid.
    fn check_theme_is_written_in_channels(theme: &Theme) {
        for (role, color) in [
            ("foreground", theme.foreground),
            ("muted_foreground", theme.muted_foreground),
            ("background", theme.background),
            ("surface", theme.surface),
            ("field", theme.field),
            ("primary", theme.primary),
            ("primary_foreground", theme.primary_foreground),
            ("secondary", theme.secondary),
            ("secondary_foreground", theme.secondary_foreground),
            ("accent", theme.accent),
            ("destructive", theme.destructive),
            ("destructive_foreground", theme.destructive_foreground),
            ("warning", theme.warning),
            ("border", theme.border),
            ("ring", theme.ring),
            ("cursor", theme.cursor),
        ] {
            assert!(
                matches!(color, Color::Rgb(_, _, _)),
                "{}: {role} is {color:?}, a name the terminal owns rather than the channels \
                 that were measured",
                theme.name
            );
        }
    }

    /// A hued role stays the palette's hue, walked toward one end.
    ///
    /// Without this, which palette slot feeds which role is unpinned: reading
    /// the red from the yellow's index produces a theme that passes every
    /// contrast floor and calls a yellow "destructive". The ray is the same
    /// shape the background check uses — the derivation is only allowed to walk
    /// a reported color toward an extreme, never to substitute one.
    fn check_hues_come_from_the_palette(theme: &Theme, palette: &[Color; 16], away: Color) {
        for (role, derived, slot) in [
            ("destructive", theme.destructive, 9),
            ("warning", theme.warning, 11),
            ("accent", theme.accent, 13),
        ] {
            let from_slot = (0..=100)
                .map(|amount| dim(palette[slot], away, amount))
                .any(|candidate| candidate == derived);
            assert!(
                from_slot,
                "{}: {role} is {derived:?}, which is not palette slot {slot} ({:?}) walked \
                 toward {away:?}",
                theme.name, palette[slot]
            );
        }
    }

    /// The derivation is total: every pair, with or without a palette to take
    /// hues from, has to produce a theme that passes what the presets pass.
    ///
    /// Each derived theme is renamed after the pair that produced it, which
    /// does two things: a failure names the input, and the name stops matching
    /// the exceptions `contrast_floor` grants three presets — a solved theme is
    /// held to 4.5:1 everywhere, because unlike a palette it can always be
    /// solved further.
    #[test]
    fn every_derived_theme_holds_every_invariant() {
        let (vga, muted, monochrome) = (vga_palette(), muted_palette(), monochrome_palette());
        for (background, foreground) in derivation_grid() {
            for (label, palette) in [
                ("no palette", None),
                ("vga palette", Some(&vga)),
                ("muted palette", Some(&muted)),
                ("monochrome palette", Some(&monochrome)),
            ] {
                let mut theme = Theme::adaptive(background, foreground, palette);
                theme.name = Box::leak(
                    format!("adaptive({foreground:?} on {background:?}, {label})").into_boxed_str(),
                );
                check_every_invariant(&theme);
                check_background_is_the_one_asked_for(&theme, background);
                check_theme_is_written_in_channels(&theme);
                if let Some(palette) = palette {
                    check_hues_come_from_the_palette(&theme, palette, away_from(theme.background));
                }
            }
        }
    }

    /// A pair that already works is honored verbatim.
    ///
    /// The low-contrast policy is a repair, and a repair that fires when
    /// nothing is broken is a theme that ignores what the terminal reported.
    /// The text guard is written to take the *least* push that clears the
    /// floors, so on a workable pair the least push is none — these five pairs
    /// are workable, and each has to come back out as it went in.
    #[test]
    fn a_workable_foreground_is_returned_unchanged() {
        for (background, foreground) in [
            (Color::Rgb(10, 10, 10), Color::Rgb(250, 250, 250)),
            (Color::Rgb(0, 0, 0), Color::Rgb(255, 255, 255)),
            (Color::Rgb(255, 255, 255), Color::Rgb(0, 0, 0)),
            (Color::Rgb(0, 43, 54), Color::Rgb(238, 232, 213)),
            (Color::Rgb(17, 17, 27), Color::Rgb(205, 214, 244)),
        ] {
            let derived = Theme::adaptive(background, foreground, None).foreground;
            assert_eq!(
                derived, foreground,
                "adaptive({foreground:?} on {background:?}) moved a foreground that already \
                 worked, to {derived:?}"
            );
        }
    }

    /// A well at rest must read as a distinct region without competing with the
    /// screen behind it. `default_dark` — the reference the other presets are
    /// tuned against — sits at 1.20:1, and every rung above it spends contrast
    /// the text still needs, so the window is narrow on both sides: below 1.10
    /// the well disappears, above 1.35 it becomes a slab and the ladder on top
    /// of it runs out of room.
    fn check_well_window(theme: &Theme) {
        let ratio = contrast(theme.field, theme.background);
        assert!(
            (WELL_MIN..=WELL_MAX).contains(&ratio),
            "{} field is {ratio:.2}:1 on its background, outside the {WELL_MIN}..={WELL_MAX} window",
            theme.name
        );
    }

    /// A raised surface is read against a backdrop the modal layer has already
    /// blended toward `background`, so it has to differ from `background`
    /// itself — a dialog painted in the background color sinks into the very
    /// thing it floats over. It also has to stay *under* the well: a list
    /// declared inside a dialog is lifted out of the dialog exactly as it is
    /// lifted out of the screen, and a surface above the well would invert
    /// that and leave the list looking sunken into the dialog.
    fn check_surface_ordering(theme: &Theme) {
        // The two steps carry different floors because they are found
        // differently. A dialog has a ring frame around it, pinned at 3:1
        // above, so its fill only has to be a different color; a well has
        // no frame at all, so its fill is the whole separation.
        for (lower, upper, floor, pair) in [
            (
                theme.background,
                theme.surface,
                SURFACE_MIN,
                "surface over background",
            ),
            (theme.surface, theme.field, WELL_MIN, "well over surface"),
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
    fn check_well_ladder(theme: &Theme) {
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
                step >= VISIBLE_STEP,
                "{}: {rung} is only {step:.3}:1 from {}, an invisible step",
                theme.name,
                previous.0
            );
            previous = (rung, color);
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
            _ => TEXT_FLOOR,
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

    /// A fill that is also a severity color has to stay on the page in every
    /// state, not just at rest.
    ///
    /// Focus and hover walk a fill toward the screen's own end, and on a light
    /// theme that is where the page already is: every real light terminal
    /// palette measured — GitHub Light, Solarized Light, Gruvbox Light,
    /// Catppuccin Latte, One Half Light, plain white — put a hovered delete
    /// button between 2.63:1 and 2.69:1 on its own surface while its resting
    /// state passed.
    ///
    /// Nord and Solarized are excused for the same reason they are excused
    /// elsewhere: their one red is mid-luminance and already sits at 2.74:1 and
    /// 3.12:1 at rest, so the states below it were never reachable.
    fn check_fill_states_stay_on_the_page(theme: &Theme) {
        if matches!(theme.name, "Nord" | "Solarized") {
            return;
        }
        let button = ButtonStyle::from_theme(theme, ButtonVariant::Destructive);
        let surface = ToasterStyle::from_theme(theme).background;
        for (state, fill) in [
            ("at rest", button.background),
            ("focused", button.focused_background),
            ("hovered", button.hovered_background),
        ] {
            let ratio = contrast(fill, surface);
            assert!(
                ratio >= LINE_FLOOR,
                "{}: the destructive fill {state} is {ratio:.2}:1 on a toast, below the \
                 {LINE_FLOOR}:1 floor",
                theme.name
            );
        }
    }

    /// Muted text has to be muted. It is the crate's most-painted color — every
    /// ordinary list row, every placeholder, every dialog description, every
    /// toast body — and if it lands on body text those roles stop being roles:
    /// a list's rows and its cursor row are then the same color, and the cursor
    /// row's whole emphasis is the fill under it.
    fn check_muted_recedes(theme: &Theme) {
        let step = contrast(theme.foreground, theme.muted_foreground);
        assert!(
            step >= MUTED_STEP,
            "{}: muted text is {step:.3}:1 from body text, below the {MUTED_STEP}:1 \
             that makes it a second register",
            theme.name
        );
        assert!(
            contrast(theme.muted_foreground, theme.background)
                < contrast(theme.foreground, theme.background),
            "{}: muted text is further from the background than body text",
            theme.name
        );
    }

    /// The three hued roles have to be three colors. A palette whose red slot
    /// holds a yellow repairs error and warning to the same tone otherwise, and
    /// an error that looks like a warning is worse than either being off-hue.
    fn check_accents_are_distinct(theme: &Theme) {
        for (left, right, pair) in [
            (theme.destructive, theme.warning, "destructive and warning"),
            (theme.destructive, theme.accent, "destructive and accent"),
            (theme.warning, theme.accent, "warning and accent"),
        ] {
            assert_ne!(left, right, "{}: {pair} are the same color", theme.name);
        }
    }

    /// A caret has to be findable on the screen it blinks on. Nothing paints it
    /// yet, which is exactly why it is measured: an unpainted role is the one
    /// that rots.
    fn check_cursor_is_visible(theme: &Theme) {
        let ratio = contrast(theme.cursor, theme.background);
        assert!(
            ratio >= LINE_FLOOR,
            "{}: the caret is {ratio:.2}:1 on the background, below the {LINE_FLOOR}:1 floor",
            theme.name
        );
    }

    /// A control's three fills, checked for size *and* direction.
    ///
    /// Size alone leaves the direction unpinned — swapping the focus and hover
    /// amounts, or hardcoding one end of the ramp in a component, changes every
    /// fill in the crate and moves no measured number. Direction is the sign of
    /// two successive luminance steps agreeing: whichever way a theme's polarity
    /// sends a fill, it has to keep going that way.
    fn check_three_fills(theme: &Theme, role: &str, rest: Color, focused: Color, hovered: Color) {
        for (step, from, to) in [("focus", rest, focused), ("hover", focused, hovered)] {
            let ratio = contrast(from, to);
            assert!(
                ratio >= VISIBLE_STEP,
                "{} {role}: the {step} step is {ratio:.3}:1, an invisible change",
                theme.name
            );
        }
        let (first, second) = (
            luminance(focused) - luminance(rest),
            luminance(hovered) - luminance(focused),
        );
        assert!(
            first * second > 0.0,
            "{} {role}: focus moves {first:+.4} and hover {second:+.4}, so the states turn back \
             on themselves",
            theme.name
        );
    }

    fn check_control_fills(theme: &Theme) {
        for (role, variant) in [
            ("default button", ButtonVariant::Default),
            ("secondary button", ButtonVariant::Secondary),
            ("destructive button", ButtonVariant::Destructive),
        ] {
            let button = ButtonStyle::from_theme(theme, variant);
            check_three_fills(
                theme,
                role,
                button.background,
                button.focused_background,
                button.hovered_background,
            );
        }
        let tabs = TabsStyle::from_theme(theme);
        check_three_fills(
            theme,
            "selected tab",
            tabs.selected_background,
            tabs.selected_focused_background,
            tabs.selected_hovered_background,
        );
        check_three_fills(
            theme,
            "tab",
            tabs.background,
            tabs.focused_background,
            tabs.hovered_background,
        );
    }

    /// A select is a list wearing a trigger: the two derive the same well from
    /// the same theme, and a component that hardcoded a direction its sibling
    /// asks for would show up here as a panel that no longer tracks the list it
    /// mirrors.
    fn check_select_tracks_the_list(theme: &Theme) {
        let list = ListStyle::from_theme(theme);
        let select = SelectStyle::from_theme(theme);
        for (role, left, right) in [
            ("well at rest", select.trigger_background, list.background),
            (
                "focused well",
                select.focused_trigger_background,
                list.focused_background,
            ),
            (
                "hovered well",
                select.hovered_trigger_background,
                list.hovered_background,
            ),
            (
                "cursor row",
                select.focused_option_background,
                list.focused_row_background,
            ),
            (
                "disabled well",
                select.disabled_background,
                list.disabled_background,
            ),
            (
                "disabled text",
                select.disabled_foreground,
                list.disabled_foreground,
            ),
        ] {
            assert_eq!(
                left, right,
                "{}: a select's {role} is not the list's",
                theme.name
            );
        }
    }

    fn check_severity_accents(theme: &Theme) {
        for (severity, accent, background) in severity_accents(theme) {
            let ratio = contrast(accent, background);
            assert!(
                ratio >= LINE_FLOOR,
                "{}: the {severity} accent is {ratio:.2}:1 on a toast, below the {LINE_FLOOR}:1 floor",
                theme.name
            );
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
    fn check_disabled_wells(theme: &Theme) {
        let style = ListStyle::from_theme(theme);
        for (half, enabled, disabled) in [
            ("fill", style.background, style.disabled_background),
            ("text", style.foreground, style.disabled_foreground),
        ] {
            let ratio = contrast(enabled, disabled);
            assert!(
                ratio >= VISIBLE_STEP,
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
            legibility >= DISABLED_LEGIBILITY,
            "{}: disabled rows are {legibility:.2}:1, past receding into gone",
            theme.name
        );
    }

    /// Text is the thing a palette is allowed to get wrong quietly: a fill that
    /// is one tone off still looks deliberate, while text one tone off is just
    /// hard to read. So every pair a component paints is measured, not only the
    /// ones on the background.
    fn check_text(theme: &Theme) {
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
        assert_ne!(
            dim(theme.field, away_from(theme.background), FIELD_FOCUS_SHIFT),
            theme.field
        );
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

        let pressed = nearest_to(theme.background);
        for color in [theme.primary, dim(theme.primary, pressed, FOCUS_SHIFT)] {
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
