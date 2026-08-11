//! Small color helpers for deriving state colors from a theme's base palette.
//!
//! A theme stores only base colors. Where a state color is a uniform function
//! of a base color — a bright fill darkening on focus, a dark fill lightening,
//! a control dimming when disabled — components derive it with these helpers
//! rather than the theme storing every variant. All operate on channels
//! resolved by [`resolve_rgb`]: `Color::Rgb` directly, the 16 named colors via
//! a fixed VGA approximation (so state changes stay visible on named-color
//! themes, at the cost of the derived color leaving the terminal palette), and
//! `Indexed`/`Reset` pass through unchanged (those carry no channels to
//! resolve).

use ratatui::style::Color;

// The standard amounts the library shifts a color by to express state, applied
// with the helpers below. Centralised so the visual design language reads in one
// place; each names the kind of element it governs.

/// A bright fill (a default or destructive button) darkens this much on focus.
/// The designed themes reproduce their focused accent within ~1/255.
pub const FOCUS_DARKEN: u16 = 10;

/// A dark fill (a secondary or ghost button) lightens this much on focus — the
/// mirror of [`FOCUS_DARKEN`], toward white instead of black.
pub const FOCUS_LIGHTEN: u16 = 10;

/// Hover is stronger than focus so moving the pointer over an already-focused
/// control still produces a visible state change.
pub const HOVER_DARKEN: u16 = 20;

/// The light-fill counterpart to [`HOVER_DARKEN`].
pub const HOVER_LIGHTEN: u16 = 20;

/// An inset well (input, list) lightens this much on focus — gentler than a
/// button, so a focused field brightens subtly rather than jumping.
pub const FIELD_FOCUS_LIGHTEN: u16 = 4;

/// The field counterpart to [`HOVER_LIGHTEN`]: double the focus shift, for the
/// same reason — the pointer over an already-focused field still produces a
/// visible state change.
pub const FIELD_HOVER_LIGHTEN: u16 = 8;

/// A disabled fill blends this far toward the surface — low enough that the
/// variant's hue still reads.
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

/// Darken a color toward black by `amount` percent. Amounts above 100 are
/// treated as 100.
///
/// Channels come from [`resolve_rgb`]; `Indexed` and [`Color::Reset`] pass
/// through unchanged.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "`c * keep / 100` with c <= 255 and keep <= 100 is <= 255, so the `as u8` cast cannot truncate"
)]
pub const fn darken(color: Color, amount: u16) -> Color {
    match resolve_rgb(color) {
        Some((r, g, b)) => {
            let amount = if amount > 100 { 100 } else { amount };
            let keep = 100 - amount;
            // `+ 50` rounds to nearest rather than flooring.
            Color::Rgb(
                ((r as u16 * keep + 50) / 100) as u8,
                ((g as u16 * keep + 50) / 100) as u8,
                ((b as u16 * keep + 50) / 100) as u8,
            )
        }
        None => color,
    }
}

/// Lighten a color toward white by `amount` percent — the mirror of
/// [`darken`], for a dark fill that should brighten (rather than deepen) as it
/// gains prominence on focus. Amounts above 100 are treated as 100.
///
/// Channels come from [`resolve_rgb`]; `Indexed` and [`Color::Reset`] pass
/// through unchanged.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "`c*(100-amount)/100 + 255*amount/100` is a weighted average of c and 255, both <= 255, so it stays <= 255 and the `as u8` cast cannot truncate"
)]
pub const fn lighten(color: Color, amount: u16) -> Color {
    match resolve_rgb(color) {
        Some((r, g, b)) => {
            let amount = if amount > 100 { 100 } else { amount };
            let keep = 100 - amount;
            // `+ 50` rounds to nearest rather than flooring.
            Color::Rgb(
                ((r as u16 * keep + 255 * amount + 50) / 100) as u8,
                ((g as u16 * keep + 255 * amount + 50) / 100) as u8,
                ((b as u16 * keep + 255 * amount + 50) / 100) as u8,
            )
        }
        None => color,
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

        assert_eq!(darken(color, 101), Color::Rgb(0, 0, 0));
        assert_eq!(lighten(color, u16::MAX), Color::Rgb(255, 255, 255));
        assert_eq!(dim(color, toward, 101), toward);
    }

    #[test]
    fn named_colors_derive_visible_state_changes() {
        // A named color resolves to its VGA approximation and scales like the
        // equivalent `Rgb`, so focus/hover/disabled stay visible on
        // named-color themes.
        assert_eq!(
            darken(Color::LightBlue, 10),
            darken(Color::Rgb(0, 0, 255), 10)
        );
        assert_ne!(darken(Color::LightBlue, 10), Color::LightBlue);
        assert_eq!(
            dim(Color::Red, Color::Black, 50),
            dim(Color::Rgb(128, 0, 0), Color::Rgb(0, 0, 0), 50)
        );
    }

    #[test]
    fn unresolvable_colors_pass_through() {
        assert_eq!(darken(Color::Reset, 10), Color::Reset);
        assert_eq!(lighten(Color::Indexed(42), 10), Color::Indexed(42));
        assert_eq!(
            dim(Color::Rgb(1, 2, 3), Color::Reset, 50),
            Color::Rgb(1, 2, 3)
        );
    }
}
