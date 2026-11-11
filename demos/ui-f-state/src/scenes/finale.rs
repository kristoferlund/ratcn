//! The three of them under the moon, the line once more, and then the screen
//! is the viewer's.

use ratatui::layout::Rect;

use super::{
    say_centered,
    title::{title, title_bold},
};
use crate::{
    canvas::{Frame, ease_in_out, ease_out, lerp, mix, span},
    figures::{self, Pose, title_width},
    hud::{self, AMBER, BLACK, DIM, GREEN, GREY, PANEL_H, PANEL_W, TEAL, WHITE, bold, fg},
    score,
};

/// The row the cast stands on.
fn horizon(h: i32) -> i32 {
    h - 8
}

/// Where the live panel sits once the film is over.
pub fn panel_rect(w: i32, h: i32) -> Rect {
    Rect::new(
        ((w - i32::from(PANEL_W)) / 2) as u16,
        (horizon(h) - i32::from(PANEL_H)).max(0) as u16,
        PANEL_W,
        PANEL_H,
    )
}

/// Where the replay button sits.
pub fn replay_rect(w: i32, h: i32) -> Rect {
    Rect::new((w / 2 + 3) as u16, (horizon(h) + 5) as u16, 10, 1)
}

pub fn scene(frame: &mut Frame, t: f64, pressed: Option<f64>) {
    let u = t - score::FINALE;
    let live = t >= score::END;
    let (w, h) = (frame.w, frame.h);
    let ground = horizon(h);
    let ground_y = f64::from(ground) * 4.0;
    let center_x = frame.dots.w() / 2.0;

    // The moon comes up behind them, instruments tracking it.
    let radius = (ground_y * 0.4).min(frame.dots.w() * 0.16);
    let risen = ease_out(span(u, 0.0, 3.5));
    let moon = (
        center_x,
        lerp(ground_y + radius * 0.2, ground_y * 0.5, risen),
    );
    figures::moon(&mut frame.dots, moon, radius, mix(WHITE, BLACK, 0.45));
    let turn = frame.wall * 0.03;
    frame.dots.arc(moon, radius + 6.0, turn, turn + 0.22, GREY);
    frame
        .dots
        .arc(moon, radius + 6.0, turn + 0.5, turn + 0.72, GREY);
    frame
        .dots
        .arc(moon, radius + 11.0, -turn * 2.0, -turn * 2.0 + 0.12, DIM);
    // Nothing of the moon shows below the horizon.
    for y in ground_y as i32..frame.dots.h() as i32 {
        for x in 0..frame.dots.w() as i32 {
            frame.dots.erase(x, y);
        }
    }
    frame
        .dots
        .line((12.0, ground_y), (frame.dots.w() - 4.0, ground_y), GREY);
    for tick in (8..w - 2).step_by(8) {
        let x = f64::from(tick) * 2.0;
        frame.dots.line((x, ground_y), (x, ground_y + 2.0), GREY);
    }

    // The cast. Once the film is over the rat's place is taken by the panel
    // he paints.
    let heavy = title_bold(w);
    let size = (f64::from(ground - 9) * 4.0).clamp(16.0, 44.0);
    let places = [0.2, 0.5, 0.8].map(|at| (f64::from(w) * at) as i32);
    let names = [("STATE", GREEN), ("RENDER", TEAL), ("MESSAGE", AMBER)];
    for (index, column) in places.iter().enumerate() {
        let start = 0.4 + index as f64 * 0.5;
        let x = f64::from(*column) * 2.0;
        let mut pose = Pose::new(x, ground_y, size);
        pose.cut = ground_y - size + (size + 2.0) * ease_in_out(span(u, start, start + 1.0));
        match index {
            0 => figures::monk(&mut frame.dots, pose, WHITE, GREEN),
            1 if !live => figures::rat(&mut frame.dots, pose, WHITE, TEAL, frame.wall),
            2 => figures::ninja(&mut frame.dots, pose, WHITE, AMBER, frame.wall),
            _ => {}
        }
        if u >= start + 1.0 && !(live && index == 1) {
            hud::tag_centered(frame, *column, ground + 1, names[index].0, names[index].1);
        }
    }
    // The loop, still running between them.
    if u >= 2.0 {
        let y = ground_y + 6.0;
        let (left, right) = (
            f64::from(places[0] + 5) * 2.0,
            f64::from(places[2] - 6) * 2.0,
        );
        frame.dots.dashed(
            (left, y),
            (right, y),
            DIM,
            2,
            4,
            -(frame.wall * 24.0) as i32,
        );
    }

    // The viewer pressed the live button: the message runs to the monk and a
    // frame comes back.
    if let Some(since) = pressed.filter(|since| *since < 1.0) {
        let y = ground_y + 6.0;
        let (panel, ninja, monk) = (
            center_x,
            f64::from(places[2]) * 2.0,
            f64::from(places[0]) * 2.0,
        );
        let out = span(since, 0.0, 0.2);
        let back = span(since, 0.2, 0.55);
        let x = if back > 0.0 {
            lerp(ninja, monk, back)
        } else {
            lerp(panel, ninja, out)
        };
        if since < 0.55 {
            frame.dots.disc((x, y), 2.5, 1.0, AMBER);
            frame.dots.line(
                (x, y),
                (x + if back > 0.0 { 14.0 } else { -14.0 }, y),
                AMBER,
            );
        } else {
            let home = span(since, 0.55, 0.8);
            frame
                .dots
                .disc((lerp(monk, panel, home), y), 2.5, 1.0, TEAL);
            let head = (monk, ground_y - size * 0.66);
            frame.dots.circle(
                head,
                size * 0.36 + (since - 0.55) * 30.0,
                mix(GREEN, BLACK, span(since, 0.55, 1.0)),
            );
        }
    }

    // The line comes back down.
    let drop = score::FINALE_DROP - score::FINALE;
    if u >= drop - 0.2 {
        let fall = ease_out(span(u, drop - 0.2, drop));
        let y = lerp(-8.0, 1.0, fall) as i32;
        title(frame, (w - title_width(heavy)) / 2, y, heavy);
        if u >= drop {
            frame.flash = (1.0 - span(u, drop, drop + 0.5)) * 0.7;
            frame.shake = (0, i32::from(u < drop + 0.1));
        }
    }

    if live {
        let since = t - score::END;
        say_centered(
            frame,
            w / 2,
            ground + 2,
            "YOUR TURN. THIS PANEL IS LIVE: YOUR MOUSE, THE REAL ENGINE.",
            since,
            0.2,
            bold(WHITE),
        );
        say_centered(
            frame,
            w / 2,
            ground + 3,
            "THE FILM KEPT NO ANIMATION STATE. EVERY FRAME WAS f(t): DRAG THE TIMELINE.",
            since,
            1.6,
            fg(GREY),
        );
    } else {
        say_centered(
            frame,
            w / 2,
            ground + 3,
            "RATCN  —  COMPONENTS FOR RATATUI.  COPY THEM.  THEME THEM.  OWN THEM.",
            u,
            4.2,
            bold(WHITE),
        );
    }
    if u >= 6.2 {
        let command = "$ ssh ratcn.com";
        let x = if live {
            w / 2 - command.len() as i32 - 5
        } else {
            w / 2 - command.len() as i32 / 2 - 1
        };
        hud::tag(frame, x, ground + 5, command, GREEN);
    }

    hud::chrome(
        frame,
        t,
        if live {
            "06 // YOUR TURN"
        } else {
            "05 // RATCN"
        },
    );
    frame.snow = 1.0;
    frame.fade = 1.0 - span(u, 0.0, 0.4);
}
