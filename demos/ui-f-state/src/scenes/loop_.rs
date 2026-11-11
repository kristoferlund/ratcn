//! The loop: state is rendered, input becomes a message, `update` applies it.
//! It turns slowly once, with every step named, and then at speed.

use ratatui::layout::Rect;
use ratcn::Theme;

use super::say_centered;
use crate::{
    canvas::{Frame, ease_in_out, lerp, mix, span},
    figures::{self, Pose},
    hud::{
        self, AMBER, BLACK, DIM, GREEN, GREY, PANEL_H, PANEL_W, Panel, RED, TEAL, WHITE, bold, fg,
    },
    score::{self, REPAINT_AT, UPDATE_AT},
};

/// When, within a turn, the message is handed to the ninja.
const HANDOFF: f64 = 0.12;
/// When the new frame sets off from the state.
const RENDER_AT: f64 = 0.52;

pub fn scene(frame: &mut Frame, t: f64, theme: &Theme) {
    let u = t - score::LOOP;
    let w = frame.w;
    let wall = frame.wall;
    // Laid out for thirty-odd rows, and centered in anything taller.
    let tall = frame.h.min(33);
    let oy = (frame.h - tall) / 2;
    let turn = score::turn(t);
    let phase = turn.map_or(0.0, |(_, phase)| phase);
    let turning = turn.is_some();

    // Top band: the monk and his state, the rat, the screen. Left to right,
    // the way a frame is made.
    let panel_x = w - i32::from(PANEL_W) - 2;
    let free = f64::from(w - 6 - 22 - i32::from(PANEL_W) - 6);
    let figure = (free / 1.15).clamp(12.0, 38.0);
    let card_x = 6 + (figure / 2.0) as i32 + 2;
    let rat_x = (card_x + 22 + panel_x) / 2;
    let floor = f64::from(oy + 12) * 4.0;
    // Bottom band: the way back. Down from the button, along to the gate, up
    // into the state.
    let rail = oy + tall - 5;
    let (down, up) = (panel_x + 6, card_x + 11);
    let runner = (f64::from(tall - 20) * 4.0).clamp(14.0, 36.0);

    let built = |at: f64| u >= at;
    let rail_y = f64::from(rail) * 4.0 + 2.0;

    // The circuit, drawn in as the scene opens.
    let drawn = span(u, 0.4, 1.2);
    let top_y = f64::from(oy + 6) * 4.0 + 2.0;
    let top = (f64::from(card_x + 22) * 2.0, f64::from(panel_x) * 2.0);
    frame.dots.dashed(
        (top.0, top_y),
        (lerp(top.0, top.1, drawn), top_y),
        DIM,
        1,
        3,
        0,
    );
    let (down_x, up_x) = (f64::from(down) * 2.0, f64::from(up) * 2.0);
    let from_panel = f64::from(oy + 2 + i32::from(PANEL_H)) * 4.0;
    frame.dots.dashed(
        (down_x, from_panel),
        (down_x, lerp(from_panel, rail_y, drawn)),
        DIM,
        1,
        3,
        0,
    );
    frame.dots.dashed(
        (down_x, rail_y),
        (lerp(down_x, up_x, drawn), rail_y),
        DIM,
        1,
        3,
        0,
    );
    let into_state = f64::from(oy + 9) * 4.0;
    frame.dots.dashed(
        (up_x, rail_y),
        (up_x, lerp(rail_y, into_state, drawn)),
        DIM,
        1,
        3,
        0,
    );

    // A pulse travelling a leg of the circuit, with a short tail.
    let pulse = |frame: &mut Frame, from: (f64, f64), to: (f64, f64), amount: f64, color| {
        if amount <= 0.0 || amount >= 1.0 {
            return;
        }
        let head = (lerp(from.0, to.0, amount), lerp(from.1, to.1, amount));
        let tail = (amount - 0.25).max(0.0);
        frame.dots.line(
            (lerp(from.0, to.0, tail), lerp(from.1, to.1, tail)),
            head,
            color,
        );
        frame.dots.disc(head, 2.0, 1.0, color);
    };

    // The gate every change passes through.
    let gate_x = f64::from(up + down) - 10.0;
    if built(0.8) {
        let lit = turning && (phase - 0.3).abs() < 0.06;
        figures::torii(
            &mut frame.dots,
            gate_x,
            rail_y,
            runner * 1.15,
            if lit { WHITE } else { RED },
        );
        hud::tag_centered(
            frame,
            (gate_x / 2.0) as i32,
            rail + 1,
            "fn update(msg)",
            if lit { WHITE } else { RED },
        );
    }

    // The ninja takes the message from under the button to the state, through
    // the gate, and is back at her post before anyone sees her return.
    let post = f64::from(down - 4) * 2.0;
    let dash = ease_in_out(span(phase, HANDOFF, UPDATE_AT - 0.05));
    let carrying = turning && (HANDOFF..UPDATE_AT).contains(&phase);
    let ninja_x = if carrying {
        lerp(post, up_x + 10.0, dash)
    } else {
        post
    };
    if built(0.6) {
        if carrying {
            for ghost in 1..=3 {
                let behind = ninja_x + f64::from(ghost) * (1.0 - (dash * 2.0 - 1.0).abs()) * 16.0;
                figures::ninja(
                    &mut frame.dots,
                    Pose {
                        lean: 0.25,
                        ..Pose::new(behind, rail_y, runner)
                    },
                    mix(DIM, BLACK, f64::from(ghost) * 0.25),
                    DIM,
                    wall,
                );
            }
        }
        figures::ninja(
            &mut frame.dots,
            Pose {
                lean: if carrying { 0.25 } else { 0.0 },
                ..Pose::new(ninja_x, rail_y, runner)
            },
            WHITE,
            AMBER,
            wall,
        );
        if carrying {
            hud::tag(
                frame,
                (ninja_x / 2.0) as i32 - 7,
                rail - (runner / 4.0) as i32 - 1,
                "Msg::Increment",
                AMBER,
            );
        } else {
            frame.text(down + 2, rail - 1, "INPUT", fg(GREY));
        }
    }
    if turning {
        pulse(
            frame,
            (down_x, from_panel),
            (down_x, rail_y),
            span(phase, 0.02, HANDOFF),
            AMBER,
        );
        pulse(
            frame,
            (up_x, rail_y),
            (up_x, into_state),
            span(phase, UPDATE_AT - 0.06, UPDATE_AT),
            AMBER,
        );
        // And the way out again: state, through the rat, to the screen.
        pulse(
            frame,
            (top.0, top_y),
            (top.1, top_y),
            span(phase, RENDER_AT, REPAINT_AT),
            TEAL,
        );
    }

    // The monk, and the state he holds.
    let written = turn.map_or(1.0, |(index, phase)| {
        let length = score::TURNS[index + 1] - score::TURNS[index];
        if phase >= UPDATE_AT {
            (phase - UPDATE_AT) * length / 0.5
        } else {
            1.0
        }
    });
    if built(0.1) {
        let monk_x = 12.0 + figure / 2.0;
        let head = (monk_x, floor - figure * 0.66);
        frame.dots.circle(
            head,
            figure * 0.36 + (1.0 - written.min(1.0)) * 6.0,
            mix(GREEN, BLACK, 0.4 * written.min(1.0)),
        );
        figures::monk(
            &mut frame.dots,
            Pose::new(monk_x, floor, figure),
            WHITE,
            GREEN,
        );
        let rect = frame.clip(card_x, oy + 4, 22, 5);
        let inner = hud::card(
            frame,
            rect,
            "State",
            if written < 1.0 { WHITE } else { GREEN },
        );
        let count = format!("{},", score::state_count(t));
        hud::code(
            frame,
            inner,
            &[
                vec![("struct ", GREY), ("State", WHITE), (" {", GREY)],
                vec![
                    ("    count", GREEN),
                    (": ", GREY),
                    (&count, if written < 1.0 { WHITE } else { GREEN }),
                ],
                vec![("}", GREY)],
            ],
        );
    }

    // The rat, who hops when a frame goes through his hands.
    if built(0.25) {
        let painting = turning && phase > RENDER_AT + 0.1 && phase < REPAINT_AT;
        let hop = if painting {
            ((phase - RENDER_AT) * 40.0).sin().abs() * 4.0
        } else {
            0.0
        };
        figures::rat(
            &mut frame.dots,
            Pose::new(f64::from(rat_x) * 2.0, floor - hop, figure),
            WHITE,
            TEAL,
            wall,
        );
        hud::tag_centered(frame, rat_x, oy + 12, "RENDER", TEAL);
    }

    // The screen, one repaint behind the state.
    if built(0.35) {
        let panel = Rect::new(panel_x as u16, (oy + 2) as u16, PANEL_W, PANEL_H);
        hud::panel(&mut frame.cells, panel, score::shown_count(t), true, theme);
        let button = Panel::button(panel);
        let fresh = turning && phase >= REPAINT_AT;
        if fresh {
            hud::brackets(
                frame,
                (
                    panel_x - 1,
                    oy + 1,
                    i32::from(PANEL_W) + 2,
                    i32::from(PANEL_H) + 2,
                ),
                TEAL,
            );
        }
        if built(1.0) {
            let press = if turning { span(phase, 0.0, 0.1) } else { 0.0 };
            hud::pointer(
                frame,
                i32::from(button.x) + 6,
                i32::from(button.y) + 1,
                press,
            );
        }
    }

    // What is being said, and the readouts.
    let line = match turn {
        None if u < 2.0 => "ONE LOOP RUNS THE WHOLE APP.",
        None => "",
        Some((0, phase)) if phase < HANDOFF => "THE USER PRESSES + 1.",
        Some((0, phase)) if phase < UPDATE_AT => "THE BUTTON WRITES NOTHING. IT EMITS A MESSAGE.",
        Some((0, phase)) if phase < RENDER_AT + 0.08 => {
            "update() APPLIES IT: THE ONLY PLACE STATE CHANGES."
        }
        Some((0, _)) => "THE NEXT FRAME IS PAINTED FROM STATE. THAT IS ALL.",
        Some((1, _)) => "AGAIN.",
        Some((2, _)) => "AND AGAIN.",
        Some(_) => "ONE DIRECTION.  ONE WRITER.  NO SURPRISES.",
    };
    let since = match turn {
        Some((0, phase)) => {
            let length = score::TURNS[1] - score::TURNS[0];
            let start = [0.0, HANDOFF, UPDATE_AT, RENDER_AT + 0.08]
                .into_iter()
                .rev()
                .find(|start| phase >= *start)
                .unwrap_or(0.0);
            (phase - start) * length
        }
        Some((index, _)) => t - score::TURNS[index.min(3)],
        None => u - 0.3,
    };
    say_centered(frame, w / 2, oy + 14, line, since, 0.0, bold(WHITE));

    if built(1.0) && up - 8 > 18 {
        let turns = turn.map_or(0, |(index, _)| index + 1);
        let rows = [
            ("TURN", format!("{turns:02}"), WHITE),
            ("STATE", format!("count = {}", score::state_count(t)), GREEN),
            ("SCREEN", format!("count = {}", score::shown_count(t)), TEAL),
        ];
        for (row, (label, value, color)) in rows.iter().enumerate() {
            let y = rail - 5 + row as i32;
            frame.text(7, y, label, fg(GREY));
            frame.text(15, y, value, fg(*color));
        }
    }

    hud::chrome(frame, t, "03 // THE LOOP");
    frame.snow = 0.4;
    hud::dip(frame, u, score::POWERS - score::LOOP);
    // The state landing is felt.
    if turning && phase >= UPDATE_AT && written < 0.25 {
        frame.shake = (0, 1);
    }
}
