//! The frame the site is drawn in: the header bar, the rules, and the geometry
//! every view lays itself out inside.

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect, Size},
    style::{Color, Style},
    symbols::line,
    widgets::Paragraph,
};
use ratcn::{Button, ButtonVariant, Theme, runtime::DeclareCtx};

use crate::{AppState, Msg, View, catalog};

/// Child ids, named once so declarations and retained identity cannot drift.
mod ids {
    pub const HOME: &str = "home";
    pub const DEMOS: &str = "demos";
}

/// A column of breathing room on each side of the longest demo name.
const NAV_PADDING: u16 = 1;

/// The narrowest demo pane worth painting: below this every demo is clipped to
/// a stripe, and the browser is showing nothing anyone can read.
const MIN_PANE_WIDTH: u16 = 20;

/// The rows the chrome owes before a view gets any: the header, the rule under
/// it, one list row, the rule above the hints, and the three hint lines.
const MIN_HEIGHT: u16 = 7;

/// The horizontal bands of the screen.
pub struct Frames {
    /// Row 0: the logo and the nav buttons.
    pub header: Rect,
    /// Row 1: the rule under the header, full width.
    pub rule: Rect,
    /// Everything below, which the current view owns.
    pub body: Rect,
}

/// The vertical bands of the Demos view.
pub struct Columns {
    /// The demo names and the key hints.
    pub nav: Rect,
    /// The one-cell rule between them and the demo.
    pub rule: Rect,
    /// Where the selected demo paints.
    pub pane: Rect,
}

/// The smallest area the chrome can lay itself out in.
pub fn min_size() -> Size {
    Size::new(nav_text_width() + 1 + MIN_PANE_WIDTH, MIN_HEIGHT)
}

/// Split `area` into its bands, or [`None`] when it is too small to hold them.
///
/// Refusing is not politeness: the rules below are painted straight into the
/// buffer, where a row outside it is a panic rather than a clip.
pub fn layout(area: Rect) -> Option<Frames> {
    let min = min_size();
    if area.width < min.width || area.height < min.height {
        return None;
    }
    let [header, rule, body] = area.layout(&Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ]));
    Some(Frames { header, rule, body })
}

/// Split the body of the Demos view into the nav column, its rule, and the pane.
pub fn columns(body: Rect) -> Columns {
    let [nav, rule, pane] = body.layout(&Layout::horizontal([
        Constraint::Length(nav_width(body.width)),
        Constraint::Length(1),
        Constraint::Fill(1),
    ]));
    Columns { nav, rule, pane }
}

/// The longest demo name with a column of padding on each side.
fn nav_text_width() -> u16 {
    catalog::widest_name() + 2 * NAV_PADDING
}

/// What the nav column takes of `total`, capped at half of it so a narrow
/// terminal keeps a demo pane rather than a list of names.
fn nav_width(total: u16) -> u16 {
    nav_text_width().min(total / 2)
}

/// The logo and the site's one nav button.
pub fn declare(ctx: &mut DeclareCtx<'_, AppState, Msg>, state: &AppState, header: Rect) {
    let home = Button::new("ratcn")
        .ghost()
        .on_press(|| Msg::Navigate(View::Landing));
    // The button that is showing wears the heavier variant: the nav reads its
    // own active state out of the theme rather than out of a hand-built style.
    let demos = Button::new("Demos")
        .variant(match state.view {
            View::Demos => ButtonVariant::Secondary,
            View::Landing => ButtonVariant::Ghost,
        })
        .on_press(|| Msg::Navigate(View::Demos));

    let [home_area, demos_area, _rest] = header.layout(&Layout::horizontal([
        Constraint::Length(home.width()),
        Constraint::Length(demos.width()),
        Constraint::Fill(1),
    ]));
    ctx.component(ids::HOME, home, home_area);
    ctx.component(ids::DEMOS, demos, demos_area);
}

/// The header rule, and — in the Demos view — the vertical rule below it,
/// joined by a `┬`.
///
/// `column` is where that rule stands; `live` says the demo beside it has the
/// input, which is what the rule's color tells the user.
pub fn separators(
    buffer: &mut Buffer,
    frames: &Frames,
    column: Option<u16>,
    theme: &Theme,
    live: bool,
) {
    horizontal_rule(buffer, frames.rule, theme.border);
    let Some(x) = column else {
        return;
    };
    let color = rule_color(theme, live);
    buffer[(x, frames.rule.y)]
        .set_symbol(line::HORIZONTAL_DOWN)
        .set_fg(color);
    vertical_rule(
        buffer,
        Rect::new(x, frames.body.y, 1, frames.body.height),
        color,
    );
}

/// What the line around the embedded demo is painted in — the rule down the
/// side of it in the Demos view with every junction that meets it, the preview
/// window's frame on the landing page: the ring while the demo has the input,
/// so the user can see what is live, and the ordinary border otherwise.
pub fn rule_color(theme: &Theme, live: bool) -> Color {
    if live { theme.ring } else { theme.border }
}

/// A one-row rule filling `area`'s width.
pub fn horizontal_rule(buffer: &mut Buffer, area: Rect, color: Color) {
    for x in area.left()..area.right() {
        buffer[(x, area.y)]
            .set_symbol(line::HORIZONTAL)
            .set_fg(color);
    }
}

/// A one-column rule filling `area`'s height.
fn vertical_rule(buffer: &mut Buffer, area: Rect, color: Color) {
    for y in area.top()..area.bottom() {
        buffer[(area.x, y)].set_symbol(line::VERTICAL).set_fg(color);
    }
}

/// What a window too small for the chrome gets instead of a layout.
pub fn too_small(frame: &mut Frame, area: Rect, theme: &Theme) {
    let min = min_size();
    frame.render_widget(
        Paragraph::new(format!(
            "Window too small — {}×{} needed",
            min.width, min.height
        ))
        .centered()
        .style(Style::default().fg(theme.muted_foreground)),
        area.centered_vertically(Constraint::Length(1)),
    );
}
