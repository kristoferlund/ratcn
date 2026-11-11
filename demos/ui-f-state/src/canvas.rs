//! What a frame of the film is painted on.
//!
//! Two layers, composed into the host's buffer at the end of the frame: a grid
//! of braille dots for line art, eight to a cell, and an ordinary cell buffer
//! for text and widgets. Cells win where both were painted, so a card laid over
//! a drawing hides the lines behind it.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
};

pub type Rgb = [u8; 3];

pub const fn hex(value: u32) -> Rgb {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

pub const fn ink(color: Rgb) -> Color {
    Color::Rgb(color[0], color[1], color[2])
}

pub fn mix(a: Rgb, b: Rgb, amount: f64) -> Rgb {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * amount) as u8;
    [
        channel(a[0], b[0]),
        channel(a[1], b[1]),
        channel(a[2], b[2]),
    ]
}

/// How far `t` has come from `from` to `to`, clamped to `0.0..=1.0`.
pub fn span(t: f64, from: f64, to: f64) -> f64 {
    ((t - from) / (to - from)).clamp(0.0, 1.0)
}

pub fn lerp(a: f64, b: f64, amount: f64) -> f64 {
    a + (b - a) * amount
}

/// Fast at first, settling at the end.
pub fn ease_out(x: f64) -> f64 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// Slow at both ends.
pub fn ease_in_out(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// SplitMix64. Stands in for an RNG, so every frame is the same function of
/// the clock on every platform.
pub fn hash(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A repeatable number in `0.0..1.0` for the pair `(a, b)`.
pub fn rnd(a: u64, b: u64) -> f64 {
    (hash(a ^ hash(b)) >> 11) as f64 / (1u64 << 53) as f64
}

/// The 4×4 ordered-dither threshold at a dot, in `0.0..1.0`.
fn bayer(x: i32, y: i32) -> f64 {
    const M: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    (f64::from(M[y.rem_euclid(4) as usize][x.rem_euclid(4) as usize]) + 0.5) / 16.0
}

/// The braille layer: 2×4 dots per cell, one color per cell.
pub struct Dots {
    cols: i32,
    rows: i32,
    bits: Vec<u8>,
    tint: Vec<Rgb>,
}

impl Dots {
    fn new(cols: u16, rows: u16) -> Self {
        let cells = usize::from(cols) * usize::from(rows);
        Self {
            cols: i32::from(cols),
            rows: i32::from(rows),
            bits: vec![0; cells],
            tint: vec![[0; 3]; cells],
        }
    }

    /// Width in dots.
    pub const fn w(&self) -> f64 {
        (self.cols * 2) as f64
    }

    /// Height in dots.
    pub const fn h(&self) -> f64 {
        (self.rows * 4) as f64
    }

    fn slot(&self, x: i32, y: i32) -> Option<(usize, u8)> {
        if x < 0 || y < 0 || x >= self.cols * 2 || y >= self.rows * 4 {
            return None;
        }
        // Braille numbers its dots down the left column, then the right, with
        // the bottom row added later as bits 6 and 7.
        const BIT: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
        let cell = (y / 4 * self.cols + x / 2) as usize;
        Some((cell, BIT[(y % 4) as usize][(x % 2) as usize]))
    }

    pub fn dot(&mut self, x: i32, y: i32, color: Rgb) {
        if let Some((cell, bit)) = self.slot(x, y) {
            self.bits[cell] |= bit;
            self.tint[cell] = color;
        }
    }

    pub fn erase(&mut self, x: i32, y: i32) {
        if let Some((cell, bit)) = self.slot(x, y) {
            self.bits[cell] &= !bit;
        }
    }

    /// A dot that is on for `level` of the area it tiles: ordered dithering.
    pub fn shade(&mut self, x: i32, y: i32, level: f64, color: Rgb) {
        if bayer(x, y) < level {
            self.dot(x, y, color);
        } else {
            self.erase(x, y);
        }
    }

    pub fn line(&mut self, from: (f64, f64), to: (f64, f64), color: Rgb) {
        self.dashed(from, to, color, 1, 0, 0);
    }

    /// A line of `on` dots then `off` dots, the pattern shifted by `phase`.
    pub fn dashed(
        &mut self,
        from: (f64, f64),
        to: (f64, f64),
        color: Rgb,
        on: i32,
        off: i32,
        phase: i32,
    ) {
        let steps = (to.0 - from.0)
            .abs()
            .max((to.1 - from.1).abs())
            .ceil()
            .max(1.0) as i32;
        // A line that long is off the screen for nearly all of it.
        if steps > 4_000 {
            return;
        }
        for step in 0..=steps {
            if (step + phase).rem_euclid(on + off) >= on {
                continue;
            }
            let amount = f64::from(step) / f64::from(steps);
            self.dot(
                lerp(from.0, to.0, amount).round() as i32,
                lerp(from.1, to.1, amount).round() as i32,
                color,
            );
        }
    }

    /// The part of a circle between two angles, in turns, clockwise from east.
    pub fn arc(&mut self, center: (f64, f64), radius: f64, from: f64, to: f64, color: Rgb) {
        let steps = (radius * std::f64::consts::TAU * (to - from).abs())
            .ceil()
            .max(1.0) as i32;
        for step in 0..=steps {
            let turn = lerp(from, to, f64::from(step) / f64::from(steps)) * std::f64::consts::TAU;
            self.dot(
                (center.0 + radius * turn.cos()).round() as i32,
                (center.1 + radius * turn.sin()).round() as i32,
                color,
            );
        }
    }

    pub fn circle(&mut self, center: (f64, f64), radius: f64, color: Rgb) {
        self.arc(center, radius, 0.0, 1.0, color);
    }

    /// A filled disc, dithered to `level`.
    pub fn disc(&mut self, center: (f64, f64), radius: f64, level: f64, color: Rgb) {
        let r = radius.ceil() as i32;
        let (cx, cy) = (center.0.round() as i32, center.1.round() as i32);
        for y in -r..=r {
            for x in -r..=r {
                if f64::from(x * x + y * y) <= radius * radius {
                    self.shade(cx + x, cy + y, level, color);
                }
            }
        }
    }
}

/// A cell nothing has painted. No widget writes it, so a cell still holding it
/// after a scene has run is one the dots may show through.
const VOID: &str = "\u{10FFFF}";

/// One frame under construction, and what is done to it as a whole.
pub struct Frame {
    pub cells: Buffer,
    pub dots: Dots,
    /// Size in cells.
    pub w: i32,
    pub h: i32,
    /// Cells the finished frame is displaced by.
    pub shake: (i32, i32),
    /// How far every color is pushed toward white, `0.0..=1.0`.
    pub flash: f64,
    /// How far every color is pushed toward the background.
    pub fade: f64,
    /// Swap ink and paper: the manga impact frame.
    pub invert: bool,
    /// The share of cells replaced by character noise.
    pub glitch: f64,
    /// How hard it is snowing, `0.0..=1.0`. The snow falls over the whole
    /// terminal, not only the scene, so the scene only says how much.
    pub snow: f64,
    /// A color for everything outside the scene, in place of the paper.
    pub surround: Option<Rgb>,
}

impl Frame {
    pub fn new(width: u16, height: u16) -> Self {
        let mut frame = Self {
            cells: Buffer::empty(Rect::new(0, 0, width, height)),
            dots: Dots::new(width, height),
            w: i32::from(width),
            h: i32::from(height),
            shake: (0, 0),
            flash: 0.0,
            fade: 0.0,
            invert: false,
            glitch: 0.0,
            snow: 0.0,
            surround: None,
        };
        frame.clear();
        frame
    }

    pub fn clear(&mut self) {
        for cell in &mut self.cells.content {
            cell.reset();
            cell.set_symbol(VOID);
        }
        self.dots.bits.fill(0);
        self.shake = (0, 0);
        self.flash = 0.0;
        self.fade = 0.0;
        self.invert = false;
        self.glitch = 0.0;
        self.snow = 0.0;
        self.surround = None;
    }

    pub const fn area(&self) -> Rect {
        self.cells.area
    }

    /// `rect` clipped to the frame, for handing to a widget.
    pub fn clip(&self, x: i32, y: i32, width: i32, height: i32) -> Rect {
        let left = x.clamp(0, self.w);
        let top = y.clamp(0, self.h);
        let right = (x + width).clamp(left, self.w);
        let bottom = (y + height).clamp(top, self.h);
        Rect::new(
            left as u16,
            top as u16,
            (right - left) as u16,
            (bottom - top) as u16,
        )
    }

    /// Write `text` at a cell, clipped to the frame on every side.
    pub fn text(&mut self, x: i32, y: i32, text: &str, style: Style) {
        if y < 0 || y >= self.h {
            return;
        }
        let mut column = x;
        for ch in text.chars() {
            let width = cell_width(ch);
            if column >= 0 && column + width <= self.w {
                let mut bytes = [0; 4];
                self.cells
                    .set_string(column as u16, y as u16, ch.encode_utf8(&mut bytes), style);
            }
            column += width;
        }
    }

    /// Copy the cells of `source` for which `keep` holds, its top left at
    /// `(x, y)`. This is how a widget rendered off to one side slides, wipes,
    /// or deals onto the frame.
    pub fn stamp(&mut self, source: &Buffer, x: i32, y: i32, keep: impl Fn(u16, u16) -> bool) {
        let area = source.area;
        for row in 0..area.height {
            for column in 0..area.width {
                let (tx, ty) = (x + i32::from(column), y + i32::from(row));
                if tx < 0 || ty < 0 || tx >= self.w || ty >= self.h || !keep(column, row) {
                    continue;
                }
                self.cells[(tx as u16, ty as u16)] =
                    source[(area.x + column, area.y + row)].clone();
            }
        }
    }

    /// Paint the finished frame into `out`, its top left at `area`'s. With
    /// `over`, cells the frame left alone keep what `out` already shows.
    pub fn compose(&self, out: &mut Buffer, area: Rect, paper: Rgb, over: bool, seed: u64) {
        let ground = ink(self.surround.unwrap_or(paper));
        for y in 0..self.h {
            for x in 0..self.w {
                let (sx, sy) = (x - self.shake.0, y - self.shake.1);
                let inside = sx >= 0 && sy >= 0 && sx < self.w && sy < self.h;
                let index = if inside {
                    (sy * self.w + sx) as usize
                } else {
                    0
                };
                let cell = &self.cells.content[index];
                let painted = inside && (cell.symbol() != VOID || cell.bg != Color::Reset);
                let dotted = inside && self.dots.bits[index] != 0;
                let roll = hash(seed ^ (((y * self.w + x) as u64) << 20));
                let noisy = self.glitch > 0.0 && rnd(roll, 1) < self.glitch;
                if over && !painted && !dotted && !noisy {
                    continue;
                }
                let Some(target) = out.cell_mut((area.x + x as u16, area.y + y as u16)) else {
                    continue;
                };
                target.reset();
                target.bg = ground;
                if painted {
                    *target = cell.clone();
                    if cell.symbol() == VOID {
                        target.set_symbol(" ");
                    }
                    if target.bg == Color::Reset {
                        target.bg = ground;
                    }
                } else if dotted {
                    let braille = char::from_u32(0x2800 + u32::from(self.dots.bits[index]))
                        .expect("every braille pattern is a character");
                    target.set_char(braille);
                    target.fg = ink(self.dots.tint[index]);
                }
                if noisy {
                    const NOISE: &[u8] = b"#%&@$0O/\\|=+-_.,:;(){}[]<>7?!*";
                    target.set_char(NOISE[(roll >> 8) as usize % NOISE.len()] as char);
                    target.fg = ink(NOISE_INKS[(roll >> 20) as usize % NOISE_INKS.len()]);
                }
                self.grade(target, paper);
            }
        }
    }

    /// The whole-frame color treatment, one cell at a time.
    fn grade(&self, cell: &mut ratatui::buffer::Cell, paper: Rgb) {
        if !self.invert && self.flash <= 0.0 && self.fade <= 0.0 {
            return;
        }
        let grade = |color: Color, default: Rgb| {
            let mut rgb = match color {
                Color::Rgb(r, g, b) => [r, g, b],
                _ => default,
            };
            if self.invert {
                rgb = [255 - rgb[0], 255 - rgb[1], 255 - rgb[2]];
            }
            rgb = mix(rgb, [255; 3], self.flash);
            ink(mix(rgb, paper, self.fade))
        };
        cell.fg = grade(cell.fg, [242; 3]);
        cell.bg = grade(cell.bg, paper);
    }
}

/// The dusty tans character noise is printed in.
const NOISE_INKS: [Rgb; 4] = [hex(0xc8a878), hex(0x8a6f4a), hex(0xe8e0d0), hex(0x5a5a5a)];

/// Columns a character takes. Rough, and enough for the film's own strings:
/// everything from Hangul Jamo up is treated as double width.
const fn cell_width(ch: char) -> i32 {
    if ch as u32 >= 0x1100 && !(ch as u32 >= 0x2000 && (ch as u32) < 0x3000) && (ch as u32) < 0xE000
        || ch as u32 >= 0xFF00 && (ch as u32) < 0xFF60
    {
        2
    } else {
        1
    }
}

pub fn text_width(text: &str) -> i32 {
    text.chars().map(cell_width).sum()
}
