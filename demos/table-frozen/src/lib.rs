//! A wide ledger with a frozen Date column and horizontally scrolling detail columns.
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Row},
};
use ratcn::{
    ListItem, SortDirection, Table, TableSort, Theme,
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
    row: Option<usize>,
    on_header: bool,
    selected: Option<usize>,
    column: usize,
    scroll_x: usize,
    sort: Option<TableSort>,
}

impl State {
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(f) => self.focus = f,
            Msg::Row(id) => {
                self.row = Some(id);
                self.on_header = false;
            }
            Msg::Header => self.on_header = true,
            Msg::Select(id) => {
                self.selected = Some(id);
                self.row = Some(id);
                self.on_header = false;
            }
            Msg::Column(c) => self.column = c,
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
    Row(usize),
    Header,
    Select(usize),
    Column(usize),
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
                row: Some(0),
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
                if sort.direction == SortDirection::Ascending {
                    cmp
                } else {
                    cmp.reverse()
                }
            });
        }
        let content = area.centered(
            Constraint::Length(44.min(area.width.saturating_sub(2))),
            Constraint::Length(11), // Header, eight visible rows, gap, status line.
        );
        let [grid, status] = content
            .layout(&Layout::vertical([Constraint::Length(9), Constraint::Length(1)]).spacing(1));
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
                .item_focus(|s: &State| s.row, |id, _| Msg::Row(id))
                .header_focus(|s| s.on_header, || Msg::Header)
                .selection(|s| s.selected, Msg::Select)
                .column_focus(|s| Some(s.column), Msg::Column)
                .horizontal_scroll(|s| s.scroll_x, Msg::ScrollX)
                .sort([0, 1, 2, 3, 4, 5], |s| s.sort, Msg::Sort);
                ctx.component("ledger-wide", table, grid);

                let hint = format!(
                    "Col {}/6 · ←/→ scroll columns · Date frozen",
                    self.state.column + 1
                );
                ctx.paint_widget(
                    Paragraph::new(hint).style(Style::default().fg(muted)),
                    status,
                );
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_scroll_updates_offset_without_moving_column() {
        let mut state = State {
            column: 1,
            ..State::default()
        };

        state.update(Msg::ScrollX(2));

        assert_eq!(state.scroll_x, 2);
        assert_eq!(state.column, 1);
    }
}
