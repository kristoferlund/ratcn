//! How many colors the terminal shows, and the colors brought within that.
//!
//! Themes are solved in rgb, which a terminal without 24-bit color does not
//! ignore: Terminal.app before macOS 26 reads `48;2;10;10;10` as separate
//! attributes, so the `2` dims the text and a channel of 42 paints a green
//! background. Such a terminal is drawn with the nearest colors it has.
//!
//! Nothing in the protocol states the depth, so it is judged the way
//! established libraries do: the environment as termenv and charmbracelet's
//! colorprofile read it, then, where that does not already say 24-bit,
//! Neovim's probe of the terminal itself (see `query`), which can only raise it.

use ratatui::{buffer::Cell, style::Color};

/// The colors a terminal can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ColorDepth {
    TrueColor,
    /// The xterm 256-color palette: 16 themed colors, a 6×6×6 cube and a gray
    /// ramp. Only the cube and the ramp have known channels.
    Ansi256,
    /// The 16 colors the user's theme sets.
    Ansi16,
}

impl ColorDepth {
    /// What the environment says, by termenv's rules with colorprofile's
    /// additions (`foot`, `*direct`, tmux and screen at 256).
    pub(super) fn from_env() -> Self {
        if cfg!(windows) {
            // Every console since Windows 10 shows 24-bit color and sets
            // neither variable, as colorprofile's Windows check finds.
            return Self::TrueColor;
        }
        let var = |name| std::env::var(name).ok();
        Self::detect(
            var("COLORTERM").as_deref(),
            var("TERM").as_deref(),
            var("TERM_PROGRAM").as_deref(),
        )
    }

    /// Whether the terminal is worth asking, with Neovim's probe, for more
    /// than the environment says. Not the Linux console: it shows 16 colors
    /// whatever it is sent, and it may not swallow a DCS it does not know,
    /// which would print the probe's tail on the user's shell.
    pub(super) fn probed() -> bool {
        std::env::var("TERM").map_or(true, |term| term != "linux")
    }

    fn detect(colorterm: Option<&str>, term: Option<&str>, term_program: Option<&str>) -> Self {
        let term = term.unwrap_or_default();
        let colorterm = colorterm.unwrap_or_default().to_ascii_lowercase();
        if colorterm == "truecolor" || colorterm == "24bit" {
            // GNU screen passes on 256 colors at most; tmux, which may run
            // under a screen TERM, says so in TERM_PROGRAM.
            return if term.starts_with("screen") && term_program != Some("tmux") {
                Self::Ansi256
            } else {
                Self::TrueColor
            };
        }
        match term {
            "alacritty" | "contour" | "foot" | "rio" | "wezterm" | "xterm-ghostty"
            | "xterm-kitty" => Self::TrueColor,
            term if term.ends_with("direct") => Self::TrueColor,
            term if term.contains("256color")
                || term.starts_with("tmux")
                || term.starts_with("screen") =>
            {
                Self::Ansi256
            }
            _ => Self::Ansi16,
        }
    }

    /// `color` as this depth shows it: unchanged where it already can.
    fn reduce(self, color: Color) -> Color {
        match (self, color) {
            (Self::Ansi256, Color::Rgb(r, g, b)) => Color::Indexed(nearest_256(r, g, b)),
            (Self::Ansi16, Color::Rgb(r, g, b)) => nearest_16(r, g, b),
            (Self::Ansi16, Color::Indexed(index)) if index >= 16 => {
                let (r, g, b) = channels_256(index);
                nearest_16(r, g, b)
            }
            _ => color,
        }
    }

    pub(super) fn reduce_cell(self, cell: &Cell) -> Cell {
        let mut cell = cell.clone();
        cell.fg = self.reduce(cell.fg);
        cell.bg = self.reduce(cell.bg);
        cell.underline_color = self.reduce(cell.underline_color);
        cell
    }
}

/// The channel values of the xterm cube's six steps.
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The closer of the nearest cube color and the nearest gray, never one of
/// the first 16, whose channels are the user's theme.
fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    let step = |channel: u8| {
        (0u8..6)
            .min_by_key(|&i| CUBE[usize::from(i)].abs_diff(channel))
            .unwrap_or_default()
    };
    let cube = 16 + 36 * step(r) + 6 * step(g) + step(b);

    let mean = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let gray = 232 + u8::try_from(mean.saturating_sub(3) / 10).map_or(23, |ramp| ramp.min(23));

    if distance((r, g, b), channels_256(gray)) < distance((r, g, b), channels_256(cube)) {
        gray
    } else {
        cube
    }
}

/// The channels of a cube or ramp entry of the 256-color palette.
fn channels_256(index: u8) -> (u8, u8, u8) {
    if index >= 232 {
        let level = 8 + 10 * (index - 232);
        return (level, level, level);
    }
    let cube = usize::from(index.saturating_sub(16));
    (CUBE[cube / 36], CUBE[cube / 6 % 6], CUBE[cube % 6])
}

/// Below this much chroma (the largest channel less the smallest) a color is
/// a gray to a 16-color terminal. Every preset's neutrals sit under it and its
/// accents over it.
const CHROMA_MIN: u8 = 64;

/// From this bright a chromatic color takes the light variant of its hue.
const LIGHT_MIN: u8 = 160;

/// The named color for `(r, g, b)`: a gray for a color with little chroma,
/// otherwise the hue's own name. Nearest by plain distance would not do: the
/// named colors are fully saturated, so a theme's pastel red is nearer a gray
/// than any red, and its pink red nearer magenta.
fn nearest_16(r: u8, g: u8, b: u8) -> Color {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let chroma = max - min;
    if chroma < CHROMA_MIN {
        const GRAYS: [(Color, u8); 4] = [
            (Color::Black, 0),
            (Color::DarkGray, 128),
            (Color::Gray, 192),
            (Color::White, 255),
        ];
        return GRAYS
            .into_iter()
            .min_by_key(|&(_, level)| distance((r, g, b), (level, level, level)))
            .map_or(Color::Reset, |(gray, _)| gray);
    }

    // The hue in degrees, then the nearest of the six the named colors sit on.
    let (r, g, b, c) = (i32::from(r), i32::from(g), i32::from(b), i32::from(chroma));
    let hue = if i32::from(max) == r {
        60 * (g - b) / c
    } else if i32::from(max) == g {
        120 + 60 * (b - r) / c
    } else {
        240 + 60 * (r - g) / c
    }
    .rem_euclid(360);
    let (dark, light) = match (hue + 30) / 60 % 6 {
        0 => (Color::Red, Color::LightRed),
        1 => (Color::Yellow, Color::LightYellow),
        2 => (Color::Green, Color::LightGreen),
        3 => (Color::Cyan, Color::LightCyan),
        4 => (Color::Blue, Color::LightBlue),
        _ => (Color::Magenta, Color::LightMagenta),
    };
    if max >= LIGHT_MIN { light } else { dark }
}

fn distance(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let d = |x: u8, y: u8| u32::from(x.abs_diff(y)).pow(2);
    d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2)
}

#[cfg(test)]
mod tests {
    use super::{ColorDepth, channels_256, nearest_256};
    use ratatui::{buffer::Cell, style::Color};

    #[test]
    fn the_environment_is_read_as_termenv_and_colorprofile_read_it() {
        use ColorDepth::{Ansi16, Ansi256, TrueColor};
        let detect = |colorterm, term| ColorDepth::detect(colorterm, term, None);
        assert_eq!(detect(Some("truecolor"), Some("xterm-256color")), TrueColor);
        assert_eq!(detect(Some("24bit"), None), TrueColor);
        assert_eq!(detect(Some("TrueColor"), Some("xterm")), TrueColor);
        // Over SSH COLORTERM is gone, but these terminals always have it.
        for term in [
            "xterm-ghostty",
            "xterm-kitty",
            "alacritty",
            "foot",
            "xterm-direct",
        ] {
            assert_eq!(detect(None, Some(term)), TrueColor, "{term}");
        }
        // GNU screen caps 24-bit at 256; tmux under a screen TERM does not.
        assert_eq!(detect(Some("truecolor"), Some("screen-256color")), Ansi256);
        assert_eq!(
            ColorDepth::detect(Some("truecolor"), Some("screen-256color"), Some("tmux")),
            TrueColor
        );
        // Terminal.app before macOS 26 claims xterm-256color and no more.
        assert_eq!(detect(None, Some("xterm-256color")), Ansi256);
        assert_eq!(detect(None, Some("tmux")), Ansi256);
        assert_eq!(detect(None, Some("screen")), Ansi256);
        assert_eq!(detect(None, Some("xterm")), Ansi16);
        assert_eq!(detect(None, Some("linux")), Ansi16);
        assert_eq!(detect(None, None), Ansi16);
    }

    #[test]
    fn no_rgb_reaches_a_terminal_without_24_bit_color() {
        let mut cell = Cell::default();
        cell.fg = Color::Rgb(250, 250, 250);
        cell.bg = Color::Rgb(10, 10, 10);
        cell.underline_color = Color::Rgb(255, 0, 136);
        for depth in [ColorDepth::Ansi256, ColorDepth::Ansi16] {
            let reduced = depth.reduce_cell(&cell);
            for color in [reduced.fg, reduced.bg, reduced.underline_color] {
                assert!(!matches!(color, Color::Rgb(..)), "{depth:?} kept {color:?}");
            }
        }
        assert_eq!(ColorDepth::TrueColor.reduce_cell(&cell), cell);
    }

    #[test]
    fn the_256_palette_keeps_dark_grays_apart_and_off_the_themed_16() {
        // A theme's background, surface and border sit a few steps apart; they
        // must stay distinct grays, not collapse into one or into black.
        let shades: Vec<u8> = [10, 38, 72, 86]
            .into_iter()
            .map(|level| nearest_256(level, level, level))
            .collect();
        assert!(shades.iter().all(|&index| index >= 232));
        assert!(shades.windows(2).all(|pair| pair[0] < pair[1]));
        // Saturated colors land on the cube, exactly where they are on it.
        assert_eq!(nearest_256(255, 0, 135), 16 + 36 * 5 + 2);
        assert_eq!(channels_256(16 + 36 * 5 + 2), (255, 0, 135));
    }

    /// On 16 colors a theme's meaning has to survive: destructive stays red,
    /// every accent keeps a hue, and text and surfaces stay gray.
    #[test]
    fn sixteen_colors_keep_each_presets_meaning() {
        use crate::Theme;
        let reduce = |color| ColorDepth::Ansi16.reduce(color);
        let grays = [Color::Black, Color::DarkGray, Color::Gray, Color::White];
        for theme in [
            Theme::default_dark(),
            Theme::catppuccin(),
            Theme::gruvbox(),
            Theme::nord(),
            Theme::tokyo_night(),
            Theme::solarized(),
        ] {
            let name = theme.name;
            assert!(
                matches!(reduce(theme.destructive), Color::Red | Color::LightRed),
                "{name}: destructive is {:?}",
                reduce(theme.destructive)
            );
            for (role, color) in [
                ("foreground", theme.foreground),
                ("background", theme.background),
            ] {
                assert!(
                    grays.contains(&reduce(color)),
                    "{name}: {role} is {:?}",
                    reduce(color)
                );
            }
            for (role, color) in [("accent", theme.accent), ("warning", theme.warning)] {
                let reduced = reduce(color);
                let Color::Rgb(r, g, b) = color else { continue };
                if r.max(g).max(b) - r.min(g).min(b) >= super::CHROMA_MIN {
                    assert!(
                        !grays.contains(&reduced),
                        "{name}: {role} lost its hue: {reduced:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn sixteen_colors_take_the_nearest_named_one() {
        let reduce = |color| ColorDepth::Ansi16.reduce(color);
        assert_eq!(reduce(Color::Rgb(10, 10, 10)), Color::Black);
        assert_eq!(reduce(Color::Rgb(250, 250, 250)), Color::White);
        assert_eq!(reduce(Color::Rgb(0, 200, 0)), Color::LightGreen);
        assert_eq!(reduce(Color::Indexed(196)), Color::LightRed);
        assert_eq!(reduce(Color::Rgb(128, 0, 0)), Color::Red);
        // The first 16 are the terminal's own, and pass through.
        assert_eq!(reduce(Color::Indexed(3)), Color::Indexed(3));
        assert_eq!(reduce(Color::Green), Color::Green);
    }
}
