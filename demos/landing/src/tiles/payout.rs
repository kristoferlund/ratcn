//! Payout threshold settings in a compact tile.

use ratatui::{
    layout::{Alignment, Constraint, Flex, Layout},
    style::{Modifier, Style},
    widgets::{Padding, Paragraph, Wrap},
};
use ratcn::{
    Button, ButtonSize::Large, ButtonVariant, ListItem, ProgressWidget, Select, Toast,
    runtime::DeclareCtx,
};

use crate::{AppMsg, AppState};

use super::shared::declare_tile_panel_with_padding;

pub const ID: &str = "payout";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Currency {
    Usd,
    Eur,
    Gbp,
}

impl Currency {
    const ALL: [Self; 3] = [Self::Usd, Self::Eur, Self::Gbp];

    const fn label(self) -> &'static str {
        match self {
            Self::Usd => "USD - United States dollar",
            Self::Eur => "EUR - Euro",
            Self::Gbp => "GBP - British pound",
        }
    }
}

pub struct State {
    currency: Currency,
    cursor: Option<Currency>,
    open: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            currency: Currency::Usd,
            cursor: Some(Currency::Usd),
            open: false,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Msg {
    OpenChanged(bool),
    Focused(Currency),
    Selected(Currency),
}

impl State {
    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::OpenChanged(open) => self.open = open,
            Msg::Focused(currency) => self.cursor = Some(currency),
            Msg::Selected(currency) => {
                self.currency = currency;
                self.cursor = Some(currency);
                self.open = false;
            }
        }
    }
}

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let controls_disabled = ctx.state().controls_disabled;
    let currency =
        Select::new(Currency::ALL.map(|currency| ListItem::new(currency, currency.label())))
            .open(
                |state: &AppState| state.payout_state.open,
                |open| AppMsg::Payout(Msg::OpenChanged(open)),
            )
            .item_focus(
                |state: &AppState| state.payout_state.cursor,
                |currency| AppMsg::Payout(Msg::Focused(currency)),
            )
            .selection(
                |state: &AppState| Some(state.payout_state.currency),
                |currency| AppMsg::Payout(Msg::Selected(currency)),
            )
            .disabled(controls_disabled);
    let cancel = Button::new("Cancel")
        .variant(ButtonVariant::Outline)
        .size(Large)
        .on_press(|| AppMsg::Toast(Toast::info("Payout changes cancelled")))
        .disabled(controls_disabled);
    let save = Button::new("Save")
        .secondary()
        .size(Large)
        .on_press(|| AppMsg::Toast(Toast::success("Payout threshold saved")))
        .disabled(controls_disabled);
    let actions_width = cancel.width() + 1 + save.width();

    let inner = declare_tile_panel_with_padding(ctx, area, " alt+7 ", Padding::new(2, 2, 1, 1));
    let [
        title_area,
        _gap_one,
        intro_area,
        _gap_two,
        currency_label_area,
        currency_area,
        _gap_three,
        amount_area,
        bar_area,
        range_area,
        _gap_four,
        button_row,
    ] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(Large.height()),
    ])
    .areas(inner);
    let [amount_label_area, amount_value_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(6)]).areas(amount_area);
    let [min_area, max_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(range_area);
    let [actions_area] = Layout::horizontal([Constraint::Length(actions_width)])
        .flex(Flex::End)
        .areas(button_row);
    let [cancel_area, _gap, save_area] = Layout::horizontal([
        Constraint::Length(cancel.width()),
        Constraint::Length(1),
        Constraint::Length(save.width()),
    ])
    .areas(actions_area);
    let theme = ctx.theme;

    ctx.paint_widget(
        Paragraph::new("Payout threshold").style(
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        title_area,
    );
    ctx.paint_widget(
        Paragraph::new("Set the minimum balance required before a payout is triggered.")
            .style(Style::default().fg(theme.muted_foreground))
            .wrap(Wrap { trim: true }),
        intro_area,
    );
    ctx.paint_widget(
        Paragraph::new("Preferred currency").style(Style::default().fg(theme.foreground)),
        currency_label_area,
    );
    ctx.component("currency", currency, currency_area);
    ctx.paint_widget(
        Paragraph::new("Minimum payout amount").style(Style::default().fg(theme.foreground)),
        amount_label_area,
    );
    ctx.paint_widget(
        Paragraph::new("$2,500")
            .style(
                Style::default()
                    .fg(theme.foreground)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Right),
        amount_value_area,
    );
    ctx.paint_widget(ProgressWidget::new(0.25).themed(theme), bar_area);
    ctx.paint_widget(
        Paragraph::new("$50 min").style(Style::default().fg(theme.muted_foreground)),
        min_area,
    );
    ctx.paint_widget(
        Paragraph::new("$10k max")
            .style(Style::default().fg(theme.muted_foreground))
            .alignment(Alignment::Right),
        max_area,
    );
    ctx.component("cancel", cancel, cancel_area);
    ctx.component("save", save, save_area);
}
