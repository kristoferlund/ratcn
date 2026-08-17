//! The same data as the `barchart` demo, drawn with `horizontal`.
//!
//! A horizontal bar gets a whole row to itself, so its label has room to be a
//! phrase rather than an abbreviation. That is the usual reason to pick this
//! direction over the vertical default.

use std::io;

use ratatui::{
    Frame,
    layout::Constraint,
    style::{Color, Style},
    widgets::Bar,
};
use ratcn::{BarChartWidget, Theme};

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

/// Paint-only: no state to keep and no input to read, so the host draws one
/// frame and then has nothing left to do.
struct Chart;

impl demo_shared::Demo for Chart {
    const INPUT: bool = false;

    fn background(&self) -> Color {
        THEME.background
    }

    fn draw(&mut self, frame: &mut Frame) {
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
}

fn main() -> io::Result<()> {
    demo_shared::run(Chart)
}
