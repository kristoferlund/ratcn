//! One progress bar, three compositions.
//!
//! A bare bar; the shadcn-style pairing of a label and a live percentage over
//! a bar still on its way; and a finished one. The middle row runs off the
//! clock, which is also why this demo keeps waking up to redraw. In a terminal
//! that draws progress in its own chrome, the middle row is shown there too.

use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::Style,
    widgets::Widget,
};
#[cfg(not(target_arch = "wasm32"))]
use ratcn::terminal::TerminalProgress;
use ratcn::{ProgressWidget, Theme};

const DEMO_WIDTH: u16 = 34;
const DEMO_HEIGHT: u16 = 9;
const CONTENT_PADDING: Margin = Margin::new(2, 1);
/// Seconds for the downloading row to travel empty-to-full and start over.
const LOOP_SECS: f64 = 8.0;

pub struct Progress;

impl demo_shared::Demo for Progress {
    const INPUT: bool = false;

    fn wake(&self) -> Option<Duration> {
        Some(demo_shared::ANIMATION_FRAME)
    }

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        buffer.set_style(area, Style::default().bg(theme.background));

        let demo = area.centered(
            Constraint::Length(DEMO_WIDTH),
            Constraint::Length(DEMO_HEIGHT),
        );
        buffer.set_style(demo, Style::default().bg(theme.surface));

        let downloading = downloading();

        let inner = demo.inner(CONTENT_PADDING);
        let [bare, active, finished] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .spacing(1)
        .areas(inner);

        ProgressWidget::new(0.33).themed(theme).render(bare, buffer);
        ProgressWidget::new(downloading)
            .label("Downloading assets.tar.gz")
            .show_value(true)
            .themed(theme)
            .render(active, buffer);
        ProgressWidget::new(1.0)
            .label("Extracted")
            .show_value(true)
            .themed(theme)
            .render(finished, buffer);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn terminal_progress(&self) -> TerminalProgress {
        // Rounded the way the row's own percentage is.
        TerminalProgress::Normal((downloading() * 100.0).round() as u8)
    }
}

/// How far along the download is, from 0 to 1. The clock decides, so any
/// frame is a truthful one however late it arrives.
fn downloading() -> f64 {
    let elapsed = demo_shared::monotonic_time().as_secs_f64();
    (elapsed % LOOP_SECS) / LOOP_SECS
}
