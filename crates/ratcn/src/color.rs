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
///
/// The shift is capped by the label that stays put on top of it. A fill moving
/// away from the background is also moving toward its own label: past about
/// this much, palettes with a mid-luminance accent (Solarized's blue, Nord's
/// frost) drop their hovered button labels under 4.5:1, which the preset
/// contrast test pins.
pub const FOCUS_DARKEN: u16 = 6;

/// A dark fill (a secondary or ghost button) lightens this much on focus — the
/// mirror of [`FOCUS_DARKEN`], toward white instead of black.
pub const FOCUS_LIGHTEN: u16 = 6;

/// Hover is stronger than focus so moving the pointer over an already-focused
/// control still produces a visible state change.
pub const HOVER_DARKEN: u16 = 12;

/// The light-fill counterpart to [`HOVER_DARKEN`].
pub const HOVER_LIGHTEN: u16 = 12;

/// An inset well (input, list) lightens this much on focus — gentler than a
/// button, so a focused field brightens subtly rather than jumping.
pub const FIELD_FOCUS_LIGHTEN: u16 = 4;

/// The field counterpart to [`HOVER_LIGHTEN`]: double the focus shift, for the
/// same reason — the pointer over an already-focused field still produces a
/// visible state change.
pub const FIELD_HOVER_LIGHTEN: u16 = 8;

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

/// Darken a color toward black by `amount` percent — [`dim`] against
/// [`Color::Black`]. Amounts above 100 are treated as 100.
///
/// Channels come from [`resolve_rgb`]; `Indexed` and [`Color::Reset`] pass
/// through unchanged.
#[must_use]
pub const fn darken(color: Color, amount: u16) -> Color {
    dim(color, Color::Black, amount)
}

/// Lighten a color toward white by `amount` percent — [`dim`] against
/// [`Color::White`], the mirror of [`darken`], for a dark fill that should
/// brighten (rather than deepen) as it gains prominence on focus. Amounts above
/// 100 are treated as 100.
///
/// Channels come from [`resolve_rgb`]; `Indexed` and [`Color::Reset`] pass
/// through unchanged.
#[must_use]
pub const fn lighten(color: Color, amount: u16) -> Color {
    dim(color, Color::White, amount)
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

    /// `darken` and `lighten` are `dim` against black and white, which holds
    /// only if the blend rounds identically to the standalone weighted average
    /// each of them used to compute — every channel value against every
    /// percentage, since one rounding difference would show as an off-by-one
    /// fill.
    #[test]
    fn darkening_and_lightening_round_as_a_blend_toward_the_endpoints() {
        for amount in 0..=100_u16 {
            let keep = 100 - amount;
            for channel in 0..=255_u8 {
                let color = Color::Rgb(channel, channel, channel);
                let scaled = u16::from(channel) * keep;
                let dark = u8::try_from((scaled + 50) / 100).expect("a darkened channel fits u8");
                let light = u8::try_from((scaled + 255 * amount + 50) / 100)
                    .expect("a lightened channel fits u8");
                assert_eq!(
                    darken(color, amount),
                    Color::Rgb(dark, dark, dark),
                    "darken({channel}, {amount})"
                );
                assert_eq!(
                    lighten(color, amount),
                    Color::Rgb(light, light, light),
                    "lighten({channel}, {amount})"
                );
            }
        }
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
