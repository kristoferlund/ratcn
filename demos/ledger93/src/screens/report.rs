//! Report tab: expenses by category, as a bar chart.
//!
//! Sort order is local to this tab. The totals and currency preference come from
//! shared state, so Settings can relabel the chart without owning report state.

use ratatui::{
    layout::{Constraint, Flex, Layout},
    text::Line,
    widgets::Bar,
};
use ratcn::{BarChartWidget, Button, ButtonSize, ButtonWidget, runtime::RenderCtx};

use crate::app::{AppState, Msg as AppMsg};
use crate::shared::{self, Category};

pub const SCREEN_ID: &str = "report_screen";
pub const SORT_ID: &str = "report_sort";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Amount,
    Name,
}

#[derive(Debug, Default)]
pub struct State {
    pub sort: Sort,
}

#[derive(Debug, Clone, Copy)]
pub enum Msg {
    ToggleSort,
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::ToggleSort => {
                self.sort = match self.sort {
                    Sort::Amount => Sort::Name,
                    Sort::Name => Sort::Amount,
                };
            }
        }
    }
}

pub fn render(ctx: &mut RenderCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let state = ctx.state();
    // Constrain layout to the widest label so the button keeps its width as the
    // sort toggles; the paint widget measures without declaring anything.
    let button_width = ButtonWidget::new("Sort by: amount").width();
    let label = match state.report.sort {
        Sort::Amount => "Sort by: amount",
        Sort::Name => "Sort by: name",
    };
    let sort = Button::new(label)
        .outline()
        .size(ButtonSize::Large)
        .on_press(|| AppMsg::Report(Msg::ToggleSort));
    let button_height = sort.height();

    let inner = crate::screens::render_panel(ctx, area, None);

    let [chart_area, _gap, footer_area] = inner.layout(&Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(button_height),
    ]));

    let bars = bars(state);
    let max = bars
        .iter()
        .map(|(_, dollars, _)| *dollars)
        .max()
        .unwrap_or(1)
        .max(1);
    let chart_bars: Vec<Bar> = bars
        .iter()
        .map(|(category, dollars, label)| {
            Bar::default()
                .value(*dollars)
                .label(Line::from(category.name()))
                .text_value(label.clone())
        })
        .collect();
    ctx.paint(move |ctx| {
        ctx.render_widget(
            BarChartWidget::new(chart_bars)
                .themed(ctx.theme)
                .max_value(max)
                .bar_width(9)
                .bar_gap(1),
            chart_area,
        );
    });

    let [button_area] = Layout::horizontal([Constraint::Length(button_width)])
        .flex(Flex::Center)
        .areas(footer_area);
    ctx.render_component(SORT_ID, sort, button_area);
}

fn bars(state: &AppState) -> Vec<(Category, u64, String)> {
    let mut rows: Vec<(Category, i64)> = Category::EXPENSES
        .iter()
        .map(|&category| (category, shared::category_total(category)))
        .collect();

    match state.report.sort {
        Sort::Amount => rows.sort_by_key(|row| std::cmp::Reverse(row.1)),
        Sort::Name => rows.sort_by(|a, b| a.0.name().cmp(b.0.name())),
    }

    rows.into_iter()
        .map(|(category, cents)| {
            let dollars = u64::try_from(cents / 100).unwrap_or(0);
            (
                category,
                dollars,
                shared::format_money(cents, &state.shared.prefs),
            )
        })
        .collect()
}
