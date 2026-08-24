//! One progress bar, three compositions.
//!
//! A bare bar; the shadcn-style pairing of a label and a live percentage over
//! a bar still on its way; and a finished one. The middle row runs off the
//! clock, which is also why this demo keeps waking up to redraw.

use std::{io, time::Duration};

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin},
    style::Style,
};
use ratcn::{ProgressWidget, Theme};

const DEMO_WIDTH: u16 = 34;
const DEMO_HEIGHT: u16 = 9;
const CONTENT_PADDING: Margin = Margin::new(2, 1);
/// Seconds for the downloading row to travel empty-to-full and start over.
const LOOP_SECS: f64 = 8.0;

struct Progress;

impl demo_shared::Demo for Progress {
    const INPUT: bool = false;

    fn wake(&self) -> Option<Duration> {
        Some(demo_shared::ANIMATION_FRAME)
    }

    fn draw(&mut self, frame: &mut Frame, theme: &Theme) {
        let area = frame.area();
        frame
            .buffer_mut()
            .set_style(area, Style::default().bg(theme.background));

        let demo = area.centered(
            Constraint::Length(DEMO_WIDTH),
            Constraint::Length(DEMO_HEIGHT),
        );
        frame
            .buffer_mut()
            .set_style(demo, Style::default().bg(theme.surface));

        // The clock decides how far along the download is, so any frame is a
        // truthful one however late it arrives.
        let elapsed = demo_shared::monotonic_time().as_secs_f64();
        let downloading = (elapsed % LOOP_SECS) / LOOP_SECS;

        let inner = demo.inner(CONTENT_PADDING);
        let [bare, active, finished] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .spacing(1)
        .areas(inner);

        frame.render_widget(ProgressWidget::new(0.33).themed(theme), bare);
        frame.render_widget(
            ProgressWidget::new(downloading)
                .label("Downloading assets.tar.gz")
                .show_value(true)
                .themed(theme),
            active,
        );
        frame.render_widget(
            ProgressWidget::new(1.0)
                .label("Extracted")
                .show_value(true)
                .themed(theme),
            finished,
        );
    }
}

fn main() -> io::Result<()> {
    demo_shared::run(Progress)
}
