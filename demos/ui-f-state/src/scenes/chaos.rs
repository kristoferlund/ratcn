//! The old way: five widgets, five copies of the truth, and every one of them
//! telling the others.

use super::{dot_at, say};
use crate::{
    canvas::{Frame, ease_in_out, hash, lerp, mix, rnd, span},
    hud::{self, AMBER, BLACK, DIM, RED, WHITE, bold, fg},
    score,
};

/// The widgets, and where each sits in the scene as fractions of it.
const WIDGETS: [(&str, f64, f64); 5] = [
    ("HEADER", 0.14, 0.14),
    ("LIST", 0.74, 0.1),
    ("BADGE", 0.45, 0.4),
    ("SIDEBAR", 0.08, 0.62),
    ("FOOTER", 0.68, 0.66),
];
/// The one the pointer clicks.
const BADGE: usize = 2;
/// Who tells whom, in the order they start.
const WIRES: [(usize, usize); 10] = [
    (2, 0),
    (2, 1),
    (2, 3),
    (2, 4),
    (0, 1),
    (3, 4),
    (0, 3),
    (1, 4),
    (0, 4),
    (1, 3),
];
const VALUES: [&str; 12] = [
    "3", "4", "4", "5", "4", "7", "2", "9", "NaN", "??", "-1", "4",
];
const ALARMS: [(f64, usize, &str); 6] = [
    (3.4, 0, "OUT OF SYNC"),
    (4.1, 1, "STALE"),
    (4.8, 3, "WHO WROTE THIS?"),
    (5.5, 4, "SIGNAL LOST"),
    (6.1, 0, "DIRECTION UNKNOWN"),
    (6.6, 2, "SYSTEM FAILURE"),
];
const LINES: [(f64, &str); 4] = [
    (0.3, "ONCE, EVERY WIDGET KEPT ITS OWN STATE."),
    (2.2, "THEN A CLICK CHANGED ONE OF THEM."),
    (4.0, "SO THEY STARTED TELLING EACH OTHER."),
    (6.4, "NOBODY AGREED ON THE TRUTH."),
];

/// What widget `index` believes the count is, `u` seconds into the scene.
fn belief(index: usize, u: f64) -> &'static str {
    let click = score::CHAOS_CLICK - score::CHAOS;
    // Rumors reach a widget once its first wire is up, and travel faster and
    // faster after that.
    let wired = WIRES
        .iter()
        .position(|wire| wire.0 == index || wire.1 == index)
        .map_or(f64::MAX, |wire| 3.2 + wire as f64 * 0.42);
    if u >= wired {
        let since = u - wired;
        let rumor = (since * (1.2 + since * 1.4)) as u64;
        VALUES[(hash(rumor ^ ((index as u64) << 32)) % VALUES.len() as u64) as usize]
    } else if index == BADGE && u >= click {
        "4"
    } else {
        "3"
    }
}

pub fn scene(frame: &mut Frame, t: f64) {
    let u = t - score::CHAOS;
    let wall = frame.wall;
    let (w, h) = (frame.w, frame.h);
    let place = |index: usize| {
        let (_, fx, fy) = WIDGETS[index];
        (
            8 + (f64::from(w - 34) * fx) as i32,
            3 + (f64::from(h - 9) * fy) as i32,
        )
    };

    // Wires first, so the widgets sit on top of them.
    for (wire, (a, b)) in WIRES.iter().enumerate() {
        let start = 2.8 + wire as f64 * 0.42;
        if u < start {
            continue;
        }
        let (from, to) = (place(*a), place(*b));
        let (from, to) = (dot_at(from.0, from.1), dot_at(to.0, to.1));
        let drawn = span(u, start, start + 0.3);
        let end = (lerp(from.0, to.0, drawn), lerp(from.1, to.1, drawn));
        frame
            .dots
            .dashed(from, end, WHITE, 2, 2, -(wall * 20.0) as i32);
        let along = (wall * 0.9 + wire as f64 * 0.37).fract();
        let pulse = (lerp(from.0, to.0, along), lerp(from.1, to.1, along));
        frame.dots.disc(pulse, 1.6, 1.0, AMBER);
    }
    // Then far too many of them.
    let tangle = (span(u, 5.5, 9.8).powi(2) * 48.0) as u64;
    for strand in 0..tangle {
        let from = place((hash(strand) % 5) as usize);
        let jitter = if strand % 3 == 0 {
            (wall * 9.0) as u64
        } else {
            0
        };
        let to = (
            rnd(strand, 11 + jitter) * frame.dots.w(),
            rnd(strand, 12 + jitter) * frame.dots.h(),
        );
        let color = mix(DIM, RED, rnd(strand, 13) * span(u, 6.5, 9.5));
        frame.dots.line(dot_at(from.0, from.1), to, color);
    }

    for (index, (name, ..)) in WIDGETS.iter().enumerate() {
        let appear = 0.2 + index as f64 * 0.22;
        if u < appear {
            continue;
        }
        let (x, y) = place(index);
        let center = dot_at(x, y);
        frame
            .dots
            .arc(center, 6.0, 0.0, span(u, appear, appear + 0.4), WHITE);
        frame.dots.disc(center, 1.6, 1.0, WHITE);
        let value = belief(index, u);
        let color = if value == "3" {
            WHITE
        } else if value == "4" && u < 3.2 {
            AMBER
        } else {
            RED
        };
        say(frame, x + 5, y - 1, name, u, appear + 0.2, bold(WHITE));
        if u >= appear + 0.35 {
            frame.text(x + 5, y, &format!("count={value}"), fg(color));
            frame.text(
                x + 5,
                y + 1,
                &format!("W-0{}//0x{:04X}", index + 1, hash(index as u64) as u16),
                fg(DIM),
            );
        }
    }

    let click = score::CHAOS_CLICK - score::CHAOS;
    if u >= click + 0.2 {
        let (x, y) = place(BADGE);
        hud::tag(frame, x + 5, y + 2, "MUTATED IN PLACE", AMBER);
    }
    for (slot, (at, index, text)) in ALARMS.iter().enumerate() {
        if u >= *at && ((u - at) > 0.3 || (wall * 16.0) as i32 % 2 == 0) {
            let (x, y) = place(*index);
            hud::tag(frame, x + 5, y + 2 + (slot as i32 / 4), text, RED);
        }
    }
    for extra in 0..(span(u, 7.0, 10.0).powi(2) * 26.0) as u64 {
        let x = 6 + (rnd(extra, 21) * f64::from(w - 16)) as i32;
        let y = 2 + (rnd(extra, 22) * f64::from(h - 6)) as i32;
        hud::tag(
            frame,
            x,
            y,
            if extra % 4 == 0 { "FAILURE" } else { "ERROR" },
            RED,
        );
    }
    // ゴゴゴ: the sound of something about to give.
    for rumble in 0..((u - 7.4) * 2.5).max(0.0) as u64 {
        let x = 8 + (rnd(rumble, 31) * f64::from(w - 20)) as i32;
        let y = 2 + (rnd(rumble, 32) * f64::from(h - 6)) as i32 + ((wall * 10.0) as i32 % 2);
        frame.text(x, y, "ゴゴゴ", bold(WHITE));
    }

    // The pointer comes in, clicks the badge, and leaves it to its fate.
    if (0.5..3.0).contains(&u) {
        let (bx, by) = place(BADGE);
        let arrive = ease_in_out(span(u, 0.5, click - 0.1));
        let x = lerp(f64::from(w) * 0.85, f64::from(bx) + 1.0, arrive) as i32;
        let y = lerp(f64::from(h) * 0.9, f64::from(by), arrive) as i32;
        hud::pointer(frame, x, y, span(u, click, click + 0.45));
    }

    // The latest line of narration, on a strip of its own.
    if let Some((start, text)) = LINES.iter().rev().find(|(start, _)| u >= *start) {
        let strip = frame.clip(6, h - 3, w - 8, 1);
        frame
            .cells
            .set_style(strip, fg(WHITE).bg(crate::canvas::ink(BLACK)));
        frame.text(6, h - 3, "▌", fg(RED));
        say(frame, 8, h - 3, text, u, *start, bold(WHITE));
    }

    hud::chrome(frame, t, "00 // THE OLD WAY");
    frame.snow = 0.5;
    frame.fade = 1.0 - span(u, 0.0, 0.4);
    let quake = span(u, 6.5, 10.0) * 1.7;
    frame.shake = (
        (quake * (wall * 41.0).sin()).round() as i32,
        (quake * 0.6 * (wall * 33.0).cos()).round() as i32,
    );
    frame.glitch = span(u, 7.5, 10.0).powi(2) * 0.22;
    frame.invert = u > 9.4 && (wall * 14.0) as i32 % 2 == 0;
    let brace = span(u, 8.8, 10.0);
    if brace > 0.0 {
        hud::speed_lines(
            frame,
            (frame.dots.w() / 2.0, frame.dots.h() / 2.0),
            wall,
            brace,
            WHITE,
        );
    }
}
