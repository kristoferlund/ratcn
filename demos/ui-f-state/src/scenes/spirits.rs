//! The cast, one dossier each: who they are, what they do, and what they
//! never do.

use ratatui::layout::Rect;
use ratcn::Theme;

use super::{dot_at, say};
use crate::{
    canvas::{Frame, Rgb, ease_in_out, ease_out, lerp, mix, span},
    figures::{self, Pose},
    hud::{self, AMBER, BLACK, DIM, GREEN, GREY, PANEL_W, Panel, TEAL, WHITE, bold, fg},
    score,
};

struct Spirit {
    tag: &'static str,
    name: &'static str,
    kana: &'static str,
    color: Rgb,
    /// How much of the viewport the figure takes.
    scale: f64,
    fields: [(&'static str, &'static str); 3],
}

const SPIRITS: [Spirit; 3] = [
    Spirit {
        tag: "STATE",
        name: "THE MONK",
        kana: "ステート",
        color: GREEN,
        scale: 1.0,
        fields: [
            ("ROLE", "HOLDS THE ONE TRUTH"),
            ("LIVES IN", "YOUR APP"),
            ("WRITTEN BY", "update() ALONE"),
        ],
    },
    Spirit {
        tag: "RENDER",
        name: "THE RAT",
        kana: "レンダー",
        color: TEAL,
        scale: 0.82,
        fields: [
            ("ROLE", "PAINTS THE SCREEN"),
            ("FROM", "STATE, EVERY FRAME"),
            ("KEEPS", "NOTHING"),
        ],
    },
    Spirit {
        tag: "MESSAGE",
        name: "THE NINJA",
        kana: "メッセージ",
        color: AMBER,
        scale: 0.95,
        fields: [
            ("ROLE", "CARRIES WHAT THE USER DID"),
            ("BORN", "WHEN A COMPONENT IS USED"),
            ("CHANGES", "NOTHING"),
        ],
    },
];

pub fn scene(frame: &mut Frame, t: f64, theme: &Theme) {
    let which = (((t - score::MONK) / 5.0) as usize).min(2);
    let u = t - score::MONK - which as f64 * 5.0;
    let spirit = &SPIRITS[which];
    let (w, h) = (frame.w, frame.h);

    // The viewport the subject appears in.
    let view_x = 7;
    let view_w = ((f64::from(w) * 0.42) as i32).clamp(30, 52);
    let view_h = h - 5;
    hud::brackets(frame, (view_x, 2, view_w, view_h), GREY);
    frame.text(
        view_x + 2,
        view_h + 1,
        &format!("SUBJECT 0{}/03", which + 1),
        fg(GREY),
    );
    // The scarf and the brush are allowed out of the frame; the body is not.
    let size = (f64::from(view_h - 3) * 4.0).min(f64::from(view_w) * 2.0 * 0.92) * spirit.scale;
    let middle = f64::from(view_x) * 2.0 + f64::from(view_w);
    let base = f64::from(view_h) * 4.0;
    let mut pose = Pose::new(middle, base, size);
    let (ink, accent) = (WHITE, spirit.color);

    let column = view_x + view_w + 3;
    let artifact = 11;
    match which {
        0 => {
            // Found by a scan, already sitting there.
            let scan = base - size - 4.0 + (size + 8.0) * ease_in_out(span(u, 0.2, 1.4));
            pose.cut = scan;
            let head = (middle, base - size * 0.66);
            let turn = t * 0.08;
            frame.dots.circle(head, size * 0.36, mix(GREEN, BLACK, 0.5));
            frame
                .dots
                .arc(head, size * 0.36 + 5.0, turn, turn + 0.3, GREEN);
            frame
                .dots
                .arc(head, size * 0.36 + 5.0, turn + 0.5, turn + 0.8, GREEN);
            figures::monk(&mut frame.dots, pose, ink, accent);
            if u < 1.4 {
                let (left, right) = (
                    f64::from(view_x + 1) * 2.0,
                    f64::from(view_x + view_w - 1) * 2.0,
                );
                frame.dots.line((left, scan), (right, scan), GREEN);
            }

            let slide = ((1.0 - ease_out(span(u, 2.0, 2.5))) * 24.0) as i32;
            if u >= 2.0 {
                let rect = frame.clip(column + slide, artifact, 30, 5);
                let inner = hud::card(frame, rect, "state.rs", GREEN);
                hud::code(
                    frame,
                    inner,
                    &[
                        vec![("struct ", GREY), ("State", WHITE), (" {", GREY)],
                        vec![("    count", GREEN), (": u32,", GREY), ("   // 3", DIM)],
                        vec![("}", GREY)],
                    ],
                );
            }
            if u >= 3.0 {
                hud::tag(frame, column, artifact + 6, "OWNED BY YOUR APP", GREEN);
            }
            say(
                frame,
                column,
                artifact + 8,
                "NOT BY A WIDGET. NOT BY THE ENGINE.",
                u,
                3.4,
                fg(GREY),
            );
        }
        1 => {
            // Runs in from the right, brush first.
            let run = 1.0 - ease_out(span(u, 0.15, 0.9));
            pose.x += run * f64::from(view_w) * 2.2;
            pose.lean = -run * 0.3;
            if run > 0.02 {
                for streak in 0..14 {
                    let y = base - size * (0.1 + 0.06 * f64::from(streak));
                    let from = pose.x + size * 0.5;
                    frame
                        .dots
                        .dashed((from, y), (from + run * 90.0, y), GREY, 5, 4, streak * 3);
                }
            }
            figures::rat(&mut frame.dots, pose, ink, accent, t);

            // The panel is painted on left to right, the brush edge leading.
            let painted = ease_in_out(span(u, 1.7, 3.0));
            if painted > 0.0 {
                let edge = (painted * f64::from(PANEL_W)) as u16;
                let buffer = hud::panel_buffer(score::FIRST_COUNT, false, theme);
                frame.stamp(&buffer, column, artifact, |x, _| x < edge);
                if painted < 1.0 {
                    for row in 0..hud::PANEL_H {
                        frame.text(
                            column + i32::from(edge),
                            artifact + i32::from(row),
                            "▌",
                            fg(TEAL),
                        );
                    }
                }
            }
            if u >= 3.2 {
                hud::tag(frame, column, artifact + 10, "UI = F(STATE)", TEAL);
                say(
                    frame,
                    column + 17,
                    artifact + 10,
                    "3, BECAUSE STATE SAYS 3.",
                    u,
                    3.4,
                    fg(GREY),
                );
            }
        }
        _ => {
            let click = score::NINJA_CLICK - score::NINJA;
            let slash = score::NINJA_SLASH - score::NINJA;
            let panel = Rect::new(column as u16, artifact as u16, PANEL_W, hud::PANEL_H);
            let button = Panel::button(panel);
            let target = (i32::from(button.x) + 6, i32::from(button.y) + 1);
            let arrive = ease_in_out(span(u, 0.3, 1.05));
            let at = (
                lerp(f64::from(column + 27), f64::from(target.0), arrive) as i32,
                lerp(f64::from(artifact - 3), f64::from(target.1), arrive) as i32,
            );
            hud::panel(
                &mut frame.cells,
                panel,
                score::FIRST_COUNT,
                arrive > 0.85,
                theme,
            );

            if u >= slash {
                figures::ninja(
                    &mut frame.dots,
                    Pose {
                        x: middle - size * 0.08,
                        ..pose
                    },
                    ink,
                    accent,
                    t,
                );
                let cut = 1.0 - span(u, slash, slash + 0.3);
                if cut > 0.0 {
                    let (left, right) = (f64::from(view_x) * 2.0, f64::from(view_x + view_w) * 2.0);
                    for offset in [-1.0, 0.0, 1.0] {
                        frame
                            .dots
                            .line((left, base - offset), (right, 8.0 + offset), WHITE);
                    }
                    frame.flash = cut * 0.7;
                    frame.shake = (i32::from(cut > 0.6), 0);
                }
            } else {
                frame.text(
                    view_x + view_w / 2 - 5,
                    view_h / 2 + 1,
                    "NO SIGNAL",
                    fg(DIM),
                );
                let sweep = base - size + size * (u * 0.9).fract();
                let (left, right) = (
                    f64::from(view_x + 1) * 2.0,
                    f64::from(view_x + view_w - 1) * 2.0,
                );
                frame
                    .dots
                    .dashed((left, sweep), (right, sweep), DIM, 2, 3, 0);
            }

            // The press makes a message, and the message goes to the ninja.
            if u >= click + 0.05 {
                let caught = ease_in_out(span(u, click + 0.2, slash));
                let from = (f64::from(target.0 + 5), f64::from(target.1));
                let to = (f64::from(view_x + view_w / 2 + 2), 5.0);
                let (x, y) = (lerp(from.0, to.0, caught), lerp(from.1, to.1, caught));
                let (px, py) = dot_at(x as i32, y as i32);
                if caught > 0.0 && caught < 1.0 {
                    frame.dots.dashed(
                        dot_at(from.0 as i32, from.1 as i32),
                        (px, py),
                        AMBER,
                        2,
                        2,
                        0,
                    );
                }
                hud::tag(frame, x as i32, y as i32, "Msg::Increment", AMBER);
            }
            if u >= 2.6 {
                hud::tag(frame, column, artifact + 10, "STATE UNTOUCHED", AMBER);
                say(
                    frame,
                    column + 19,
                    artifact + 10,
                    "COUNT IS STILL 3.",
                    u,
                    2.8,
                    fg(GREY),
                );
            }
            hud::pointer(frame, at.0, at.1, span(u, click, click + 0.4));
        }
    }

    // The dossier.
    let tag_width = hud::tag(frame, column, 3, spirit.tag, spirit.color);
    frame.text(column + tag_width + 2, 3, spirit.kana, fg(DIM));
    say(frame, column, 5, spirit.name, u, 0.3, bold(WHITE));
    for (row, (label, value)) in spirit.fields.iter().enumerate() {
        let start = 0.7 + row as f64 * 0.45;
        say(frame, column, 7 + row as i32, label, u, start, fg(GREY));
        say(
            frame,
            column + 12,
            7 + row as i32,
            value,
            u,
            start + 0.1,
            fg(WHITE),
        );
    }

    hud::chrome(frame, t, "02 // THE THREE SPIRITS");
    frame.snow = 0.5;
    hud::dip(frame, u, 5.0);
}
