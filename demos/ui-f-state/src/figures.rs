//! The cast, and the film's lettering.
//!
//! Nothing here is a bitmap. Each figure is a function from a point to a tone,
//! sampled once per braille dot: outlines where two shapes meet, ordered
//! dithering inside. That is what lets one drawing fill a portrait at one
//! moment and stand a few rows tall on a horizon the next, and what lets a
//! scarf move.

use crate::canvas::{Dots, Rgb};

/// What a figure is at one point: which shape, how bright, and whether it
/// takes the accent color.
type Tone = Option<(u8, f64, bool)>;

/// Where a figure stands, in dots.
#[derive(Clone, Copy)]
pub struct Pose {
    /// The middle of the figure.
    pub x: f64,
    /// Where its feet are.
    pub base: f64,
    /// Its height.
    pub size: f64,
    /// How far the head leans ahead of the feet, in heights.
    pub lean: f64,
    /// Nothing below this row is drawn: a scan revealing the figure.
    pub cut: f64,
}

impl Pose {
    pub const fn new(x: f64, base: f64, size: f64) -> Self {
        Self {
            x,
            base,
            size,
            lean: 0.0,
            cut: f64::MAX,
        }
    }
}

fn ellipse(u: f64, v: f64, cx: f64, cy: f64, rx: f64, ry: f64) -> bool {
    ((u - cx) / rx).powi(2) + ((v - cy) / ry).powi(2) <= 1.0
}

/// A line with rounded ends and a thickness.
fn capsule(u: f64, v: f64, a: (f64, f64), b: (f64, f64), radius: f64) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let along = (((u - a.0) * dx + (v - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (u - a.0 - dx * along).hypot(v - a.1 - dy * along) <= radius
}

/// A convex polygon, its corners listed clockwise on screen.
fn polygon(u: f64, v: f64, corners: &[(f64, f64)]) -> bool {
    (0..corners.len()).all(|index| {
        let (a, b) = (corners[index], corners[(index + 1) % corners.len()]);
        (b.0 - a.0) * (v - a.1) - (b.1 - a.1) * (u - a.0) >= 0.0
    })
}

/// Sample `tone` over the figure's box and put it on the dots. `wide` is the
/// box's width in heights, centered on the pose.
fn draw(
    dots: &mut Dots,
    pose: Pose,
    wide: f64,
    ink: Rgb,
    accent: Rgb,
    tone: impl Fn(f64, f64) -> Tone,
) {
    let size = pose.size;
    let left = pose.x - size * wide / 2.0;
    let top = pose.base - size;
    let at = |dx: i32, dy: i32| {
        let v = f64::from(dy) / size;
        let u = f64::from(dx) / size - (wide - 1.0) / 2.0 - pose.lean * (1.0 - v);
        tone(u, v)
    };
    for dy in 0..size as i32 {
        if top + f64::from(dy) > pose.cut {
            break;
        }
        for dx in 0..(size * wide) as i32 {
            let Some((id, level, accented)) = at(dx, dy) else {
                continue;
            };
            let color = if accented { accent } else { ink };
            let (x, y) = ((left + f64::from(dx)) as i32, (top + f64::from(dy)) as i32);
            let shape = |tone: Tone| tone.map(|(id, ..)| id);
            let edge = shape(at(dx + 1, dy)) != Some(id)
                || shape(at(dx, dy + 1)) != Some(id)
                || shape(at(dx - 1, dy)) != Some(id)
                || shape(at(dx, dy - 1)) != Some(id);
            if edge && level > 0.0 {
                dots.dot(x, y, color);
            } else {
                dots.shade(x, y, level, color);
            }
        }
    }
}

/// State: a monk under a straw hat, seated, hands folded.
pub fn monk(dots: &mut Dots, pose: Pose, ink: Rgb, accent: Rgb) {
    draw(dots, pose, 1.0, ink, accent, |u, v| {
        if ellipse(u, v, 0.5, 0.75, 0.09, 0.045) {
            Some((5, 0.85, false))
        } else if polygon(u, v, &[(0.5, 0.0), (0.98, 0.27), (0.02, 0.27)]) {
            // Straw: bands across the cone.
            let band = ((v * 34.0) as i32) % 2 == 0;
            Some((1, if band { 0.7 } else { 0.38 }, true))
        } else if ellipse(u, v, 0.5, 0.275, 0.48, 0.035) {
            Some((2, 1.0, true))
        } else if ellipse(u, v, 0.5, 0.37, 0.115, 0.1) {
            // Closed eyes: two short lines in an unlit face.
            let eye =
                (v - 0.375).abs() < 0.012 && ((u - 0.455).abs() < 0.03 || (u - 0.545).abs() < 0.03);
            Some((if eye { 9 } else { 3 }, if eye { 1.0 } else { 0.1 }, false))
        } else if polygon(u, v, &[(0.44, 0.46), (0.56, 0.46), (0.5, 0.62)]) {
            Some((6, 0.0, false))
        } else if polygon(
            u,
            v,
            &[(0.35, 0.46), (0.65, 0.46), (0.8, 0.84), (0.2, 0.84)],
        ) {
            Some((4, 0.3, false))
        } else if ellipse(u, v, 0.25, 0.7, 0.13, 0.17) || ellipse(u, v, 0.75, 0.7, 0.13, 0.17) {
            Some((7, 0.45, false))
        } else if ellipse(u, v, 0.5, 0.88, 0.49, 0.115) {
            Some((8, 0.2, false))
        } else {
            None
        }
    });
}

/// Render: a rat with a headband and a brush taller than it is.
pub fn rat(dots: &mut Dots, pose: Pose, ink: Rgb, accent: Rgb, t: f64) {
    let wag = (t * 3.0).sin() * 0.05;
    let flutter = (t * 7.0).sin() * 0.025;
    draw(dots, pose, 1.3, ink, accent, |u, v| {
        if polygon(
            u,
            v,
            &[(0.28, 0.2), (0.72, 0.2), (0.72, 0.245), (0.28, 0.245)],
        ) || capsule(u, v, (0.72, 0.22), (0.93, 0.27 + flutter), 0.014)
            || capsule(u, v, (0.72, 0.23), (0.9, 0.35 - flutter), 0.014)
        {
            Some((9, 1.0, true))
        } else if ellipse(u, v, 0.41, 0.31, 0.035, 0.038) || ellipse(u, v, 0.59, 0.31, 0.035, 0.038)
        {
            Some((10, 0.0, false))
        } else if ellipse(u, v, 0.5, 0.42, 0.03, 0.022) {
            Some((11, 1.0, true))
        } else if capsule(u, v, (0.36, 0.4), (0.2, 0.37), 0.004)
            || capsule(u, v, (0.36, 0.42), (0.2, 0.44), 0.004)
            || capsule(u, v, (0.64, 0.4), (0.8, 0.37), 0.004)
            || capsule(u, v, (0.64, 0.42), (0.8, 0.44), 0.004)
        {
            Some((14, 1.0, false))
        } else if ellipse(u, v, 0.24, 0.11, 0.065, 0.06) || ellipse(u, v, 0.76, 0.11, 0.065, 0.06) {
            Some((2, 0.12, false))
        } else if ellipse(u, v, 0.24, 0.11, 0.12, 0.11) || ellipse(u, v, 0.76, 0.11, 0.12, 0.11) {
            Some((1, 0.5, false))
        } else if ellipse(u, v, 0.5, 0.29, 0.25, 0.16)
            || polygon(u, v, &[(0.34, 0.36), (0.66, 0.36), (0.5, 0.48)])
        {
            Some((3, 0.4, false))
        } else if ellipse(u, v, 0.885, 0.2, 0.035, 0.085) {
            Some((13, 0.9, true))
        } else if capsule(u, v, (0.95, 0.97), (0.89, 0.28), 0.011) {
            Some((12, 1.0, false))
        } else if capsule(u, v, (0.34, 0.56), (0.22, 0.7), 0.045)
            || capsule(u, v, (0.66, 0.56), (0.9, 0.62), 0.045)
        {
            Some((6, 0.36, false))
        } else if ellipse(u, v, 0.5, 0.71, 0.12, 0.16) {
            Some((5, 0.72, false))
        } else if ellipse(u, v, 0.5, 0.69, 0.21, 0.24) {
            Some((4, 0.3, false))
        } else if ellipse(u, v, 0.38, 0.955, 0.09, 0.04) || ellipse(u, v, 0.62, 0.955, 0.09, 0.04) {
            Some((7, 0.6, false))
        } else if capsule(u, v, (0.32, 0.86), (0.14, 0.92), 0.013)
            || capsule(u, v, (0.14, 0.92), (0.03, 0.82), 0.013)
            || capsule(u, v, (0.03, 0.82), (0.07 + wag, 0.64), 0.013)
        {
            Some((8, 1.0, false))
        } else {
            None
        }
    });
}

/// Message: a ninja, sword on the back, scarf in the wind.
pub fn ninja(dots: &mut Dots, pose: Pose, ink: Rgb, accent: Rgb, t: f64) {
    draw(dots, pose, 1.7, ink, accent, |u, v| {
        // The scarf streams off to the right, waving more the further out.
        let out = u - 0.56;
        let ribbon = |offset: f64, phase: f64| {
            out > 0.0
                && out < 0.78
                && (v - offset - (out * 13.0 - t * 7.0 + phase).sin() * out * 0.11 - out * 0.05)
                    .abs()
                    < 0.03 - out * 0.018
        };
        if ellipse(u, v, 0.455, 0.16, 0.02, 0.02) || ellipse(u, v, 0.545, 0.16, 0.02, 0.02) {
            Some((10, 0.0, false))
        } else if polygon(
            u,
            v,
            &[(0.395, 0.13), (0.605, 0.13), (0.605, 0.19), (0.395, 0.19)],
        ) {
            Some((2, 1.0, false))
        } else if ribbon(0.31, 0.0)
            || ribbon(0.33, 1.9)
            || polygon(
                u,
                v,
                &[(0.4, 0.285), (0.6, 0.285), (0.6, 0.34), (0.4, 0.34)],
            )
        {
            Some((3, 1.0, true))
        } else if ellipse(u, v, 0.5, 0.17, 0.115, 0.135) {
            Some((1, 0.3, false))
        } else if capsule(u, v, (0.22, 0.06), (0.4, 0.34), 0.013) {
            Some((11, 1.0, false))
        } else if capsule(u, v, (0.33, 0.4), (0.25, 0.63), 0.045)
            || capsule(u, v, (0.67, 0.4), (0.75, 0.63), 0.045)
        {
            Some((5, 0.2, false))
        } else if polygon(
            u,
            v,
            &[
                (0.365, 0.575),
                (0.635, 0.575),
                (0.635, 0.615),
                (0.365, 0.615),
            ],
        ) {
            Some((6, 1.0, false))
        } else if polygon(
            u,
            v,
            &[(0.33, 0.34), (0.67, 0.34), (0.63, 0.62), (0.37, 0.62)],
        ) {
            Some((4, 0.34, false))
        } else if capsule(u, v, (0.44, 0.65), (0.35, 0.95), 0.055)
            || capsule(u, v, (0.56, 0.65), (0.65, 0.95), 0.055)
        {
            Some((7, 0.26, false))
        } else {
            None
        }
    });
}

/// A full moon: dithered, lit from the upper left, cratered.
pub fn moon(dots: &mut Dots, center: (f64, f64), radius: f64, color: Rgb) {
    let r = radius as i32;
    for y in -r..=r {
        for x in -r..=r {
            let (u, v) = (f64::from(x) / radius, f64::from(y) / radius);
            let d = u.hypot(v);
            if d > 1.0 {
                continue;
            }
            let crater = [
                (-0.3, -0.25, 0.22),
                (0.35, 0.1, 0.3),
                (-0.1, 0.5, 0.18),
                (0.45, -0.5, 0.12),
            ]
            .iter()
            .any(|(cx, cy, cr)| (u - cx).hypot(v - cy) < *cr);
            let light = 0.34 - 0.2 * (u * 0.6 + v * 0.8) - if crater { 0.14 } else { 0.0 };
            let (px, py) = (center.0 as i32 + x, center.1 as i32 + y);
            if d > 1.0 - 1.5 / radius {
                dots.dot(px, py, color);
            } else {
                dots.shade(px, py, light, color);
            }
        }
    }
}

/// A torii gate, `size` dots tall, standing on `base`.
pub fn torii(dots: &mut Dots, x: f64, base: f64, size: f64, color: Rgb) {
    let top = base - size;
    let mut bar = |x0: f64, y0: f64, x1: f64, y1: f64, level: f64| {
        for py in (top + y0 * size) as i32..(top + y1 * size) as i32 {
            for px in (x + x0 * size) as i32..(x + x1 * size) as i32 {
                dots.shade(px, py, level, color);
            }
        }
    };
    bar(-0.62, 0.0, 0.62, 0.09, 1.0);
    bar(-0.54, 0.09, 0.54, 0.15, 0.5);
    bar(-0.5, 0.3, 0.5, 0.37, 1.0);
    bar(-0.4, 0.15, -0.31, 1.0, 0.75);
    bar(0.31, 0.15, 0.4, 1.0, 0.75);
    bar(-0.04, 0.15, 0.04, 0.3, 0.75);
}

/// Rows of a 5×7 letter, top first, the high bit of five on the left.
const fn glyph(ch: char) -> [u8; 7] {
    match ch {
        'u' => [0, 0, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101],
        'i' => [0b00100, 0, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110],
        '=' => [0, 0, 0b11111, 0, 0b11111, 0, 0],
        'f' => [
            0b00110, 0b01001, 0b01000, 0b11100, 0b01000, 0b01000, 0b01000,
        ],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        's' => [0, 0, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110],
        't' => [
            0b01000, 0b01000, 0b11100, 0b01000, 0b01000, 0b01001, 0b00110,
        ],
        'a' => [0, 0, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111],
        'e' => [0, 0, 0b01110, 0b10001, 0b11111, 0b10000, 0b01110],
        _ => [0; 7],
    }
}

pub const TITLE: &str = "ui = f(state)";
pub const TITLE_H: i32 = 7;

/// How wide the title is drawn `bold` or not.
pub const fn title_width(bold: bool) -> i32 {
    TITLE.len() as i32 * if bold { 7 } else { 6 } - 1
}

/// Every lit cell of the title, as `(column, row)`. `bold` doubles every
/// stroke sideways, which is what a cell twice as tall as it is wide needs for
/// its uprights to weigh what its crossbars do.
pub fn title_cells(bold: bool) -> Vec<(i32, i32)> {
    let mut cells = Vec::new();
    for (index, ch) in TITLE.chars().enumerate() {
        let rows = glyph(ch);
        for row in 0..7 {
            let bits = u16::from(rows[row as usize]);
            let bits = if bold { bits << 1 | bits } else { bits << 1 };
            for column in 0..6 {
                if bits >> (5 - column) & 1 == 1 {
                    cells.push((index as i32 * if bold { 7 } else { 6 } + column, row));
                }
            }
        }
    }
    cells
}
