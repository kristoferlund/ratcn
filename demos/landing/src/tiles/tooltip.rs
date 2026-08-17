//! One button that explains itself on hover.
//!
//! The tile stores nothing. The runtime owns hover, so the bubble follows the
//! pointer on its own and no message is routed for the tooltip at all.

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratcn::{
    Button, ButtonSize::Large, ButtonVariant, Toast, ToastKind, Tooltip, TooltipSide,
    runtime::DeclareCtx,
};

use crate::{AppMsg, AppState};

use super::shared::declare_tile_panel;

pub const ID: &str = "tooltip";

/// The Tooltip's own id inside the tile, and the button's id inside that.
const TIP: &str = "tip";
const TRIGGER: &str = "button";

const LABEL: &str = "What's this?";
const TOOLTIP_TEXT: &str = "Just a tooltip.";
const TOAST_TEXT: &str = "Just a toast.";

pub fn declare(ctx: &mut DeclareCtx<'_, AppState, AppMsg>) {
    let area = ctx.area();
    let inner = declare_tile_panel(ctx, area, " alt+6 ");
    let button_area = centered(inner, button(false).width());

    let disabled = ctx.state().controls_disabled;
    let tooltip = Tooltip::new(TOOLTIP_TEXT)
        .side(TooltipSide::Top)
        // Hover is the runtime's, and the tooltip shows on it by default; all
        // this adds is that a disabled button explains nothing.
        .open_when(move |state: &AppState, hovered| hovered && !state.controls_disabled)
        .trigger(move |ctx| {
            let area = ctx.area();
            ctx.component(TRIGGER, button(disabled), area);
        });

    ctx.component(TIP, tooltip, button_area);
}

/// Built twice per frame — once to measure, once to declare — so the width the
/// layout reserves and the width that paints cannot disagree.
///
/// The press handler is what makes the button focusable: `Button::is_focusable`
/// is `!disabled && on_press.is_some()`, so without one the tile has no focus
/// target and `alt+6` has nowhere to land.
fn button(disabled: bool) -> Button<AppMsg> {
    Button::new(LABEL)
        .variant(ButtonVariant::Outline)
        .size(Large)
        .on_press(|| AppMsg::Toast(Toast::new(TOAST_TEXT).kind(ToastKind::Info)))
        .disabled(disabled)
}

/// The button centered in both axes, or nothing when the tile is too short to
/// hold it. A zero area keeps the identity and drops out of traversal.
fn centered(bounds: Rect, width: u16) -> Rect {
    let height = Large.height();
    if bounds.height < height || bounds.width < width {
        return Rect::ZERO;
    }
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(bounds);
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    area
}
