//! Small color helpers for deriving state colors from a theme's base palette.
//!
//! A theme stores only base colors. Where a state color is a uniform function
//! of a base color — a well separating from the background on focus, a fill
//! deepening, a control dimming when disabled — components derive it with these
//! helpers rather than the theme storing every variant. All operate on channels
//! resolved by [`resolve_rgb`]: `Color::Rgb` directly, and the 16 named colors
//! via a fixed VGA approximation, so a color derived from a named base is an
//! `Rgb` near it. `Indexed` and `Reset` carry no channels to resolve and pass
//! through unchanged.

use ratatui::style::Color;

// The standard amounts the library shifts a color by to express state, applied
// with the helpers below. Centralised so the visual design language reads in one
// place; each names the kind of element it governs.

/// A filled control (a button, a selected tab) shifts this far on focus.
///
/// The amount carries no direction of its own: which end a fill moves toward
/// comes from [`nearest_to`] and [`away_from`], so the same number deepens a
/// light theme's control and brightens a dark theme's.
///
/// It is capped by the label that stays put on top of it. A fill moving toward
/// the screen's own end is also moving toward its own label, and past about
/// this much a palette with a mid-luminance accent (Solarized's blue, Nord's
/// frost) loses the hovered button label the preset contrast test pins.
pub const FOCUS_SHIFT: u16 = 6;

/// Hover is stronger than focus so moving the pointer over an already-focused
/// control still produces a visible state change.
pub const HOVER_SHIFT: u16 = 12;

/// An inset well (input, list) shifts this far from the background on focus —
/// gentler than a button, so a focused field separates subtly rather than
/// jumping.
pub const FIELD_FOCUS_SHIFT: u16 = 4;

/// The well counterpart to [`HOVER_SHIFT`]: double the focus shift, for the
/// same reason — the pointer over an already-focused well still produces a
/// visible state change.
pub const FIELD_HOVER_SHIFT: u16 = 8;

/// The cursor row inside a well shifts this far past the focused well it sits
/// on — the top rung of the ladder, and the only one a row rather than the
/// whole control gets.
///
/// It is the largest of the state shifts because it marks one row out of many
/// — [`DISABLED_DIM`] is larger still, but that is a blend toward another
/// layer rather than a step along the ladder. It is also the rung where a
/// theme's text budget runs out first, which is why a list draws it in the
/// ordinary foreground rather than the muted one.
pub const ROW_FOCUS_SHIFT: u16 = 15;

/// A disabled control blends this far toward the layer behind it — low enough
/// that the variant's hue still reads.
///
/// Which layer is the caller's: a button dims toward the surface, so a disabled
/// destructive button stays red-ish, while a well and its text dim toward the
/// background, because a well already sits one tone off the background and
/// dimming it toward the surface would move it nowhere.
pub const DISABLED_DIM: u16 = 50;

/// Resolve a color to rgb channels: `Rgb` as-is, the 16 named colors via a
/// fixed VGA approximation, `Indexed`/`Reset` to `None` (the terminal owns
/// their palette and offers no way to read it back).
///
/// The one channel table in the crate — state derivation here and backdrop
/// dimming both resolve through it, so they cannot disagree about which colors
/// are scalable.
#[must_use]
pub const fn resolve_rgb(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((128, 0, 0)),
        Color::Green => Some((0, 128, 0)),
        Color::Yellow => Some((128, 128, 0)),
        Color::Blue => Some((0, 0, 128)),
        Color::Magenta => Some((128, 0, 128)),
        Color::Cyan => Some((0, 128, 128)),
        Color::Gray => Some((192, 192, 192)),
        Color::DarkGray => Some((128, 128, 128)),
        Color::LightRed => Some((255, 0, 0)),
        Color::LightGreen => Some((0, 255, 0)),
        Color::LightYellow => Some((255, 255, 0)),
        Color::LightBlue => Some((0, 0, 255)),
        Color::LightMagenta => Some((255, 0, 255)),
        Color::LightCyan => Some((0, 255, 255)),
        Color::White => Some((255, 255, 255)),
        Color::Indexed(_) | Color::Reset => None,
    }
}

/// Relative luminance of the channels [`resolve_rgb`] reads, per WCAG 2.1 —
/// `None` for a color that carries none.
///
/// This is the crate's one definition of how light a color is. The derivation
/// and the tests that hold it to contrast floors share it, so a theme cannot
/// pass a floor that is measured differently than it was solved.
#[must_use]
pub fn luminance(color: Color) -> Option<f64> {
    let (r, g, b) = resolve_rgb(color)?;
    Some(0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b))
}

/// The sRGB transfer function: one channel, linearised.
fn linear(channel: u8) -> f64 {
    let value = f64::from(channel) / 255.0;
    if value <= 0.039_28 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// The WCAG 2.1 contrast ratio between two colors, from 1.0 to 21.0 — `None`
/// if either carries no channels to measure.
#[must_use]
pub fn contrast(a: Color, b: Color) -> Option<f64> {
    match (luminance(a), luminance(b)) {
        (Some(a), Some(b)) => Some((a.max(b) + 0.05) / (a.min(b) + 0.05)),
        _ => None,
    }
}

/// Whether a color is light enough that things drawn against it have to get
/// darker to be seen. Unreadable colors are taken as dark, which is the
/// assumption the crate's own default makes.
#[must_use]
fn is_light(anchor: Color) -> bool {
    luminance(anchor).is_some_and(|luminance| luminance > 0.5)
}

/// The end of the gray ramp `anchor` sits furthest from: white for a dark
/// anchor, black for a light one.
///
/// This is the direction "more" points in. A well is lifted out of the
/// background by shifting it this way, and each rung above it goes further the
/// same way, so one call decides a whole ladder's polarity — on a dark terminal
/// the wells lighten, on a light one they darken, and nothing else in the
/// derivation has to know which.
///
/// A color with no channels to read is taken as dark, so a theme that leaves
/// its background to the terminal keeps the lighter-is-more direction the
/// crate's own default has.
#[must_use]
pub fn away_from(anchor: Color) -> Color {
    if is_light(anchor) {
        Color::Black
    } else {
        Color::White
    }
}

/// The end of the gray ramp `anchor` sits nearest to — the mirror of
/// [`away_from`], and the direction "quieter" points in.
///
/// A filled control deepens toward the screen's own end as it gains focus,
/// rather than climbing away from it like a well: the fill is already the loud
/// thing on the screen, and the state change is meant to read as pressed.
#[must_use]
pub fn nearest_to(anchor: Color) -> Color {
    if is_light(anchor) {
        Color::White
    } else {
        Color::Black
    }
}

/// The first of these two that [`resolve_rgb`] can read channels from.
///
/// A blend *target* has to be a real color. [`Color::Reset`] is a real color on
/// screen — it is the terminal's own — but nothing here can read it, and
/// blending toward it returns the original unchanged, which is how a state
/// silently stops being a state. Name the layer you mean and the layer to fall
/// back to when the theme leaves that one to the terminal.
#[must_use]
pub const fn blendable(color: Color, fallback: Color) -> Color {
    match resolve_rgb(color) {
        Some(_) => color,
        None => fallback,
    }
}

/// Blend `color` toward `toward` by `amount` percent, keeping a fraction of the
/// original hue so a dimmed control isn't flattened to a neutral grey (a
/// disabled destructive button stays red-ish). Amounts above 100 are treated
/// as 100.
///
/// If either color resolves to no rgb channels (see [`resolve_rgb`]), `color`
/// passes through unchanged.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "each channel is a weighted average of two u8s, so it stays within 0..=255 and the `as u8` cast cannot truncate"
)]
pub const fn dim(color: Color, toward: Color, amount: u16) -> Color {
    match (resolve_rgb(color), resolve_rgb(toward)) {
        (Some((r, g, b)), Some((tr, tg, tb))) => {
            let amount = if amount > 100 { 100 } else { amount };
            let keep = 100 - amount;
            // `+ 50` rounds to nearest rather than flooring.
            Color::Rgb(
                ((r as u16 * keep + tr as u16 * amount + 50) / 100) as u8,
                ((g as u16 * keep + tg as u16 * amount + 50) / 100) as u8,
                ((b as u16 * keep + tb as u16 * amount + 50) / 100) as u8,
            )
        }
        _ => color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_above_one_hundred_clamp_to_the_endpoint() {
        let color = Color::Rgb(20, 40, 60);
        let toward = Color::Rgb(100, 120, 140);

        assert_eq!(dim(color, Color::Black, 101), Color::Rgb(0, 0, 0));
        assert_eq!(
            dim(color, Color::White, u16::MAX),
            Color::Rgb(255, 255, 255)
        );
        assert_eq!(dim(color, toward, 101), toward);
    }

    #[test]
    fn named_colors_derive_visible_state_changes() {
        // A named color resolves to its VGA approximation and scales like the
        // equivalent `Rgb`, so focus/hover/disabled stay visible on
        // named-color themes.
        assert_eq!(
            dim(Color::LightBlue, Color::Black, 10),
            dim(Color::Rgb(0, 0, 255), Color::Black, 10)
        );
        assert_ne!(dim(Color::LightBlue, Color::Black, 10), Color::LightBlue);
        assert_eq!(
            dim(Color::Red, Color::Black, 50),
            dim(Color::Rgb(128, 0, 0), Color::Rgb(0, 0, 0), 50)
        );
    }

    /// A blend toward either endpoint rounds to nearest. The boundaries are
    /// where a rounding difference would show.
    #[test]
    fn dimming_rounds_to_nearest_at_both_endpoints() {
        let gray = Color::Rgb(128, 128, 128);

        assert_eq!(dim(gray, Color::Black, 0), gray, "nothing moves at all");
        assert_eq!(dim(gray, Color::White, 0), gray);
        assert_eq!(
            dim(gray, Color::Black, 100),
            Color::Rgb(0, 0, 0),
            "all the way down"
        );
        assert_eq!(
            dim(gray, Color::White, 100),
            Color::Rgb(255, 255, 255),
            "all the way up"
        );
        // Halves round away from zero, in both directions and at both ends.
        assert_eq!(
            dim(Color::Rgb(1, 1, 1), Color::Black, 50),
            Color::Rgb(1, 1, 1)
        );
        assert_eq!(
            dim(Color::Rgb(3, 3, 3), Color::Black, 50),
            Color::Rgb(2, 2, 2)
        );
        assert_eq!(
            dim(Color::Rgb(254, 254, 254), Color::White, 50),
            Color::Rgb(255, 255, 255)
        );
        assert_eq!(
            dim(Color::Rgb(255, 255, 255), Color::Black, 1),
            Color::Rgb(252, 252, 252)
        );
    }

    /// A color the crate cannot read is taken as dark, so a theme that leaves
    /// its background to the terminal keeps the lighter-is-more direction the
    /// crate's own default has.
    #[test]
    fn an_unreadable_anchor_points_the_way_a_dark_one_does() {
        assert_eq!(
            away_from(Color::Reset),
            Color::White,
            "Color::Reset stopped pointing away toward white, so a terminal-background theme \
             lifts its wells the wrong way"
        );
        assert_eq!(
            nearest_to(Color::Reset),
            Color::Black,
            "Color::Reset stopped pointing nearest toward black, so a terminal-background \
             theme presses its fills the wrong way"
        );
    }

    #[test]
    fn unresolvable_colors_pass_through() {
        assert_eq!(dim(Color::Reset, Color::Black, 10), Color::Reset);
        assert_eq!(
            dim(Color::Indexed(42), Color::White, 10),
            Color::Indexed(42)
        );
        assert_eq!(
            dim(Color::Rgb(1, 2, 3), Color::Reset, 50),
            Color::Rgb(1, 2, 3)
        );
    }
}
