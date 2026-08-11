//! Ledger tab: the book of transactions.
//!
//! The row cursor and scroll offset are local. The entries and currency preference
//! are shared, so this view updates when Settings changes the currency.

use ratatui::{
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
};
use ratcn::{List, ListItem, runtime::RenderCtx};

use crate::app::{AppState, Msg as AppMsg};
use crate::shared;

pub const SCREEN_ID: &str = "ledger_screen";
pub const LIST_ID: &str = "ledger_list";

/// The width the label + dot-leaders + amount are padded to, so the amounts form
/// a clean right-hand column like a real ledger.
const ROW_WIDTH: usize = 44;

#[derive(Debug, Default)]
pub struct State {
    pub row: Option<&'static str>,
    pub list_scroll: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum Msg {
    RowFocused(&'static str, usize),
    ListScrolled(usize),
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::RowFocused(row, offset) => {
                self.row = Some(row);
                self.list_scroll = offset;
            }
            Msg::ListScrolled(offset) => self.list_scroll = offset,
        }
    }
}

pub fn render(ctx: &mut RenderCtx<'_, '_, AppState, AppMsg>) {
    let area = ctx.area();
    let state = ctx.state();
    let list = List::new(shared::SEED.map(|entry| ListItem::new(entry.label, entry.label)))
        .item_focus(
            |s: &AppState| s.ledger.row,
            |row, offset| AppMsg::Ledger(Msg::RowFocused(row, offset)),
        )
        .scroll(
            |s: &AppState| s.ledger.list_scroll,
            |offset| AppMsg::Ledger(Msg::ListScrolled(offset)),
        )
        .render_item(|s: &AppState, row| render_row(row.index, &s.shared.prefs));

    let inner = crate::screens::render_panel(ctx, area, ctx.theme, None);

    let [list_area, _gap, balance_area] = inner.layout(&Layout::vertical([
        Constraint::Length(shared::SEED.len() as u16),
        Constraint::Min(0),
        Constraint::Length(1),
    ]));
    ctx.render_component(LIST_ID, list, list_area);

    let balance = shared::balance();
    let theme = ctx.theme;
    let amount = shared::format_money(balance, &state.shared.prefs);
    let amount_color = if balance < 0 {
        theme.destructive
    } else {
        theme.primary
    };
    ctx.render_widget(
        Line::from(vec![
            Span::styled(
                "Balance ",
                Style::default()
                    .fg(theme.muted_foreground)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                amount,
                Style::default()
                    .fg(amount_color)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        balance_area,
    );
}

/// One ledger row: `Floppy disks (bulk box) ......... ($42.00)`, with dot
/// leaders and the amount colored by sign.
fn render_row(index: usize, prefs: &shared::Prefs) -> Line<'static> {
    let entry = shared::SEED[index];
    let amount = shared::format_money(entry.cents, prefs);

    let used = entry.label.chars().count() + amount.chars().count();
    let leaders = ".".repeat(ROW_WIDTH.saturating_sub(used).max(1));

    let amount_color = if entry.cents < 0 {
        crate::app::THEME.destructive
    } else {
        crate::app::THEME.primary
    };

    Line::from(vec![
        Span::raw(entry.label),
        Span::styled(
            format!(" {leaders} "),
            Style::default().fg(crate::app::THEME.border),
        ),
        Span::styled(amount, Style::default().fg(amount_color)),
    ])
}
