//! The same data as the `barchart` demo, drawn with `horizontal`.
//!
//! A horizontal bar gets a whole row to itself, so its label has room to be a
//! phrase rather than an abbreviation. That is the usual reason to pick this
//! direction over the vertical default.

use std::io;

use ratatui::{layout::Constraint, style::Style, widgets::Bar};
use ratcn::{BarChartWidget, Theme};

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;

#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

const THEME: Theme = Theme::default_dark();
const DATA: [(&str, u64); 5] = [
    ("Documentation", 12),
    ("Bug fixes", 18),
    ("Refactoring", 9),
    ("New features", 21),
    ("Code review", 15),
];
/// A horizontal bar is one row tall, so the chart is as tall as it has bars.
const BAR_HEIGHT: u16 = 1;
const BAR_GAP: u16 = 0;
const CHART_WIDTH: u16 = 48;
const CHART_HEIGHT: u16 = DATA.len() as u16 * BAR_HEIGHT + (DATA.len() as u16 - 1) * BAR_GAP;

fn bars() -> Vec<Bar<'static>> {
    DATA.into_iter()
        .map(|(label, value)| Bar::default().label(label).value(value))
        .collect()
}

fn draw(frame: &mut ratatui::Frame) {
    let area = frame.area();
    frame
        .buffer_mut()
        .set_style(area, Style::default().bg(THEME.background));

    let chart_area = area.centered(
        Constraint::Length(CHART_WIDTH),
        Constraint::Length(CHART_HEIGHT),
    );

    frame.render_widget(
        BarChartWidget::horizontal(bars())
            .themed(&THEME)
            .max_value(24)
            .bar_width(BAR_HEIGHT)
            .bar_gap(BAR_GAP),
        chart_area,
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> io::Result<()> {
    ratatui::run(|terminal| {
        loop {
            terminal.draw(draw)?;
            if demo_shared::is_quit(&event::read()?) {
                break Ok(());
            }
        }
    })
}

#[cfg(target_arch = "wasm32")]
fn main() -> io::Result<()> {
    let backend = demo_shared::web_backend(THEME.background)?;
    let terminal = ratatui::Terminal::new(backend)?;
    terminal.draw_web(draw);
    Ok(())
}
