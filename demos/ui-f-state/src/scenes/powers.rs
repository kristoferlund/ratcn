//! What follows from the screen being a function of state: four short proofs.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::Widget,
};
use ratcn::{ButtonSize, ButtonVariant, ButtonWidget, Theme};

use super::{dot_at, say, say_centered};
use crate::{
    canvas::{Frame, ease_in_out, ease_out, ink, lerp, span},
    hud::{
        self, AMBER, BLACK, DIM, FAINT, GREEN, GREY, PANEL_H, PANEL_W, RED, TEAL, WHITE, bold, fg,
    },
    score,
};

/// Which of the four beats `t` is in, and how far into it.
fn beat(t: f64) -> (usize, f64) {
    let index = (((t - score::POWERS) / score::POWER) as usize).min(3);
    (index, t - score::POWERS - index as f64 * score::POWER)
}

/// How far down a beat's content starts, so it sits in the middle of a tall
/// scene rather than at the top of it. `rows` is how many it needs.
fn top(frame: &Frame, rows: i32) -> i32 {
    5 + ((frame.h - 6 - rows) / 2).max(0)
}

/// The beat's heading: a numeral, a name, and the claim.
fn heading(frame: &mut Frame, u: f64, numeral: &str, name: &str, claim: &str) {
    hud::tag(frame, 7, 2, numeral, WHITE);
    say(frame, 13, 2, name, u, 0.1, bold(WHITE));
    say(frame, 13, 3, claim, u, 0.4, fg(GREY));
}

pub fn scene(frame: &mut Frame, t: f64, theme: &Theme) {
    let (index, u) = beat(t);
    match index {
        0 => rewind(frame, t, u, theme),
        1 => faces(frame, u, theme),
        2 => layers_scene(frame, t, u),
        _ => yours(frame, u, theme),
    }
    hud::chrome(frame, t, "04 // WHAT IT MAKES POSSIBLE");
    frame.snow = 0.4;
    hud::dip(frame, u, score::POWER);
}

/// Keep the states, and any moment can be drawn again.
fn rewind(frame: &mut Frame, t: f64, u: f64, theme: &Theme) {
    let w = frame.w;
    heading(
        frame,
        u,
        "一",
        "REWIND",
        "KEEP THE STATES, AND ANY MOMENT CAN BE DRAWN AGAIN.",
    );

    let head = score::playhead(u);
    let speed = (score::playhead(u + 0.05) - score::playhead(u - 0.05)) / 0.1;
    let shown = head.round() as i32;
    let tape_x = (w - 49) / 2;
    let y0 = top(frame, 16);
    frame.text(tape_x, y0, "STATE HISTORY", fg(GREY));
    let mode = if speed < -0.5 {
        ("◀◀ REW", AMBER)
    } else if speed > 0.5 {
        ("▶▶ FWD", AMBER)
    } else {
        ("▶ PLAY", GREEN)
    };
    hud::tag(frame, tape_x + 49 - 8, y0, mode.0, mode.1);
    for slot in 0..10 {
        let style = if slot == shown {
            Style::default()
                .fg(ink(BLACK))
                .bg(ink(GREEN))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(ink(GREY)).bg(ink(FAINT))
        };
        let count = score::FIRST_COUNT + slot as u32;
        frame.text(tape_x + slot * 5, y0 + 2, &format!(" {count:02} "), style);
    }
    let needle = tape_x + (head * 5.0) as i32 + 1;
    frame.text(needle, y0 + 1, "▼", fg(WHITE));

    let panel_x = (w - i32::from(PANEL_W)) / 2;
    let panel_y = y0 + 5;
    let count = score::FIRST_COUNT + shown as u32;
    let buffer = hud::panel_buffer(count, false, theme);
    frame.stamp(&buffer, panel_x, panel_y, |_, _| true);
    let from = dot_at(needle, y0 + 3);
    frame
        .dots
        .line((from.0, from.1 - 2.0), dot_at(needle, y0 + 4), TEAL);
    frame
        .dots
        .line(dot_at(needle, y0 + 4), dot_at(w / 2, y0 + 4), TEAL);

    let left = panel_x - 27;
    if left >= 7 {
        say(
            frame,
            left,
            panel_y + 2,
            "NOTHING IS REPLAYED.",
            u,
            0.8,
            fg(WHITE),
        );
        say(
            frame,
            left,
            panel_y + 3,
            "NOTHING IS UNDONE.",
            u,
            1.3,
            fg(WHITE),
        );
        say(
            frame,
            left,
            panel_y + 5,
            "A STATE GOES IN.",
            u,
            2.0,
            fg(GREY),
        );
        say(
            frame,
            left,
            panel_y + 6,
            "ITS FRAME COMES OUT.",
            u,
            2.4,
            fg(GREY),
        );
    }
    let right = panel_x + i32::from(PANEL_W) + 3;
    if right + 24 <= w && u >= 1.0 {
        let rect = frame.clip(right, panel_y + 1, 24, 6);
        let inner = hud::card(frame, rect, "test", GREY);
        let settled = speed.abs() < 0.5;
        let index = format!("[{shown}]");
        hud::code(
            frame,
            inner,
            &[
                vec![("assert_eq!(", GREY)],
                vec![
                    ("  render", TEAL),
                    ("(&states", GREY),
                    (&index, WHITE),
                    ("),", GREY),
                ],
                vec![("  golden", GREY), (&index, WHITE), (",", GREY)],
                vec![(")", GREY), (if settled { "  ✓ pass" } else { "" }, GREEN)],
            ],
        );
    }
    hud::tracking(frame, t, (speed.abs() / 12.0).min(1.0));
}

/// The same state, drawn by five themes at once.
fn faces(frame: &mut Frame, u: f64, theme: &Theme) {
    let w = frame.w;
    heading(
        frame,
        u,
        "二",
        "MANY FACES",
        "A THEME IS ONE MORE ARGUMENT:  ui = f(state, theme)",
    );

    let themes = [
        *theme,
        Theme::catppuccin(),
        Theme::gruvbox(),
        Theme::nord(),
        Theme::tokyo_night(),
    ];
    let panel = i32::from(PANEL_W);
    let step = panel + 1;
    let fit = ((w - 7) / step).clamp(2, themes.len() as i32);
    let row_x = (w - (step * (fit - 1) + panel)) / 2 + 1;
    let card_y = top(frame, 17);
    let (card_x, row_y) = ((w - 26) / 2, card_y + 6);

    let rect = frame.clip(card_x, card_y, 26, 3);
    let inner = hud::card(frame, rect, "state", GREEN);
    hud::code(
        frame,
        inner,
        &[vec![
            (" State { ", GREY),
            ("count: 12", GREEN),
            (" }", GREY),
        ]],
    );

    for (slot, theme) in themes.iter().take(fit as usize).enumerate() {
        let start = 0.7 + slot as f64 * 0.5;
        if u < start {
            continue;
        }
        let dealt = ease_out(span(u, start, start + 0.45));
        let home = row_x + slot as i32 * step;
        let x = lerp(f64::from((w - panel) / 2), f64::from(home), dealt) as i32;
        let y = lerp(f64::from(card_y + 2), f64::from(row_y), dealt) as i32;
        let from = dot_at(w / 2, card_y + 3);
        frame
            .dots
            .line((from.0, from.1 - 2.0), dot_at(x + panel / 2, y), TEAL);
        let buffer = hud::panel_buffer(score::FIRST_COUNT + 9, false, theme);
        frame.stamp(&buffer, x, y, |_, _| true);
        if dealt >= 1.0 {
            hud::tag(
                frame,
                home + 1,
                row_y + i32::from(PANEL_H),
                &theme.name.to_uppercase(),
                WHITE,
            );
        }
    }
}

/// What the layers beat asks of the live engine, `u` seconds in.
pub struct Layers {
    /// The scripted pointer, as a cell in the scene.
    pub pointer: (i32, i32),
    /// The tooltip is open.
    pub hint: bool,
    /// How long the toast has been up.
    pub toast: Option<f64>,
    /// The dialog is open.
    pub modal: bool,
}

/// Where the engine paints the panel during the layers beat and at the end.
pub fn panel_rect(w: i32, h: i32) -> Rect {
    Rect::new(
        ((w - i32::from(PANEL_W)) / 2) as u16,
        ((h - i32::from(PANEL_H)) / 2) as u16,
        PANEL_W,
        PANEL_H,
    )
}

/// The layers beat `t` is in, if it is in one.
pub fn layers(t: f64, w: i32, h: i32) -> Option<Layers> {
    let (index, u) = beat(t);
    if !(score::POWERS..score::FINALE).contains(&t) || index != 2 {
        return None;
    }
    let button = hud::Panel::button(panel_rect(w, h));
    let arrive = ease_in_out(span(u, 0.3, 0.9));
    let target = (f64::from(button.x) + 6.0, f64::from(button.y) + 1.0);
    Some(Layers {
        pointer: (
            lerp(target.0 + 24.0, target.0, arrive) as i32,
            lerp(target.1 + 6.0, target.1, arrive) as i32,
        ),
        hint: (1.0..3.2).contains(&u),
        toast: (u >= 2.0).then_some(u - 2.0),
        modal: (3.2..4.5).contains(&u),
    })
}

/// A stack of three planes, the lit ones in color: the engine's layers.
fn layers_scene(frame: &mut Frame, t: f64, u: f64) {
    let (w, h) = (frame.w, frame.h);
    heading(
        frame,
        u,
        "三",
        "LAYERS",
        "WHAT FLOATS ABOVE THE SCREEN IS STATE TOO.",
    );
    let Some(script) = layers(t, w, h) else {
        return;
    };
    let entries = [
        ("HINT", "EXPLAINS. NEVER IN THE WAY.", script.hint, TEAL),
        (
            "TOAST",
            "REPORTS. THEN LEAVES.",
            script.toast.is_some(),
            GREEN,
        ),
        ("MODAL", "TAKES OVER. HOLDS FOCUS.", script.modal, AMBER),
    ];
    let panel = panel_rect(w, h);
    let left = 7;
    let room = i32::from(panel.x) - left - 3;
    for (row, (name, role, lit, color)) in entries.iter().enumerate() {
        let y = i32::from(panel.y) + 1 + row as i32 * 3;
        if *lit {
            hud::tag(frame, left, y, name, *color);
        } else {
            frame.text(left + 1, y, name, fg(DIM));
        }
        if room >= 28 {
            frame.text(left, y + 1, role, fg(if *lit { WHITE } else { DIM }));
        }
    }
    // The stack itself, to the right: base at the bottom, modal on top.
    let x = f64::from(i32::from(panel.x) + i32::from(PANEL_W) + 6) * 2.0;
    if x + 60.0 < frame.dots.w() {
        let planes = [
            ("BASE", true, WHITE),
            ("HINT", script.hint, TEAL),
            ("TOAST", script.toast.is_some(), GREEN),
            ("MODAL", script.modal, AMBER),
        ];
        for (level, (name, lit, color)) in planes.iter().enumerate() {
            let y = f64::from(i32::from(panel.y) + i32::from(PANEL_H)) * 4.0 - level as f64 * 11.0;
            let color = if *lit { *color } else { FAINT };
            let (skew, wide, deep) = (14.0, 40.0, 9.0);
            let corners = [
                (x + skew, y - deep),
                (x + skew + wide, y - deep),
                (x + wide, y),
                (x, y),
            ];
            for corner in 0..4 {
                frame
                    .dots
                    .line(corners[corner], corners[(corner + 1) % 4], color);
            }
            frame.text(
                ((x + wide + skew) / 2.0) as i32 + 2,
                (y / 4.0) as i32 - 1,
                name,
                fg(color),
            );
        }
    }
    say_centered(
        frame,
        w / 2,
        i32::from(panel.y) - 2,
        "DECLARED FROM ANYWHERE IN YOUR UI. PAINTED ABOVE ALL OF IT.",
        u,
        1.2,
        fg(GREY),
    );
}

/// The component is source in your project, and you change it.
fn yours(frame: &mut Frame, u: f64, theme: &Theme) {
    let w = frame.w;
    heading(
        frame,
        u,
        "四",
        "YOURS",
        "NOT A DEPENDENCY YOU CONFIGURE. SOURCE YOU OWN.",
    );

    let card_w = 42.min(w - 44);
    let stage_w = (w - card_w - 14).min(44);
    let x0 = (w - card_w - 4 - stage_w) / 2;
    let y0 = top(frame, 13);
    frame.text(x0, y0, "$", fg(GREEN));
    say(
        frame,
        x0 + 2,
        y0,
        "cargo ratcn add button",
        u,
        0.2,
        fg(WHITE),
    );

    /// When each edit lands.
    const EDITS: [f64; 4] = [1.5, 2.2, 2.9, 3.6];
    let step = EDITS.iter().filter(|at| u >= **at).count();
    let label = if step >= 4 { "RING THE BELL" } else { "+ 1" };
    if u >= 0.9 {
        let rect = frame.clip(x0, y0 + 2, card_w, 8);
        let inner = hud::card(frame, rect, "src/ui/button.rs", WHITE);
        let quoted = format!("\"{label}\"");
        let fresh = |index: usize| if step == index { WHITE } else { GREY };
        let mut lines = vec![vec![
            ("Button", TEAL),
            ("::new(", GREY),
            (&quoted, if step == 4 { WHITE } else { GREEN }),
            (")", GREY),
        ]];
        if step >= 1 {
            lines.push(vec![("    .size(ButtonSize::Large)", fresh(1))]);
        }
        if step >= 2 {
            lines.push(vec![(
                "    .outline()",
                if step == 2 { WHITE } else { DIM },
            )]);
        }
        if step >= 3 {
            lines.push(vec![(
                "    .destructive()",
                if step == 3 { WHITE } else { RED },
            )]);
        }
        lines.push(vec![
            ("    .on_press(|| ", GREY),
            ("Msg::Increment", AMBER),
            (")", GREY),
        ]);
        hud::code(frame, inner, &lines);
    }

    // The preview, which is whatever the source says.
    let stage = (x0 + card_w + 4, y0 + 2, stage_w, 8);
    let landed = EDITS
        .iter()
        .map(|at| u - at)
        .filter(|since| *since >= 0.0)
        .fold(f64::MAX, f64::min);
    hud::brackets(frame, stage, if landed < 0.2 { WHITE } else { GREY });
    frame.text(stage.0 + 2, stage.1 + stage.3, "LIVE PREVIEW", fg(GREY));
    if u >= 0.9 {
        let button = ButtonWidget::new(label)
            .themed(theme)
            .size(if step >= 1 {
                ButtonSize::Large
            } else {
                ButtonSize::Small
            })
            .variant(match step {
                0 | 1 => ButtonVariant::Default,
                2 => ButtonVariant::Outline,
                _ => ButtonVariant::Destructive,
            });
        let (bw, bh) = (button.width(), if step >= 1 { 3 } else { 1 });
        let mut buffer = Buffer::empty(Rect::new(0, 0, bw, bh));
        button.render(buffer.area, &mut buffer);
        frame.stamp(
            &buffer,
            stage.0 + (stage.2 - i32::from(bw)) / 2,
            stage.1 + (stage.3 - i32::from(bh)) / 2,
            |_, _| true,
        );
    }
    say_centered(
        frame,
        w / 2,
        y0 + 12,
        "COPY IT IN. READ IT. CHANGE IT. IT IS YOUR COMPONENT NOW.",
        u,
        2.6,
        bold(WHITE),
    );
}
