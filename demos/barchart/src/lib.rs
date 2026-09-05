//! `BarChartWidget` is paint-only: no focus, no events, no runtime.
//!
//! So this demo has no `Ratcn` and no message type — it draws one chart and
//! waits for a quit key.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Style,
    widgets::Bar,
};
use ratcn::{BarChartWidget, Theme};

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

/// Paint-only: no state to keep and no input to read, so the host draws one
/// frame and then has nothing left to do.
pub struct Chart;

impl demo_shared::Demo for Chart {
    const INPUT: bool = false;

    fn draw(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(theme.background));

        let chart_area = area.centered(
            Constraint::Length(CHART_WIDTH),
            Constraint::Length(CHART_HEIGHT),
        );

        frame.render_widget(
            BarChartWidget::new(bars())
                .themed(theme)
                // Pinned so the chart keeps one scale instead of rescaling to
                // whichever bar happens to be tallest.
                .max_value(24)
                .bar_width(BAR_WIDTH)
                .bar_gap(BAR_GAP),
            chart_area,
        );
    }
}
