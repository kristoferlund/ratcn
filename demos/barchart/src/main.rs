//! `BarChartWidget` is paint-only: no focus, no events, no runtime.
//!
//! So this demo has no `Ratcn` and no message type — it draws one chart and
//! waits for a quit key.

use std::io;

use ratatui::{layout::Constraint, style::Style, widgets::Bar};
use ratcn::{BarChartWidget, Theme};

#[cfg(not(target_arch = "wasm32"))]
use ratatui::crossterm::event;

#[cfg(target_arch = "wasm32")]
use ratzilla::WebRenderer;

const THEME: Theme = Theme::default_dark();
const DATA: [(&str, u64); 5] = [
    ("Mon", 12),
    ("Tue", 18),
    ("Wed", 9),
    ("Thu", 21),
    ("Fri", 15),
];
const BAR_WIDTH: u16 = 6;
const BAR_GAP: u16 = 2;
/// Exactly what the bars occupy, so the chart's background ends where the bars
/// do instead of trailing off to the right of the last one.
const CHART_WIDTH: u16 = DATA.len() as u16 * BAR_WIDTH + (DATA.len() as u16 - 1) * BAR_GAP;
const CHART_HEIGHT: u16 = 13;

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
        BarChartWidget::new(bars())
            .themed(&THEME)
            // Pinned so the chart keeps one scale instead of rescaling to
            // whichever bar happens to be tallest.
            .max_value(24)
            .bar_width(BAR_WIDTH)
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
    // Nothing here reacts to input, so there is no event wiring and no
    // `Rc<RefCell<..>>` — `draw_web` takes the terminal and drives the frames.
    terminal.draw_web(draw);
    Ok(())
}
