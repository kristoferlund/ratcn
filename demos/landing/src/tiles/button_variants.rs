use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    widgets::Padding,
};
use ratcn::{Button, ButtonSize::Large, ButtonVariant, Toast, ToastKind, runtime::DeclareCtx};

use crate::{AppMsg, AppState};

use super::shared::declare_tile_panel_with_padding;

const BUTTONS: [(&str, &str, ButtonVariant, ToastKind); 5] = [
    (
        "default",
        "Default",
        ButtonVariant::Default,
        ToastKind::Default,
    ),
    (
        "secondary",
        "Secondary",
        ButtonVariant::Secondary,
        ToastKind::Info,
    ),
    (
        "outline",
        "Outline",
        ButtonVariant::Outline,
        ToastKind::Success,
    ),
    ("ghost", "Ghost", ButtonVariant::Ghost, ToastKind::Default),
    (
        "destructive",
        "Destructive",
        ButtonVariant::Destructive,
        ToastKind::Error,
    ),
];

pub const ID: &str = "button_variants";

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let controls_disabled = ctx.state().controls_disabled;
    let buttons = BUTTONS
        .map(|(id, label, variant, kind)| (id, button(label, variant, kind, controls_disabled)));
    let button_width = buttons
        .iter()
        .map(|(_, button)| button.width())
        .max()
        .unwrap_or(0);
    let button_layout = Layout::horizontal([Constraint::Length(button_width)]).flex(Flex::Center);
    let inner_area =
        declare_tile_panel_with_padding(ctx, area, " alt+3 ", Padding::new(2, 2, 0, 0));
    let rows = button_rows(inner_area);

    for ((id, button), row) in buttons.into_iter().zip(rows) {
        declare_button(ctx, id, button, row, &button_layout, inner_area);
    }
}

fn button(
    label: &'static str,
    variant: ButtonVariant,
    kind: ToastKind,
    disabled: bool,
) -> Button<AppMsg> {
    Button::new(label)
        .variant(variant)
        .size(Large)
        .on_press(move || AppMsg::Toast(Toast::new(format!("{label} pressed")).kind(kind)))
        .disabled(disabled)
}

fn button_rows(area: Rect) -> [Rect; BUTTONS.len()] {
    let button_height = Large.height();
    let content_height = BUTTONS.len() as u16 * button_height;
    let start_y = area.y + area.height.saturating_sub(content_height) / 2;

    std::array::from_fn(|index| Rect {
        x: area.x,
        y: start_y + index as u16 * button_height,
        width: area.width,
        height: button_height,
    })
}

fn declare_button(
    ctx: &mut DeclareCtx<'_, AppState, AppMsg>,
    id: &'static str,
    button: Button<AppMsg>,
    row: Rect,
    button_layout: &Layout,
    bounds: Rect,
) {
    let button_area = if row.y >= bounds.y
        && row.y.saturating_add(row.height) <= bounds.y.saturating_add(bounds.height)
    {
        let [area] = row.layout(button_layout);
        area
    } else {
        Rect::ZERO
    };
    ctx.component(id, button, button_area);
}
