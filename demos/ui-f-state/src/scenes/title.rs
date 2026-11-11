//! It blows up, and the pieces come back down as one line.

use super::say_centered;
use crate::{
    canvas::{Frame, ease_in_out, ease_out, hex, lerp, mix, rnd, span},
    figures::{TITLE_H, title_cells, title_width},
    hud::{self, BLACK, DIM, GREY, WHITE, bold, fg},
    score,
};

const DEBRIS: &[u8] = b"#%&@$0O/\\|=+-_:;(){}[]<>37?";

/// Whether there is room for the title's heavier cut.
pub fn title_bold(width: i32) -> bool {
    width >= title_width(true) + 12
}

/// The finished title, its top left at `(x, y)`: `#` faces with a shadow
/// thrown down and to the right.
pub fn title(frame: &mut Frame, x: i32, y: i32, bold_cut: bool) {
    let cells = title_cells(bold_cut);
    for (column, row) in &cells {
        frame.text(x + column + 1, y + row + 1, "░", fg(DIM));
    }
    for (column, row) in &cells {
        frame.text(x + column, y + row, "#", bold(WHITE));
    }
}

pub fn scene(frame: &mut Frame, t: f64) {
    let u = t - score::BOOM;
    let lands = score::TITLE_LANDS - score::BOOM;
    let (w, h) = (frame.w, frame.h);
    let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
    let center = (frame.dots.w() / 2.0, frame.dots.h() / 2.0);

    // The blast: speed lines, two rings, and everything that was on screen.
    let blast = 1.0 - span(u, 0.0, 1.3);
    if blast > 0.0 {
        hud::speed_lines(frame, center, t, blast, WHITE);
    }
    for (delay, color) in [(0.0, WHITE), (0.25, GREY)] {
        let ring = ease_out(span(u, delay, delay + 1.8));
        if ring > 0.0 && ring < 1.0 {
            frame.dots.circle(
                center,
                ring * frame.dots.w() * 0.55,
                mix(color, BLACK, ring),
            );
        }
    }
    let settle = span(u, 1.0, 2.9);
    if settle < 1.0 {
        let flown = ease_out(span(u, 0.0, 2.4));
        for piece in 0..170u64 {
            let turn = rnd(piece, 1) * std::f64::consts::TAU;
            let speed = 0.25 + rnd(piece, 2) * 0.9;
            let x = cx + turn.cos() * speed * flown * cx * 1.3;
            let y = cy + turn.sin() * speed * flown * cy * 1.3 + u * u * rnd(piece, 3) * 1.4;
            let glyph = DEBRIS[(rnd(piece, 4) * DEBRIS.len() as f64) as usize] as char;
            let tan = mix(hex(0xc8a878), WHITE, rnd(piece, 5));
            frame.text(
                x as i32,
                y as i32,
                glyph.encode_utf8(&mut [0; 4]),
                fg(mix(tan, BLACK, settle)),
            );
        }
    }

    let heavy = title_bold(w);
    let wide = title_width(heavy);
    let (tx, ty) = ((w - wide) / 2, (h - TITLE_H) / 2 - 2);
    if u >= lands {
        let since = u - lands;
        title(frame, tx, ty, heavy);
        // Rules run out from the title to both edges.
        let reach = ease_out(span(since, 0.0, 0.6));
        let rule_y = f64::from(ty + 3) * 4.0 + 2.0;
        let (left, right) = (f64::from(tx - 3) * 2.0, f64::from(tx + wide + 4) * 2.0);
        frame
            .dots
            .line((left, rule_y), (lerp(left, 12.0, reach), rule_y), GREY);
        frame.dots.line(
            (right, rule_y),
            (lerp(right, frame.dots.w() - 4.0, reach), rule_y),
            GREY,
        );
        if since > 0.35 {
            hud::tag(frame, tx, ty - 2, "RATCN", WHITE);
            frame.text(tx + 8, ty - 2, "ラットシーエヌ", fg(DIM));
            let credit = "A SAGA IN THREE SPIRITS";
            frame.text(tx + wide - credit.len() as i32, ty - 2, credit, fg(GREY));
        }
        say_centered(
            frame,
            w / 2,
            ty + TITLE_H + 3,
            "THE SCREEN IS A FUNCTION OF YOUR STATE.",
            since,
            0.8,
            bold(WHITE),
        );
        say_centered(
            frame,
            w / 2,
            ty + TITLE_H + 5,
            "YOUR APP OWNS IT  ·  COMPONENTS READ IT  ·  UPDATE WRITES IT",
            since,
            1.9,
            fg(GREY),
        );
        hud::chrome(frame, t, "01 // UI = F(STATE)");
        frame.flash = (1.0 - span(since, 0.0, 0.45)) * 0.6;
        frame.shake = (0, i32::from(since < 0.12));
    } else {
        // Every pixel of the title starts somewhere in the wreckage and flies
        // home, each on its own schedule.
        for (index, (column, row)) in title_cells(heavy).into_iter().enumerate() {
            let index = index as u64;
            let leaves = 1.4 + rnd(index, 7) * 1.3;
            let flight = ease_in_out(span(u, leaves, leaves + 0.9));
            if flight <= 0.0 {
                continue;
            }
            let turn = rnd(index, 8) * std::f64::consts::TAU;
            let (sx, sy) = (cx + turn.cos() * cx * 1.1, cy + turn.sin() * cy * 1.2);
            let home = (f64::from(tx + column), f64::from(ty + row));
            let (x, y) = (lerp(sx, home.0, flight), lerp(sy, home.1, flight));
            let glyph = if flight < 1.0 { "*" } else { "#" };
            frame.text(
                x as i32,
                y as i32,
                glyph,
                fg(mix(hex(0xc8a878), WHITE, flight)),
            );
        }
    }

    frame.snow = span(u, 1.0, 3.5);
    frame.flash = frame.flash.max((1.0 - span(u, 0.0, 0.5)).powi(2));
    if u < 0.6 {
        let quake = (1.0 - span(u, 0.0, 0.6)) * 2.0;
        frame.shake = (
            (quake * (t * 47.0).sin()).round() as i32,
            (quake * (t * 39.0).cos()).round() as i32,
        );
    }
    frame.fade = span(
        u,
        score::MONK - score::BOOM - 0.3,
        score::MONK - score::BOOM,
    );
}
