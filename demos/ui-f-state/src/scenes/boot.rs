//! Before: an 8-bit machine, a two-line program, and state everywhere.

use ratatui::style::Style;

use crate::{
    canvas::{Frame, Rgb, hex, ink, rnd, span},
    hud::{AMBER, WHITE, typed},
};

const BORDER: Rgb = hex(0x7c70da);
const SCREEN: Rgb = hex(0x3e31a2);
const TEXT: Rgb = hex(0xa59ff0);

const HEADER: [&str; 5] = [
    "    **** RATATUI 64 SHELL V1 ****",
    "",
    " 64K RAM SYSTEM  38911 CELLS FREE",
    "",
    "READY.",
];
/// What is typed, and when each line starts and ends.
const TYPED: [(&str, f64, f64); 3] = [
    ("10 PRINT \"STATE IS EVERYWHERE \";", 0.9, 2.7),
    ("20 GOTO 10", 2.9, 3.4),
    ("RUN", 3.7, 4.0),
];
const CHANT: &[u8] = b"STATE IS EVERYWHERE ";
const NOISE: &[u8] = b"#%&@$?/\\|=+*<>0O";
/// The program starts printing, and is stopped.
const RUNS: f64 = 4.3;
const BREAKS: f64 = 6.5;
/// The picture tube switches off.
const OFF: f64 = 7.0;

pub fn scene(frame: &mut Frame, t: f64) {
    let cols = (frame.w - 16).min(54);
    let rows = (frame.h - 8).min(20);
    let (x0, y0) = ((frame.w - cols) / 2, (frame.h - rows) / 2);

    if t >= OFF {
        // The screen folds to a line, then the line to a point.
        let line = span(t, OFF, OFF + 0.2);
        let point = span(t, OFF + 0.2, OFF + 0.42);
        let half = (f64::from(cols) / 2.0 * (1.0 - point)) as i32;
        let tall = ((f64::from(rows) * (1.0 - line)) as i32).max(1);
        if point < 1.0 {
            let glow = Style::default().bg(ink(WHITE));
            for y in 0..tall {
                for x in -half..=half {
                    frame.text(frame.w / 2 + x, frame.h / 2 - tall / 2 + y, " ", glow);
                }
            }
        }
        return;
    }

    frame.surround = Some(BORDER);
    frame.fade = 1.0 - span(t, 0.0, 0.5);
    let whole = frame.area();
    frame
        .cells
        .set_style(whole, Style::default().bg(ink(BORDER)));
    let screen = frame.clip(x0, y0, cols, rows);
    frame
        .cells
        .set_style(screen, Style::default().bg(ink(SCREEN)));

    // The screen as lines of `(character, corrupted)`.
    let mut lines: Vec<Vec<(char, bool)>> = HEADER
        .iter()
        .map(|line| line.chars().map(|ch| (ch, false)).collect())
        .collect();
    for (text, from, to) in TYPED {
        if t >= from {
            lines.push(
                typed(text, span(t, from, to))
                    .chars()
                    .map(|ch| (ch, false))
                    .collect(),
            );
        }
    }
    if t >= RUNS {
        let printed = (270.0 * (t.min(BREAKS) - RUNS).powi(2)) as usize;
        let rot = span(t, RUNS + 0.5, BREAKS) * 0.3;
        let tick = (t * 15.0) as u64;
        let chant: Vec<(char, bool)> = (0..printed)
            .map(|index| {
                if rnd(index as u64, tick / 3) < rot {
                    let noise = NOISE[(rnd(index as u64, tick) * NOISE.len() as f64) as usize];
                    (noise as char, true)
                } else {
                    (CHANT[index % CHANT.len()] as char, false)
                }
            })
            .collect();
        lines.extend(chant.chunks((cols - 2) as usize).map(<[_]>::to_vec));
    }
    if t >= BREAKS {
        lines.push(Vec::new());
        lines.push(
            "?OUT OF SYNC  ERROR IN 10"
                .chars()
                .map(|ch| (ch, false))
                .collect(),
        );
        lines.push("READY.".chars().map(|ch| (ch, false)).collect());
        frame.shake = (
            if t < BREAKS + 0.25 {
                ((t * 60.0) as i32 % 3) - 1
            } else {
                0
            },
            0,
        );
    }
    // The cursor waits on a line of its own unless something is being typed.
    let typing = TYPED.iter().any(|(_, from, to)| t >= *from && t < *to);
    let printing = (RUNS..BREAKS).contains(&t);
    if !typing && !printing {
        lines.push(Vec::new());
    }

    let visible = (rows - 2) as usize;
    let skip = lines.len().saturating_sub(visible);
    let style = Style::default().bg(ink(SCREEN));
    for (row, line) in lines.iter().skip(skip).enumerate() {
        for (column, (ch, corrupted)) in line.iter().enumerate() {
            let color = if *corrupted { AMBER } else { TEXT };
            let mut bytes = [0; 4];
            frame.text(
                x0 + 1 + column as i32,
                y0 + 1 + row as i32,
                ch.encode_utf8(&mut bytes),
                style.fg(ink(color)),
            );
        }
    }
    if !printing && (t * 2.5) as i32 % 2 == 0 {
        let last = lines.len() - skip - 1;
        let column = lines.last().map_or(0, Vec::len) as i32;
        frame.text(
            x0 + 1 + column,
            y0 + 1 + last as i32,
            " ",
            Style::default().bg(ink(TEXT)),
        );
    }
}
