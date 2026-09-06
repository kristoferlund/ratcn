//! The header bar and the rule under it — the two rows every view sits below —
//! and the rule primitives the views draw their own separators with.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect, Size},
    style::{Color, Style},
    symbols::line,
    widgets::{Paragraph, Widget},
};
use ratcn::{Button, ButtonVariant, Theme, runtime::DeclareCtx};

use crate::{AppState, Msg, View, demos};

/// Child ids, named once so declarations and retained identity cannot drift.
mod ids {
    pub const HOME: &str = "home";
    pub const GETTING_STARTED: &str = "getting-started";
    pub const DEMOS: &str = "demos";
}

/// What the links say, for the same reason [`ids`] exists: the landing page's
/// hero buttons are two of these destinations again, and a destination with two
/// names is two destinations to a reader.
pub mod labels {
    pub const HOME: &str = "ratcn";
    pub const GETTING_STARTED: &str = "Getting started";
    pub const DEMOS: &str = "Demos";
}

/// The header and the rule under it.
const HEADER_ROWS: u16 = 2;

/// The horizontal bands of the screen.
pub struct Bands {
    /// Row 0: the logo and the nav buttons.
    pub header: Rect,
    /// Row 1: the rule under the header, full width.
    pub rule: Rect,
    /// Everything below, which the current view owns.
    pub body: Rect,
}

/// The smallest area the chrome can lay itself out in.
///
/// The Demos view sets both numbers — its nav column and its hints are what
/// the app cannot shrink past — and the landing page scrolls into whatever is
/// left.
pub fn min_size() -> Size {
    Size::new(demos::min_width(), HEADER_ROWS + demos::min_body_height())
}

/// Split `area` into its bands, or [`None`] when it is too small to hold them.
///
/// Refusing is not politeness: the rules are painted straight into the buffer,
/// where a row outside it is a panic rather than a clip.
pub fn layout(area: Rect) -> Option<Bands> {
    let min = min_size();
    if area.width < min.width || area.height < min.height {
        return None;
    }
    let [header, rule, body] = area.layout(&Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ]));
    Some(Bands { header, rule, body })
}

/// The site's three links, in reading order: the logo, then the two pages the
/// hero buttons also lead to.
pub fn declare(ctx: &mut DeclareCtx<'_, AppState, Msg>, state: &AppState, header: Rect) {
    let home = link(labels::HOME, View::Landing, state.view);
    let started = link(labels::GETTING_STARTED, View::GettingStarted, state.view);
    let demos = link(labels::DEMOS, View::Demos, state.view);

    let [home_area, started_area, demos_area, _rest] = header.layout(&Layout::horizontal([
        Constraint::Length(home.width()),
        Constraint::Length(started.width()),
        Constraint::Length(demos.width()),
        Constraint::Fill(1),
    ]));
    ctx.component(ids::HOME, home, home_area);
    ctx.component(ids::GETTING_STARTED, started, started_area);
    ctx.component(ids::DEMOS, demos, demos_area);
}

/// One header link. The button whose view is showing wears the heavier
/// variant: the nav reads its own active state out of the theme rather than
/// out of a hand-built style.
fn link(label: &str, target: View, showing: View) -> Button<Msg> {
    Button::new(label)
        .variant(if target == showing {
            ButtonVariant::Secondary
        } else {
            ButtonVariant::Ghost
        })
        .on_press(move || Msg::Navigate(target))
}

/// The rule under the header, full width. What meets it from below is the
/// view's to draw.
pub fn header_rule(buffer: &mut Buffer, bands: &Bands, theme: &Theme) {
    horizontal_rule(buffer, bands.rule, theme.border);
}

/// What a line around the embedded demo is painted in: the ring while the demo
/// has the input, so the user can see what is live, and the border otherwise.
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
pub fn vertical_rule(buffer: &mut Buffer, area: Rect, color: Color) {
    for y in area.top()..area.bottom() {
        buffer[(area.x, y)].set_symbol(line::VERTICAL).set_fg(color);
    }
}

/// What a window too small for the chrome gets instead of a layout.
pub fn too_small(buffer: &mut Buffer, area: Rect, theme: &Theme) {
    let min = min_size();
    Paragraph::new(format!(
        "Window too small — {}×{} needed",
        min.width, min.height
    ))
    .centered()
    .style(Style::default().fg(theme.muted_foreground))
    .render(area.centered_vertically(Constraint::Length(1)), buffer);
}
