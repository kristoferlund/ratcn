//! Telling the terminal what the app is doing.
//!
//! One button starts some made-up work. Halfway it stops to ask whether to go
//! on, then finishes. Each step is reported to the terminal: its own progress
//! indicator (OSC 9;4) and its program status (OSC 7501), which a terminal
//! such as Rex or kitty shows as idle, working, blocked on the user, and done.
//! In a browser the job runs the same, with nothing to report it to.

use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::Style,
    widgets::Paragraph,
};
#[cfg(not(target_arch = "wasm32"))]
use ratcn::terminal::{BlockedKind, ProgramState, ProgramStatus, TerminalProgress};
use ratcn::{
    Button, ButtonSize, ProgressWidget, Theme,
    runtime::{Event, EventResult, FocusState, PointerShape, Ratcn},
};

const DEMO_WIDTH: u16 = 40;
/// Seconds the whole job takes, not counting the wait for the user.
const WORK_SECS: f64 = 8.0;
/// Where the job stops to ask the user, from 0 to 1.
const QUESTION_AT: f64 = 0.5;
/// How the app names itself to the terminal.
#[cfg(not(target_arch = "wasm32"))]
const APP: &str = "ratcn-demo";

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// Nothing started yet.
    Ready,
    /// Working from `from`, since `since` on the demo clock.
    Working {
        from: f64,
        since: Duration,
    },
    /// Stopped halfway, waiting for the user to go on.
    Asking,
    Done,
}

impl Phase {
    /// How far along the job is, from 0 to 1, at `now`.
    fn done_by(self, now: Duration) -> f64 {
        match self {
            Self::Ready => 0.0,
            Self::Working { from, since } => {
                (from + now.saturating_sub(since).as_secs_f64() / WORK_SECS).min(1.0)
            }
            Self::Asking => QUESTION_AT,
            Self::Done => 1.0,
        }
    }

    /// The phase the clock has moved this one on to: work that reached the
    /// question asks it, and work that reached the end is done.
    fn at(self, now: Duration) -> Self {
        let Self::Working { from, .. } = self else {
            return self;
        };
        let done = self.done_by(now);
        if from < QUESTION_AT && done >= QUESTION_AT {
            Self::Asking
        } else if done >= 1.0 {
            Self::Done
        } else {
            self
        }
    }

    const fn caption(self) -> &'static str {
        match self {
            Self::Ready => "Ready when you are.",
            Self::Working { .. } => "Crunching numbers…",
            Self::Asking => "Halfway there. Go on?",
            Self::Done => "All done.",
        }
    }
}

struct AppState {
    focus: FocusState,
    phase: Phase,
    /// How far along the job was at the last frame, from 0 to 1.
    done: f64,
}

#[derive(Clone)]
enum Msg {
    Focus(FocusState),
    Pressed,
}

pub struct App {
    state: AppState,
    ratcn: Ratcn<AppState, Msg>,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: AppState {
                // Start focused, so Enter starts the job.
                focus: FocusState::intent(["job"]),
                phase: Phase::Ready,
                done: 0.0,
            },
            ratcn: Ratcn::new().focus(|s: &AppState| &s.focus, Msg::Focus),
        }
    }

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(focus) => self.state.focus = focus,
            Msg::Pressed => {
                let since = demo_shared::monotonic_time();
                self.state.phase = match self.state.phase {
                    Phase::Ready | Phase::Done => Phase::Working { from: 0.0, since },
                    Phase::Asking => Phase::Working {
                        from: QUESTION_AT,
                        since,
                    },
                    working @ Phase::Working { .. } => working,
                };
            }
        }
    }

    /// The job's progress as a whole percentage.
    #[cfg(not(target_arch = "wasm32"))]
    fn percent(&self) -> u8 {
        (self.state.done * 100.0).round() as u8
    }
}

impl demo_shared::Demo for App {
    fn wake(&self) -> Option<Duration> {
        matches!(self.state.phase, Phase::Working { .. }).then_some(demo_shared::ANIMATION_FRAME)
    }

    fn handle_event(&mut self, event: Event) -> bool {
        match self.ratcn.handle_event(event, &self.state) {
            EventResult::Emit(msg) => {
                self.update(msg);
                true
            }
            EventResult::Consumed => true,
            EventResult::Ignored => false,
        }
    }

    fn pointer_shape(&self) -> PointerShape {
        self.ratcn.pointer_shape()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn terminal_progress(&self) -> TerminalProgress {
        match self.state.phase {
            Phase::Ready | Phase::Done => TerminalProgress::Hidden,
            Phase::Working { .. } => TerminalProgress::Normal(self.percent()),
            Phase::Asking => TerminalProgress::Paused(self.percent()),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn program_status(&self) -> Option<ProgramStatus> {
        let state = match self.state.phase {
            Phase::Ready => ProgramState::Idle,
            Phase::Working { .. } => ProgramState::Working {
                progress: Some(self.percent()),
            },
            Phase::Asking => ProgramState::Blocked {
                kind: Some(BlockedKind::Permission),
                progress: Some(self.percent()),
            },
            Phase::Done => ProgramState::Done,
        };
        Some(
            ProgramStatus::new(state)
                .app(APP)
                .title("Program status demo")
                .message(self.state.phase.caption()),
        )
    }

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        buffer.set_style(area, Style::default().bg(theme.background));

        // The clock moves the job on, so any frame is a truthful one however
        // late it arrives.
        let now = demo_shared::monotonic_time();
        self.state.phase = self.state.phase.at(now);
        self.state.done = self.state.phase.done_by(now);

        let state = &self.state;
        self.ratcn.render_into(buffer, area, state, theme, |ctx| {
            let button = match state.phase {
                Phase::Ready => Button::new("Start"),
                Phase::Working { .. } => Button::new("Working").disabled(true),
                Phase::Asking => Button::new("Continue"),
                Phase::Done => Button::new("Start again").secondary(),
            }
            .size(ButtonSize::Large)
            .on_press(|| Msg::Pressed);

            let column = area.centered_horizontally(Constraint::Length(DEMO_WIDTH));
            let [caption, bar, button_row] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(ButtonSize::Large.height()),
            ])
            .flex(Flex::Center)
            .spacing(1)
            .areas(column);
            let [button_area] = Layout::horizontal([Constraint::Length(button.width())])
                .flex(Flex::Center)
                .areas(button_row);

            ctx.paint_widget(
                Paragraph::new(state.phase.caption())
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(theme.muted_foreground)),
                caption,
            );
            ctx.paint_widget(ProgressWidget::new(state.done).themed(theme), bar);
            ctx.component("job", button, button_area);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{Duration, Phase, QUESTION_AT};

    const fn secs(secs: u64) -> Duration {
        Duration::from_secs(secs)
    }

    /// The job stops at the question however late the frame that notices
    /// arrives, and goes on from there, not from where the clock would be.
    #[test]
    fn the_job_stops_to_ask_halfway_and_then_finishes() {
        let started = Phase::Working {
            from: 0.0,
            since: secs(10),
        };
        assert_eq!(started.at(secs(11)), started);
        assert_eq!(started.at(secs(30)), Phase::Asking);

        let resumed = Phase::Working {
            from: QUESTION_AT,
            since: secs(40),
        };
        assert_eq!(resumed.at(secs(41)), resumed);
        assert_eq!(resumed.at(secs(60)), Phase::Done);
        assert!((Phase::Asking.done_by(secs(99)) - QUESTION_AT).abs() < f64::EPSILON);
    }
}
