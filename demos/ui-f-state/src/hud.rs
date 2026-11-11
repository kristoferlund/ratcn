//! The film's visual vocabulary: its inks, and the small marks every scene is
//! built from — tags, rules, readouts, snow, speed lines.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph, Widget},
};
use ratcn::{ButtonSize, ButtonWidget, ProgressWidget, Theme};

use crate::canvas::{Frame, Rgb, hash, hex, ink, mix, rnd, span, text_width};

pub const WHITE: Rgb = hex(0xf2f2f2);
pub const GREY: Rgb = hex(0x8c8c8c);
pub const DIM: Rgb = hex(0x4c4c4c);
pub const FAINT: Rgb = hex(0x2a2a2a);
/// State: phosphor green.
pub const GREEN: Rgb = hex(0x9dff3a);
/// Messages: amber.
pub const AMBER: Rgb = hex(0xffb02e);
/// Rendering: teal.
pub const TEAL: Rgb = hex(0x35e0d0);
/// Whatever has gone wrong.
pub const RED: Rgb = hex(0xff4d4f);
pub const BLACK: Rgb = hex(0x0a0a0a);

pub fn fg(color: Rgb) -> Style {
    Style::default().fg(ink(color))
}

pub fn bold(color: Rgb) -> Style {
    fg(color).add_modifier(Modifier::BOLD)
}

/// The first `amount` of `text`, by characters: a typewriter.
pub fn typed(text: &str, amount: f64) -> &str {
    let count = (text.chars().count() as f64 * amount.clamp(0.0, 1.0)).round() as usize;
    text.char_indices()
        .nth(count)
        .map_or(text, |(end, _)| &text[..end])
}

/// A label in a filled box, the film's way of naming things. Returns its width.
pub fn tag(frame: &mut Frame, x: i32, y: i32, text: &str, color: Rgb) -> i32 {
    let label = format!(" {text} ");
    frame.text(
        x,
        y,
        &label,
        Style::default()
            .fg(ink(BLACK))
            .bg(ink(color))
            .add_modifier(Modifier::BOLD),
    );
    text_width(&label)
}

pub fn tag_centered(frame: &mut Frame, center: i32, y: i32, text: &str, color: Rgb) {
    tag(frame, center - (text_width(text) + 2) / 2, y, text, color);
}

/// Corner brackets around a rectangle of cells.
pub fn brackets(frame: &mut Frame, rect: (i32, i32, i32, i32), color: Rgb) {
    let (x, y, w, h) = rect;
    let style = fg(color);
    frame.text(x, y, "┌─", style);
    frame.text(x + w - 2, y, "─┐", style);
    frame.text(x, y + h - 1, "└─", style);
    frame.text(x + w - 2, y + h - 1, "─┘", style);
}

/// A bordered card with a title set into its top edge. Returns the inside.
pub fn card(frame: &mut Frame, rect: Rect, title: &str, color: Rgb) -> Rect {
    let block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(fg(mix(color, BLACK, 0.45)))
        .title(Span::styled(format!(" {title} "), bold(color)))
        .style(Style::default().bg(ink(BLACK)));
    let inner = block.inner(rect);
    block.render(rect, &mut frame.cells);
    inner
}

/// Lines of `(text, color)` runs, as a paragraph.
pub fn code(frame: &mut Frame, rect: Rect, lines: &[Vec<(&str, Rgb)>]) {
    let lines: Vec<Line> = lines
        .iter()
        .map(|runs| {
            Line::from(
                runs.iter()
                    .map(|(text, color)| Span::styled((*text).to_owned(), fg(*color)))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    Paragraph::new(lines).render(rect, &mut frame.cells);
}

/// The instrument frame around every scene: a ruler down the left edge, the
/// film clock, and the name of the chapter.
pub fn chrome(frame: &mut Frame, t: f64, chapter: &str) {
    let (w, h) = (frame.w, frame.h);
    for row in (1..h - 2).step_by(3) {
        let reading = 875 - row * 25 / 3;
        frame.text(0, row, &format!("{reading:>3}"), fg(DIM));
        frame.text(4, row, "╴", fg(DIM));
    }
    frame.text(6, 0, chapter, fg(GREY));
    let clock = format!("T+{:02}:{:05.2}", (t / 60.0) as u32, t % 60.0);
    frame.text(w - text_width(&clock) - 1, 0, &clock, fg(GREY));
    let frame_no = format!("FRAME {:05}  f(t)", (t * 30.0) as u32);
    frame.text(w - text_width(&frame_no) - 1, 1, &frame_no, fg(DIM));
}

/// Snow: fine dots drifting down, `amount` of full strength.
pub fn snow(frame: &mut Frame, t: f64, amount: f64) {
    let (w, h) = (frame.dots.w(), frame.dots.h());
    let flakes = (w * h / 90.0 * amount) as u64;
    for flake in 0..flakes {
        let speed = 5.0 + rnd(flake, 1) * 9.0;
        let sway = (t * (0.4 + rnd(flake, 2)) + rnd(flake, 3) * 6.3).sin() * 3.0;
        let x = (rnd(flake, 4) * w + sway + t * 1.5).rem_euclid(w);
        let y = (rnd(flake, 5) * h + t * speed).rem_euclid(h);
        let color = if speed > 10.0 { WHITE } else { GREY };
        frame.dots.dot(x as i32, y as i32, color);
    }
}

/// Lines converging on a point from every side, redrawn a dozen times a
/// second: the manga speed line.
pub fn speed_lines(frame: &mut Frame, center: (f64, f64), t: f64, amount: f64, color: Rgb) {
    let reach = frame.dots.w().hypot(frame.dots.h());
    let tick = (t * 12.0) as u64;
    for line in 0..(90.0 * amount) as u64 {
        let turn = rnd(line, tick) * std::f64::consts::TAU;
        let inner = reach * (0.16 + 0.22 * rnd(line, tick ^ 7));
        let (dx, dy) = (turn.cos(), turn.sin());
        frame.dots.line(
            (center.0 + dx * inner, center.1 + dy * inner),
            (center.0 + dx * reach, center.1 + dy * reach),
            color,
        );
    }
}

/// The scripted mouse pointer, at a cell.
pub fn pointer(frame: &mut Frame, x: i32, y: i32, pressed: f64) {
    if pressed > 0.0 && pressed < 1.0 {
        let center = (f64::from(x) * 2.0 + 1.0, f64::from(y) * 4.0 + 2.0);
        frame
            .dots
            .circle(center, 2.0 + pressed * 12.0, mix(WHITE, BLACK, pressed));
    }
    frame.text(x, y, "◤", bold(WHITE));
}

/// Horizontal streaks across the whole picture: a tape being wound.
pub fn tracking(frame: &mut Frame, t: f64, amount: f64) {
    let tick = (t * 24.0) as u64;
    for band in 0..(amount * 7.0).ceil() as u64 {
        let y = rnd(band, tick) * frame.dots.h();
        let from = rnd(band, tick ^ 5) * frame.dots.w() * 0.5;
        frame.dots.dashed(
            (from, y),
            (frame.dots.w(), y),
            GREY,
            9,
            4,
            (hash(band) % 13) as i32,
        );
    }
}

/// A fade from black at the start of a scene and back to it at the end.
pub fn dip(frame: &mut Frame, local: f64, length: f64) {
    frame.fade =
        frame
            .fade
            .max(1.0 - span(local, 0.0, 0.3))
            .max(span(local, length - 0.25, length));
}

pub const PANEL_W: u16 = 30;
pub const PANEL_H: u16 = 10;

/// Everything of the counter panel but its button. Painted through a widget so
/// the film's scripted frames and the live engine at the end draw the same UI.
pub struct Panel<'a> {
    pub count: u32,
    pub theme: &'a Theme,
}

impl Panel<'_> {
    /// Where the `+ 1` button sits inside `area`.
    pub fn button(area: Rect) -> Rect {
        Rect::new(
            area.x + 2,
            area.y + 6,
            ButtonWidget::new(BUTTON_LABEL)
                .size(ButtonSize::Large)
                .width(),
            ButtonSize::Large.height(),
        )
    }
}

pub const BUTTON_LABEL: &str = "+ 1";
/// The count the progress bar calls full.
pub const FULL: u32 = 12;

impl Widget for Panel<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let theme = self.theme;
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border))
            .title(Span::styled(
                " counter ",
                Style::default().fg(theme.muted_foreground),
            ))
            .style(Style::default().bg(theme.surface).fg(theme.foreground));
        let inner = block.inner(area).inner(ratatui::layout::Margin::new(1, 0));
        // The panel is opaque: whatever was drawn here is behind it.
        Clear.render(area, buffer);
        block.render(area, buffer);
        if inner.height < 4 {
            return;
        }
        Line::from("count")
            .style(Style::default().fg(theme.muted_foreground))
            .render(inner, buffer);
        Line::from(self.count.to_string())
            .style(
                Style::default()
                    .fg(theme.foreground)
                    .add_modifier(Modifier::BOLD),
            )
            .right_aligned()
            .render(inner, buffer);
        ProgressWidget::new(f64::from(self.count) / f64::from(FULL))
            .label("to twelve")
            .show_value(true)
            .themed(theme)
            .render(Rect::new(inner.x, inner.y + 2, inner.width, 2), buffer);
    }
}

/// The whole panel, button included, as a function of what it shows.
pub fn panel(buffer: &mut Buffer, area: Rect, count: u32, hovered: bool, theme: &Theme) {
    Panel { count, theme }.render(area, buffer);
    ButtonWidget::new(BUTTON_LABEL)
        .size(ButtonSize::Large)
        .themed(theme)
        .hovered(hovered)
        .render(Panel::button(area), buffer);
}

/// The panel rendered off to one side, for [`Frame::stamp`].
pub fn panel_buffer(count: u32, hovered: bool, theme: &Theme) -> Buffer {
    let area = Rect::new(0, 0, PANEL_W, PANEL_H);
    let mut buffer = Buffer::empty(area);
    panel(&mut buffer, area, count, hovered, theme);
    buffer
}
