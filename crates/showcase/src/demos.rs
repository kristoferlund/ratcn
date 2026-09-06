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
use ratcn::{List, ListItem, ListStyle, Theme, runtime::DeclareCtx, text_width::display_width_u16};

use crate::{AppState, Msg, catalog, chrome};

/// The nav list's child id, and what focus names to put the arrows in reach.
pub const NAV_ID: &str = "nav";

/// A column of breathing room on each side of the longest demo name, and the
/// indent every row of the column is written at.
const NAV_PADDING: u16 = 1;

/// The narrowest demo pane worth painting: below this every demo is clipped to
/// a stripe, and the browser is showing nothing anyone can read.
const MIN_PANE_WIDTH: u16 = 20;

/// What the four lines under the list say, and the keys they name.
///
/// Enter is the list's own — a bound selection consumes it — so entering the
/// demo pane is Right alone.
const HINTS: [(&str, &str); 4] = [
    ("↑ ↓", "browse"),
    ("enter", "show"),
    ("→", "interact"),
    ("esc", "back"),
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
        Constraint::Length(nav_width(body.width)),
        Constraint::Length(1),
        Constraint::Fill(1),
    ]));
    Columns { nav, rule, pane }
}

/// The narrowest body this view lays out in: the nav column at its full width,
/// the rule, and a pane worth painting.
pub fn min_width() -> u16 {
    nav_text_width() + 1 + MIN_PANE_WIDTH
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

/// The longest demo name with a column of padding on each side.
fn nav_text_width() -> u16 {
    catalog::widest_name() + 2 * NAV_PADDING
}

/// What the nav column takes of `total`, capped at half of it so a narrow
/// terminal keeps a demo pane rather than a list of names.
fn nav_width(total: u16) -> u16 {
    nav_text_width().min(total / 2)
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

/// The nav column and the key hints below it.
///
/// Both halves of the list are bound, because browsing and choosing are two
/// different things here: `item_focus` moves the cursor and shows nothing new,
/// and only `selection` — Enter, or a click — changes the demo on the right.
///
/// Rows draw themselves, which is what keeps the selection markers a bound
/// `selection` would otherwise bring off them, and what lets the committed row
/// stay readable once focus has moved into the demo pane: a list paints its own
/// cursor row only while it has focus or hover.
pub fn declare(ctx: &mut DeclareCtx<'_, AppState, Msg>, nav: Rect) {
    let [list_area, _, hints_area] = rows(nav);
    let indent = usize::from(NAV_PADDING);

    // Two fills, one step apart on the theme's own ladder: the committed row
    // takes the deeper one, the cursor the shallower, so a place and a choice
    // do not read as two choices. Both are padded to the full width, because a
    // `Line` colors the cells it covers and no more.
    let style = nav_style(ctx.theme);
    let cursor_background = ListStyle::from_theme(ctx.theme).focused_background;
    let rest = usize::from(list_area.width).saturating_sub(indent);
    let list = List::new(
        catalog::ENTRIES
            .iter()
            .enumerate()
            .map(|(index, entry)| ListItem::new(index, entry.name)),
    )
    .item_focus(|state: &AppState| Some(state.cursor), Msg::NavFocused)
    .selection(|state: &AppState| Some(state.showing), Msg::NavSelected)
    .scroll(|state: &AppState| state.nav_scroll, Msg::NavScrolled)
    .paint_item(move |state: &AppState, row| {
        let label = format!("{:indent$}{:<rest$}", "", row.label);
        let fill = if row.index == state.showing {
            Some(style.focused_row_background)
        } else if row.focused {
            Some(cursor_background)
        } else {
            None
        };
        match fill {
            Some(background) => Line::styled(
                label,
                Style::default().fg(style.focused_foreground).bg(background),
            ),
            None => Line::from(label),
        }
    })
    .style(nav_style);
    ctx.component(NAV_ID, list, list_area);

    let key_width = usize::from(hint_key_width());
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

/// Cells the hints' key column takes: the widest key above, measured rather
/// than counted, since `↑ ↓` is three cells of seven bytes.
fn hint_key_width() -> u16 {
    HINTS
        .iter()
        .map(|(key, _)| display_width_u16(key))
        .max()
        .unwrap_or(0)
}

/// The nav list's colors: the theme's list style, standing on the page instead
/// of in a well.
///
/// A list is a control and fills itself with the theme's field color, which
/// here would box the names off from the hints below them and read as
/// something to operate rather than as navigation. Only the three backdrops
/// move; the row fills the theme gave it are what `declare` paints the cursor
/// and the committed row with, and are then the only fills in the column.
fn nav_style(theme: &Theme) -> ListStyle {
    let mut style = ListStyle::from_theme(theme);
    style.background = theme.background;
    style.focused_background = theme.background;
    style.hovered_background = theme.background;
    style
}
