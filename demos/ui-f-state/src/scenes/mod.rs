//! The film, scene by scene. Each is a function from the film clock to a
//! frame, and none of them keeps anything from one frame to the next.

use ratatui::style::Style;
use ratcn::Theme;

use crate::{
    canvas::{Frame, span},
    hud::typed,
    score,
};

mod boot;
mod chaos;
pub mod finale;
mod loop_;
pub mod powers;
mod spirits;
mod title;

/// Paint the frame the film shows at `t`. `pressed` is how long ago the
/// viewer last pressed the live button, once the film has handed over.
pub fn render(frame: &mut Frame, t: f64, pressed: Option<f64>, theme: &Theme) {
    if t < score::CHAOS {
        boot::scene(frame, t);
    } else if t < score::BOOM {
        chaos::scene(frame, t);
    } else if t < score::MONK {
        title::scene(frame, t);
    } else if t < score::LOOP {
        spirits::scene(frame, t, theme);
    } else if t < score::POWERS {
        loop_::scene(frame, t, theme);
    } else if t < score::FINALE {
        powers::scene(frame, t, theme);
    } else {
        finale::scene(frame, t, pressed);
    }
}

/// The name of the chapter playing at `t`, for the timeline.
pub const CHAPTERS: [(f64, &str); 8] = [
    (0.0, "BOOT"),
    (score::CHAOS, "THE OLD WAY"),
    (score::BOOM, "UI = F(STATE)"),
    (score::MONK, "THE THREE SPIRITS"),
    (score::LOOP, "THE LOOP"),
    (score::POWERS, "WHAT IT MAKES POSSIBLE"),
    (score::FINALE, "RATCN"),
    (score::END, "YOUR TURN"),
];

/// Type `text` out from `start`, forty-five characters a second.
fn say(frame: &mut Frame, x: i32, y: i32, text: &str, now: f64, start: f64, style: Style) {
    if now >= start {
        let length = text.chars().count() as f64 / 45.0;
        frame.text(x, y, typed(text, span(now, start, start + length)), style);
    }
}

/// As [`say`], with the finished line centered on `center`.
fn say_centered(
    frame: &mut Frame,
    center: i32,
    y: i32,
    text: &str,
    now: f64,
    start: f64,
    style: Style,
) {
    say(
        frame,
        center - crate::canvas::text_width(text) / 2,
        y,
        text,
        now,
        start,
        style,
    );
}

/// The middle of a cell, in dots.
fn dot_at(x: i32, y: i32) -> (f64, f64) {
    (f64::from(x) * 2.0 + 1.0, f64::from(y) * 4.0 + 2.0)
}
