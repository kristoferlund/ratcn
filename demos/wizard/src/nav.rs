//! Which step is showing, and the Back/Next row that moves between them.
//!
//! This is orchestration state — it belongs to the app shell, not to any step.

use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::Style,
    text::{Line, Span},
};
use ratcn::{Button, ButtonSize, ButtonWidget, Theme, runtime::RenderCtx};

use crate::app::{AppState, Msg};

pub const BACK_ID: &str = "nav_back";
pub const NEXT_ID: &str = "nav_next";

/// The widest label the Next button ever takes. Its slot is sized for this one
/// so the row does not shift when the label changes on the last input step.
const FINISH_LABEL: &str = "Finish";
const BUTTON_GAP: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Step {
    #[default]
    Project,
    Backend,
    Theme,
    Done,
}

impl Step {
    pub const ALL: [Step; 4] = [Step::Project, Step::Backend, Step::Theme, Step::Done];

    pub const fn index(self) -> usize {
        match self {
            Step::Project => 0,
            Step::Backend => 1,
            Step::Theme => 2,
            Step::Done => 3,
        }
    }

    pub const fn is_first(self) -> bool {
        matches!(self, Step::Project)
    }

    pub const fn is_last(self) -> bool {
        matches!(self, Step::Done)
    }

    /// The ends clamp: the buttons that would leave the wizard are disabled or
    /// absent, so a message past either end can only arrive by mistake.
    const fn next(self) -> Self {
        match self {
            Step::Project => Step::Backend,
            Step::Backend => Step::Theme,
            Step::Theme | Step::Done => Step::Done,
        }
    }

    const fn previous(self) -> Self {
        match self {
            Step::Project | Step::Backend => Step::Project,
            Step::Theme => Step::Backend,
            Step::Done => Step::Theme,
        }
    }
}

#[derive(Debug, Default)]
pub struct Nav {
    pub step: Step,
}

#[derive(Debug, Clone, Copy)]
pub enum NavMsg {
    Next,
    Back,
}

impl Nav {
    pub fn update(&mut self, msg: NavMsg) {
        self.step = match msg {
            NavMsg::Next => self.step.next(),
            NavMsg::Back => self.step.previous(),
        };
    }
}

/// How far along the wizard is: one dot per step, filled up to the current one.
pub fn stepper(step: Step, theme: &Theme) -> Line<'static> {
    let mut spans = Vec::with_capacity(Step::ALL.len() * 2 - 1);
    for index in 0..Step::ALL.len() {
        if index > 0 {
            spans.push(Span::styled("───", Style::default().fg(theme.border)));
        }
        let reached = index <= step.index();
        spans.push(Span::styled(
            if reached { "●" } else { "○" },
            Style::default().fg(if reached { theme.primary } else { theme.border }),
        ));
    }
    Line::from(spans).centered()
}

/// The Back/Next row. Both buttons keep their ids across steps, so focus stays
/// on Next and Enter walks the whole wizard.
pub fn render(ctx: &mut RenderCtx<'_, AppState, Msg>, area: Rect, step: Step) {
    let back = Button::new("Back")
        .outline()
        .size(ButtonSize::Large)
        .disabled(step.is_first())
        .on_press(|| Msg::Nav(NavMsg::Back));
    let back_width = back.width();

    if step.is_last() {
        let [back_area] = Layout::horizontal([Constraint::Length(back_width)])
            .flex(Flex::Center)
            .areas(area);
        ctx.render_component(BACK_ID, back, back_area);
        return;
    }

    let next = Button::new(next_label(step))
        .size(ButtonSize::Large)
        .on_press(|| Msg::Nav(NavMsg::Next));
    let [back_area, _gap, next_area] = Layout::horizontal([
        Constraint::Length(back_width),
        Constraint::Length(BUTTON_GAP),
        Constraint::Length(ButtonWidget::new(FINISH_LABEL).width()),
    ])
    .flex(Flex::Center)
    .areas(area);

    ctx.render_component(BACK_ID, back, back_area);
    ctx.render_component(NEXT_ID, next, next_area);
}

const fn next_label(step: Step) -> &'static str {
    if step.next().is_last() {
        FINISH_LABEL
    } else {
        "Next"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Back is disabled on the first step and Next is absent on the last, so
    /// neither end can be walked past — but a stray message must not either.
    #[test]
    fn the_ends_clamp() {
        let mut nav = Nav::default();

        nav.update(NavMsg::Back);
        assert_eq!(nav.step, Step::Project);

        for _ in 0..Step::ALL.len() + 2 {
            nav.update(NavMsg::Next);
        }
        assert_eq!(nav.step, Step::Done);
    }

    /// The label announces that the next press ends the wizard rather than
    /// advancing it.
    #[test]
    fn the_last_input_step_offers_finish_rather_than_next() {
        assert_eq!(next_label(Step::Project), "Next");
        assert_eq!(next_label(Step::Backend), "Next");
        assert_eq!(next_label(Step::Theme), FINISH_LABEL);
    }
}
