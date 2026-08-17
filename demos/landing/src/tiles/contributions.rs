use ratatui::{
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    widgets::{Bar, Paragraph, Wrap},
};
use ratcn::{BarChartWidget, runtime::RenderCtx};

use crate::{AppMsg, AppState};

use super::shared::render_tile_panel;

const MONTHLY_CONTRIBUTIONS: [(&str, u64); 6] = [
    ("Dec", 8),
    ("Jan", 14),
    ("Feb", 10),
    ("Mar", 18),
    ("Apr", 13),
    ("May", 22),
];

pub const ID: &str = "contributions";

pub fn render(ctx: &mut RenderCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let inner = render_tile_panel(ctx, area, " alt+5 ");
    let [header_area, intro_area, chart_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Fill(1),
    ])
    .spacing(1)
    .areas(inner);

    let left_padding = ratatui::layout::Rect {
        width: 1.min(chart_area.width),
        ..chart_area
    };
    let chart_area = ratatui::layout::Rect {
        x: chart_area.x.saturating_add(1),
        width: chart_area.width.saturating_sub(1),
        ..chart_area
    };
    ctx.paint(move |ctx| {
        let theme = ctx.theme;
        ctx.render_widget(
            Paragraph::new("Contribution history").style(
                Style::default()
                    .fg(theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            header_area,
        );
        ctx.render_widget(
            Paragraph::new("Last six months of activity")
                .style(Style::default().fg(theme.muted_foreground))
                .wrap(Wrap { trim: true }),
            intro_area,
        );
        ctx.with_buffer(|buf| buf.set_style(left_padding, Style::default().bg(theme.field)));
        let bars = MONTHLY_CONTRIBUTIONS.map(|(label, value)| Bar::with_label(label, value));
        ctx.render_widget(
            BarChartWidget::vertical(bars)
                .themed(theme)
                .max_value(24)
                .show_values(false)
                .bar_width(4)
                .bar_gap(2),
            chart_area,
        );
    });
}
