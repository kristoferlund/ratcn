//! Small color helpers for deriving state colors from a theme's base palette.
//!
//! A theme stores only base colors. Where a state color is a uniform function
//! of a base color — a well separating from the background on focus, a fill
//! deepening, a control dimming when disabled — components derive it with these
//! helpers rather than the theme storing every variant. All operate on channels
//! resolved by [`resolve_rgb`]: `Color::Rgb` directly, the 16 named colors via
//! a fixed VGA approximation (so state changes stay visible on named-color
//! themes, at the cost of the derived color leaving the terminal palette), and
//! `Indexed`/`Reset` pass through unchanged (those carry no channels to
//! resolve).

use std::sync::LazyLock;

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
/// the screen's own end is also moving toward its own label: past about this
/// much, palettes with a mid-luminance accent (Solarized's blue, Nord's frost)
/// drop their hovered button labels under 4.5:1, which the preset contrast test
/// pins.
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
    let channel = &*LINEAR_CHANNEL;
    Some(0.2126 * channel[r as usize] + 0.7152 * channel[g as usize] + 0.0722 * channel[b as usize])
}

/// The sRGB transfer function, one entry per channel value.
///
/// A channel is a `u8`, so this is the whole domain — the table is exact, not
/// an approximation. It exists because luminance is the crate's innermost loop:
/// solving one adaptive theme measures thousands of candidate colors, and
/// `powf` per channel per measurement made that the dominant cost.
static LINEAR_CHANNEL: LazyLock<[f64; 256]> = LazyLock::new(|| {
    std::array::from_fn(|value| {
        #[allow(
            clippy::cast_precision_loss,
            reason = "the index is 0..=255, which f64 represents exactly"
        )]
        let value = value as f64 / 255.0;
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    })
});

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

    /// The table is a cache, so it has to hold exactly what it caches.
    ///
    /// Every contrast floor in the crate is measured through it — a theme is
    /// solved against these numbers and tested against these numbers, so a
    /// table that drifted from the formula would move both sides at once and
    /// nothing else would notice. The comparison is exact rather than
    /// approximate because the table is built by this formula and nothing
    /// stands between them: an epsilon here would license a drift.
    #[test]
    fn the_channel_table_is_the_wcag_transfer_function() {
        for value in 0..=255_usize {
            #[allow(
                clippy::cast_precision_loss,
                reason = "the index is 0..=255, which f64 represents exactly"
            )]
            let scaled = value as f64 / 255.0;
            let expected = if scaled <= 0.039_28 {
                scaled / 12.92
            } else {
                ((scaled + 0.055) / 1.055).powf(2.4)
            };
            assert_eq!(
                LINEAR_CHANNEL[value].to_bits(),
                expected.to_bits(),
                "the cached transfer of channel {value} is {}, not the {expected} the formula \
                 gives",
                LINEAR_CHANNEL[value]
            );
        }
    }

    /// A color the crate cannot read is taken as dark, so a theme that leaves
    /// its background to the terminal keeps the lighter-is-more direction the
    /// crate's own default has. The pair is documented on both functions and
    /// every derived state on the terminal preset depends on it.
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
        assert_eq!(darken(Color::Reset, 10), Color::Reset);
        assert_eq!(lighten(Color::Indexed(42), 10), Color::Indexed(42));
        assert_eq!(
            dim(Color::Rgb(1, 2, 3), Color::Reset, 50),
            Color::Rgb(1, 2, 3)
        );
    }
}
