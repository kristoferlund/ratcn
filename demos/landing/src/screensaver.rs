//! The Alt+S screensaver: gentle snowfall on a hand-rolled modal layer.
//!
//! The effect is a pure function of the time elapsed since activation, so
//! there is no animation state to store or tick: the native loop polls on a
//! short timeout while the layer is open, wasm redraws every animation frame,
//! and both simply repaint from the clock. Deterministic per-flake parameters
//! come from a hash instead of an RNG, which keeps the demo dependency-free
//! and the animation identical across native and wasm.

use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};
use ratcn::runtime::{RenderCtx, ScopeOptions};

use crate::{AppMsg, AppState};

pub const ID: &str = "screensaver";

const SNOW_GLYPHS: [&str; 3] = ["*", "•", "·"];

/// The app-owned screensaver state: when it started. Whether the screensaver
/// is *open* lives in the modal stack, not here.
#[derive(Debug, Clone, Copy, Default)]
pub struct State {
    started: Duration,
}

impl State {
    /// The state for an activation at `now`, with the snowfall starting from
    /// zero.
    pub fn activate(now: Duration) -> Self {
        Self { started: now }
    }
}

/// Declare the screensaver as a hand-rolled modal layer. Call from the root
/// declaration closure when the modal stack says the screensaver is open.
///
/// The scope itself is the focusable leaf — nothing inside takes focus — and
/// the runtime dims the base layer when the layer opens. Snow is deferred from
/// the root so it paints onto that dimmed frame instead of an opaque layer
/// canvas, preserving the app beneath it.
pub fn declare(ctx: &mut RenderCtx<'_, '_, AppState, AppMsg>, area: Rect, now: Duration) {
    ctx.modal_scope(ID, area, ScopeOptions::default().focusable(), |_| {});
    ctx.defer_paint(move |painter, state| {
        let elapsed = now.saturating_sub(state.screensaver.started);
        painter.with_buffer(|buf| snow(buf, area, elapsed));
    });
}

/// Sparse white flakes drifting slowly downward with a little sideways wobble.
fn snow(buf: &mut Buffer, area: Rect, elapsed: Duration) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let millis = elapsed.as_millis() as u64;
    // Density: roughly one flake per one-and-a-half columns.
    let flakes = (u32::from(area.width) * 13 / 20) as u16;
    for flake in 0..flakes {
        let seed = hash2(u64::from(flake), 2);
        let step_ms = 140 + seed % 160;
        let fall = millis / step_ms + (seed >> 16);
        let y = (fall % u64::from(area.height)) as u16;
        let wobble = ((fall / 3 + (seed >> 24)) % 3) as i32 - 1;
        let x = (i32::from((seed % u64::from(area.width)) as u16) + wobble)
            .rem_euclid(i32::from(area.width)) as u16;
        let glyph = SNOW_GLYPHS[((seed >> 32) % SNOW_GLYPHS.len() as u64) as usize];
        let color = if seed & 1 == 0 {
            Color::White
        } else {
            Color::Gray
        };
        overlay(
            buf,
            area.x + x,
            area.y + y,
            glyph,
            Style::default().fg(color),
        );
    }
}

/// Block glyphs whose colour lives in the *foreground*, not the background.
///
/// Ratatui's `BarChart` draws a bar as `█` in the bar colour, and a large
/// button's caps are `▄`/`▀` in the fill colour. Writing a glyph over such a
/// cell replaces the thing that was carrying the colour, so the surface behind
/// shows through and the overlay looks like a hole.
const FILL_GLYPHS: &str = "█▉▊▋▌▍▎▏▁▂▃▄▅▆▇▀";

/// Draw `glyph` over whatever is already at `(x, y)`, keeping its surface.
///
/// A cell coloured by its background keeps that background. A cell coloured by a
/// block glyph hands its foreground over as the background instead, so an
/// overlay lands *on* a bar or a button cap rather than punching through it.
fn overlay(buf: &mut Buffer, x: u16, y: u16, glyph: &str, style: Style) {
    let Some(surface) = buf.cell((x, y)).map(|cell| {
        if !cell.symbol().is_empty() && cell.symbol().chars().all(|c| FILL_GLYPHS.contains(c)) {
            cell.fg
        } else {
            cell.bg
        }
    }) else {
        return;
    };
    buf.set_string(x, y, glyph, style.bg(surface));
}

/// SplitMix64: a tiny, well-distributed integer mixer. Stands in for an RNG
/// so the animation stays deterministic and dependency-free.
fn mix(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn hash2(a: u64, b: u64) -> u64 {
    mix(a ^ mix(b))
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, text::Line};
    use ratcn::{Theme, runtime::Ratcn};

    use super::*;

    #[test]
    fn screensaver_keeps_the_dimmed_app_visible_beneath_snow() {
        let mut state = AppState {
            screensaver: State::activate(Duration::ZERO),
            ..AppState::default()
        };
        state
            .modals_state
            .open(ID, &mut state.focus)
            .expect("screensaver opens once");
        let mut ratcn = Ratcn::new().modals(|state: &AppState| &state.modals_state);
        let mut terminal = Terminal::new(TestBackend::new(1, 1)).expect("terminal");
        let theme = Theme::default_dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                ratcn.render(frame, &state, &theme, |ctx| {
                    ctx.paint(move |ctx| ctx.render_widget(Line::from("A"), area));
                    declare(ctx, area, Duration::ZERO);
                });
            })
            .expect("draw");

        assert_eq!(
            terminal
                .backend()
                .buffer()
                .cell((0, 0))
                .expect("cell")
                .symbol(),
            "A"
        );
    }
}
