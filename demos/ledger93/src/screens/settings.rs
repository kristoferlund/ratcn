//! Settings tab: edits the shared display preferences.
//!
//! The list cursor and scroll offset are local to Settings. The selected currency
//! is shared state, applied by the app shell and read by Ledger and Report.

use ratatui::{
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
};
use ratcn::{List, ListItem, runtime::RenderCtx};

use crate::app::{AppState, Msg as AppMsg};
use crate::shared::{self, Currency, PrefsMsg};

pub const SCREEN_ID: &str = "settings_screen";
pub const CURRENCY_ID: &str = "settings_currency";

#[derive(Debug, Default)]
pub struct State {
    pub currency_cursor: Option<Currency>,
    pub list_scroll: usize,
}

#[derive(Debug, Clone, Copy)]
pub enum Msg {
    CurrencyFocused(Currency, usize),
    ListScrolled(usize),
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::CurrencyFocused(currency, offset) => {
                self.currency_cursor = Some(currency);
                self.list_scroll = offset;
            }
            Msg::ListScrolled(offset) => self.list_scroll = offset,
        }
    }
}

pub fn render(ctx: &mut RenderCtx<'_, '_, AppState, AppMsg>) {
    let area = ctx.area();
    let state = ctx.state();
    let currency =
        List::new(Currency::ALL.map(|currency| ListItem::new(currency, currency.label())))
            .item_focus(
                |s: &AppState| s.settings.currency_cursor,
                |currency, offset| AppMsg::Settings(Msg::CurrencyFocused(currency, offset)),
            )
            .selection(
                |s: &AppState| Some(s.shared.prefs.currency),
                |currency| AppMsg::Prefs(PrefsMsg::SetCurrency(currency)),
            )
            .scroll(
                |s: &AppState| s.settings.list_scroll,
                |offset| AppMsg::Settings(Msg::ListScrolled(offset)),
            );

    let theme = ctx.theme;
    let inner = crate::screens::render_panel(ctx, area, theme, None);

    let [header, list_area, _gap, preview_area] = inner.layout(&Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(Currency::ALL.len() as u16),
        Constraint::Min(0),
        Constraint::Length(1),
    ]));

    ctx.render_widget(
        Line::from(Span::styled(
            "Currency",
            Style::default()
                .fg(theme.muted_foreground)
                .add_modifier(Modifier::BOLD),
        )),
        header,
    );
    ctx.render_component(CURRENCY_ID, currency, list_area);

    let prefs = &state.shared.prefs;
    ctx.render_widget(
        Line::from(vec![
            Span::styled("Preview: ", Style::default().fg(theme.muted_foreground)),
            Span::styled(
                shared::format_money(-123_456, prefs),
                Style::default().fg(theme.destructive),
            ),
            Span::raw("   "),
            Span::styled(
                shared::format_money(78_900, prefs),
                Style::default().fg(theme.primary),
            ),
        ]),
        preview_area,
    );
}
