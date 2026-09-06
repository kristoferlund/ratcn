//! The Demos view: every demo by name on the left, the one under the cursor on
//! the right.

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    symbols::line,
    text::{Line, Text},
    widgets::Paragraph,
};
use ratcn::{List, ListItem, ListStyle, Theme, runtime::DeclareCtx};

use crate::{AppState, Msg, catalog, chrome};

/// The nav list's child id, and what focus names to put the arrows in reach.
pub const LIST_ID: &str = "catalog";

/// What the three lines under the list say, and the keys they name.
const HINTS: [(&str, &str); 3] = [("↑ ↓", "browse"), ("enter", "interact"), ("esc", "back")];

/// Cells the hints' key column takes: the longest key name above.
const HINT_KEY_WIDTH: usize = 5;

/// The nav column and the key hints below it.
///
/// The cursor *is* the selection here — moving it swaps the demo on the right —
/// so the list binds `item_focus` and nothing else, which is also what keeps
/// the selection markers a bound `selection` would bring off the rows.
pub fn declare(ctx: &mut DeclareCtx<'_, AppState, Msg>, nav: Rect, live: bool) {
    let [list_area, rule_area, hints_area] = nav.layout(&Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(HINTS.len() as u16),
    ]));

    // A list paints its cursor row only while it has focus or hover, and the
    // selected demo has to stay readable once focus is in the demo pane. So the
    // row draws itself, in the fill the theme would have given it, padded to the
    // full width because a `Line` colors the cells it covers and no more.
    let style = nav_style(ctx.theme);
    let width = usize::from(list_area.width);
    let list = List::new(
        catalog::ENTRIES
            .iter()
            .enumerate()
            .map(|(index, entry)| ListItem::new(index, entry.name)),
    )
    .item_focus(|state: &AppState| Some(state.selected), Msg::DemoFocused)
    .scroll(|state: &AppState| state.scroll, Msg::DemoScrolled)
    .paint_item(move |state: &AppState, row| {
        let label = format!(" {:<pad$}", row.label, pad = width.saturating_sub(1));
        if row.index == state.selected {
            Line::styled(
                label,
                Style::default()
                    .fg(style.focused_foreground)
                    .bg(style.focused_row_background),
            )
        } else {
            Line::from(label)
        }
    })
    .style(nav_style);
    ctx.component(LIST_ID, list, list_area);

    let muted = Style::default().fg(ctx.theme.muted_foreground);
    let border = ctx.theme.border;
    // The vertical rule stands in the column immediately right of the nav.
    let junction = chrome::rule_color(ctx.theme, live);
    ctx.paint(move |ctx| {
        ctx.with_buffer(|buffer| {
            chrome::horizontal_rule(buffer, rule_area, border);
            buffer[(rule_area.right(), rule_area.y)]
                .set_symbol(line::VERTICAL_LEFT)
                .set_fg(junction);
        });
    });
    let hints = Text::from(
        HINTS
            .map(|(key, label)| Line::from(format!(" {key:<HINT_KEY_WIDTH$} {label}")))
            .to_vec(),
    );
    ctx.paint_widget(Paragraph::new(hints).style(muted), hints_area);
}

/// The nav list's colors: the theme's list style, standing on the page instead
/// of in a well.
///
/// A list is a control and fills itself with the theme's field color, which
/// here would box the names off from the hints below them and read as
/// something to operate rather than as navigation. Only the three backdrops
/// move; the cursor row keeps the fill the theme gave it, and is then the one
/// fill in the column.
fn nav_style(theme: &Theme) -> ListStyle {
    let mut style = ListStyle::from_theme(theme);
    style.background = theme.background;
    style.focused_background = theme.background;
    style.hovered_background = theme.background;
    style
}
