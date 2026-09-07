//! The Demos view: every demo by name on the left, the one committed on the
//! right, and the rules between them.

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    symbols::line,
    text::{Line, Text},
    widgets::Paragraph,
};
use ratcn::{List, ListItem, Theme, runtime::DeclareCtx, text_width::display_width_u16};

use crate::{AppState, Msg, catalog, chrome};

/// The nav list's child id, and what focus names to put the arrows in reach.
pub const NAV_ID: &str = "nav";

const HINT_INDENT: u16 = 1;

/// The narrowest demo pane worth painting: below this every demo is clipped to
/// a stripe, and the browser is showing nothing anyone can read.
const MIN_PANE_WIDTH: u16 = 20;

/// Enter commits the list row; Right enters the selected demo.
const HINTS: [(&str, &str); 4] = [
    ("↑ ↓", "browse"),
    ("enter", "show"),
    ("→", "into the demo"),
    ("esc", "leave the demo"),
];

/// The vertical bands of the view.
pub struct Columns {
    /// The demo names and the key hints.
    pub nav: Rect,
    /// The one-cell rule between them and the demo.
    pub rule: Rect,
    /// Where the demo that is showing paints.
    pub pane: Rect,
}

/// Split the body into the nav column, its rule, and the pane.
pub fn columns(body: Rect) -> Columns {
    let [nav, rule, pane] = body.layout(&Layout::horizontal([
        Constraint::Length(nav_width()),
        Constraint::Length(1),
        Constraint::Fill(1),
    ]));
    Columns { nav, rule, pane }
}

/// The narrowest body this view lays out in: the nav column at its full width,
/// the rule, and a pane worth painting.
pub fn min_width() -> u16 {
    nav_width() + 1 + MIN_PANE_WIDTH
}

/// The rows the nav column owes: one list row, the rule above the hints, and
/// the hints themselves.
pub fn min_body_height() -> u16 {
    1 + 1 + HINTS.len() as u16
}

/// The nav column's rows: the list, the rule under it, and the key hints.
fn rows(nav: Rect) -> [Rect; 3] {
    nav.layout(&Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(HINTS.len() as u16),
    ]))
}

/// Standard List selection rows prepend a space, a marker, and a space.
fn nav_width() -> u16 {
    catalog::widest_name() + 3
}

/// Both of this view's rules and both of their junctions.
///
/// The vertical rule and the tees that meet it come from one derivation of its
/// column, so nothing has to be kept in agreement by hand. `live` says the demo
/// beside it has the input, which is what its color tells the user.
pub fn separators(
    buffer: &mut Buffer,
    bands: &chrome::Bands,
    columns: &Columns,
    theme: &Theme,
    live: bool,
) {
    let x = columns.rule.x;
    let color = chrome::rule_color(theme, live);
    let [_, hint_rule, _] = rows(columns.nav);

    chrome::horizontal_rule(buffer, hint_rule, theme.border);
    chrome::vertical_rule(
        buffer,
        Rect::new(x, bands.body.y, 1, bands.body.height),
        color,
    );
    buffer[(x, bands.rule.y)]
        .set_symbol(line::HORIZONTAL_DOWN)
        .set_fg(color);
    buffer[(x, hint_rule.y)]
        .set_symbol(line::VERTICAL_LEFT)
        .set_fg(color);
}

/// Standard List cursor styling shows browsing; its marker shows the committed demo.
pub fn declare(ctx: &mut DeclareCtx<'_, AppState, Msg>, nav: Rect) {
    let [list_area, _, hints_area] = rows(nav);
    let list = List::new(
        catalog::ENTRIES
            .iter()
            .enumerate()
            .map(|(index, entry)| ListItem::new(index, entry.name)),
    )
    .item_focus(|state: &AppState| Some(state.cursor), Msg::NavFocused)
    .selection(|state: &AppState| Some(state.showing), Msg::NavSelected)
    .scroll(|state: &AppState| state.nav_scroll, Msg::NavScrolled);
    ctx.component(NAV_ID, list, list_area);

    let key_width = usize::from(hint_key_width());
    let indent = usize::from(HINT_INDENT);
    let hints = Text::from(
        HINTS
            .map(|(key, label)| Line::from(format!("{:indent$}{key:<key_width$} {label}", "")))
            .to_vec(),
    );
    ctx.paint_widget(
        Paragraph::new(hints).style(Style::default().fg(ctx.theme.muted_foreground)),
        hints_area,
    );
}

/// Rows the nav list has on screen in a body `height` rows tall — what a
/// reveal has to fit its row into.
pub fn visible_rows(nav: Rect) -> u16 {
    rows(nav)[0].height
}

/// Cells the hints' key column takes: the widest key above, measured rather
/// than counted, since `↑ ↓` is three cells of seven bytes.
fn hint_key_width() -> u16 {
    HINTS
        .iter()
        .map(|(key, _)| display_width_u16(key))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_minimum_window_keeps_the_full_nav_and_a_readable_pane() {
        let columns = columns(Rect::new(0, 0, min_width(), min_body_height()));
        assert_eq!(columns.nav.width, nav_width());
        assert_eq!(columns.pane.width, MIN_PANE_WIDTH);
    }

    /// The hints are laid out in the nav column's own width, so a label that
    /// outgrew it would be silently clipped rather than wrapped.
    #[test]
    fn every_hint_fits_the_column_it_is_painted_in() {
        let room = nav_width() - HINT_INDENT - hint_key_width() - 1;
        for (key, label) in HINTS {
            assert!(
                display_width_u16(label) <= room,
                "{key} {label:?} needs {} of {room} cells",
                display_width_u16(label)
            );
        }
    }
}
