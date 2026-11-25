//! A wide ledger with a frozen Date column and horizontally scrolling detail columns.
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Row},
};
use ratcn::{
    ListItem, SortDirection, Table, TableCursor, TableSort, Theme,
    runtime::{Event, EventResult, FocusState, PointerShape, Ratcn},
};

// November day, payee, category, account, status, and signed amount in cents.
const RECORDS: [(u8, &str, &str, &str, &str, i32); 16] = [
    (24, "Market groceries", "Food", "Checking", "Cleared", -6248),
    (23, "Bike repair", "Transport", "Card", "Cleared", -3200),
    (
        22,
        "Payroll deposit",
        "Income",
        "Checking",
        "Cleared",
        240_000,
    ),
    (21, "Cinema tickets", "Leisure", "Card", "Pending", -2800),
    (20, "North line rail", "Transport", "Card", "Cleared", -1850),
    (19, "Lunch at Miro", "Food", "Card", "Cleared", -1630),
    (18, "Bakery", "Food", "Cash", "Cleared", -650),
    (17, "Corner bookshop", "Leisure", "Card", "Cleared", -2400),
    (16, "Internet bill", "Bills", "Checking", "Cleared", -4900),
    (15, "Farm stand", "Food", "Cash", "Cleared", -1240),
    (
        14,
        "Client payment",
        "Income",
        "Checking",
        "Cleared",
        78_000,
    ),
    (13, "Museum admission", "Leisure", "Card", "Cleared", -1500),
    (12, "Card refund", "Income", "Card", "Pending", 1250),
    (11, "Flower shop", "Home", "Cash", "Cleared", -1800),
    (10, "Park kiosk", "Food", "Cash", "Cleared", -525),
    (
        9,
        "Apartment rent",
        "Housing",
        "Checking",
        "Cleared",
        -95_000,
    ),
];

fn money(cents: i32) -> String {
    let sign = if cents < 0 { "-" } else { "+" };
    let value = cents.unsigned_abs();
    let dollars = value / 100;
    let whole = if dollars >= 1000 {
        format!("{},{:03}", dollars / 1000, dollars % 1000)
    } else {
        dollars.to_string()
    };
    format!("{sign}${whole}.{:02}", value % 100)
}

#[derive(Default)]
struct State {
    focus: FocusState,
    cursor: Option<TableCursor<usize>>,
    selected: Option<usize>,
    column: usize,
    scroll_x: usize,
    sort: Option<TableSort>,
}

impl State {
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(f) => self.focus = f,
            Msg::Cursor(cursor) => self.cursor = Some(cursor),
            Msg::Select(id) => {
                self.selected = Some(id);
                self.cursor = Some(TableCursor::Row(id));
            }
            Msg::Column(column, offset) => {
                self.column = column;
                self.scroll_x = offset;
            }
            Msg::ScrollX(offset) => self.scroll_x = offset,
            Msg::Sort(s) => {
                self.column = s.column;
                self.sort = Some(s);
            }
        }
    }
}

enum Msg {
    Focus(FocusState),
    Cursor(TableCursor<usize>),
    Select(usize),
    Column(usize, usize),
    ScrollX(usize),
    Sort(TableSort),
}

pub struct App {
    state: State,
    engine: Ratcn<State, Msg>,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: State {
                cursor: Some(TableCursor::Row(0)),
                sort: Some(TableSort {
                    column: 0,
                    direction: SortDirection::Descending,
                }),
                ..State::default()
            },
            engine: Ratcn::new().focus(|s: &State| &s.focus, Msg::Focus),
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl demo_shared::Demo for App {
    fn handle_event(&mut self, event: Event) -> bool {
        match self.engine.handle_event(event, &self.state) {
            EventResult::Emit(msg) => self.state.update(msg),
            EventResult::Consumed => (),
            EventResult::Ignored => return false,
        }
        true
    }

    fn pointer_shape(&self) -> PointerShape {
        self.engine.pointer_shape()
    }

    fn draw(&mut self, buffer: &mut Buffer, area: Rect, theme: &Theme) {
        let mut order = (0..RECORDS.len()).collect::<Vec<_>>();
        if let Some(sort) = self.state.sort {
            order.sort_by(|&a, &b| {
                let lhs = RECORDS[a];
                let rhs = RECORDS[b];
                let cmp = match sort.column {
                    0 => lhs.0.cmp(&rhs.0),
                    1 => lhs.1.cmp(rhs.1),
                    2 => lhs.2.cmp(rhs.2),
                    3 => lhs.3.cmp(rhs.3),
                    4 => lhs.4.cmp(rhs.4),
                    _ => lhs.5.cmp(&rhs.5),
                };
                sort.direction.apply(cmp)
            });
        }
        let content = area.centered(
            Constraint::Length(48.min(area.width.saturating_sub(2))),
            Constraint::Length(12), // Header, eight visible rows, gap, status and hint lines.
        );
        let [grid, status] = content
            .layout(&Layout::vertical([Constraint::Length(9), Constraint::Length(2)]).spacing(1));
        buffer.set_style(area, Style::default().bg(theme.background));
        let primary = theme.primary;
        let muted = theme.muted_foreground;
        self.engine
            .render_into(buffer, area, &self.state, theme, |ctx| {
                let table = Table::new(
                    order.iter().map(|&i| ListItem::new(i, RECORDS[i].1)),
                    [
                        Constraint::Length(6),
                        Constraint::Length(18),
                        Constraint::Length(11),
                        Constraint::Length(10),
                        Constraint::Length(9),
                        Constraint::Length(10),
                    ],
                )
                .column_spacing(2)
                .frozen_columns(1)
                .header([
                    Line::from("Date"),
                    Line::from("Transaction"),
                    Line::from("Category"),
                    Line::from("Account"),
                    Line::from("Status"),
                    Line::from("Amount").right_aligned(),
                ])
                .paint_item(move |_, item| {
                    let (day, name, category, account, state, amount) = RECORDS[*item.value];
                    let payee = if item.selected {
                        Line::from(vec![
                            Span::styled("● ", Style::new().fg(primary)),
                            Span::raw(name),
                        ])
                    } else {
                        Line::from(name)
                    };
                    Row::new([
                        Line::from(format!("Nov {day:02}")),
                        payee,
                        Line::from(category),
                        Line::from(account),
                        Line::from(state),
                        Line::from(money(amount)).right_aligned(),
                    ])
                })
                .item_focus(|s: &State| s.cursor, |cursor, _| Msg::Cursor(cursor))
                .selection(|s| s.selected, Msg::Select)
                .column_focus(|s| Some(s.column), Msg::Column)
                .horizontal_scroll(|s| s.scroll_x, Msg::ScrollX)
                .sort([0, 1, 2, 3, 4, 5], |s| s.sort, Msg::Sort)
                .sort_key('s');
                ctx.component("ledger-wide", table, grid);

                let summary = match self.state.selected {
                    Some(id) => {
                        let (day, name, _, _, _, amount) = RECORDS[id];
                        format!("Selected: Nov {day:02} · {name} ({})", money(amount))
                    }
                    None => "Selected: None".to_owned(),
                };
                ctx.paint_widget(
                    Paragraph::new(vec![Line::from(summary), Line::from("Press s to sort")])
                        .style(Style::default().fg(muted)),
                    status,
                );
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_a_row_moves_the_cursor_and_leaves_the_header() {
        let mut state = State {
            cursor: Some(TableCursor::Header),
            ..State::default()
        };

        state.update(Msg::Select(4));

        assert_eq!(state.selected, Some(4));
        assert_eq!(state.cursor, Some(TableCursor::Row(4)));
    }

    #[test]
    fn column_focus_updates_column_and_horizontal_scroll_together() {
        let mut state = State::default();

        state.update(Msg::Column(4, 2));

        assert_eq!(state.column, 4);
        assert_eq!(state.scroll_x, 2);
    }

    #[test]
    fn sorting_updates_the_active_column_and_direction() {
        let mut state = State::default();
        let sort = TableSort {
            column: 5,
            direction: SortDirection::Ascending,
        };

        state.update(Msg::Sort(sort));

        assert_eq!(state.column, 5);
        assert_eq!(state.sort, Some(sort));
    }
}
