//! A small ledger. Sort numbers as numbers; keep the cursor on the same entry.
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

// November day, payee and signed amount in cents.
const RECORDS: [(u8, &str, i32); 24] = [
    (24, "Market groceries", -6248),
    (23, "Bike repair", -3200),
    (22, "Payroll deposit", 240_000),
    (21, "Cinema tickets", -2800),
    (20, "North line rail", -1850),
    (19, "Lunch at Miro", -1630),
    (18, "Bakery", -650),
    (17, "Corner bookshop", -2400),
    (16, "Internet bill", -4900),
    (15, "Farm stand", -1240),
    (14, "Client payment", 78_000),
    (13, "Museum admission", -1500),
    (12, "Card refund", 1250),
    (11, "Flower shop", -1800),
    (10, "Park kiosk", -525),
    (9, "Apartment rent", -95_000),
    (8, "Sunday lunch", -3200),
    (7, "Pharmacy", -1420),
    (6, "Morning coffee", -475),
    (5, "Record shop", -2199),
    (4, "Gym membership", -4500),
    (3, "Cash deposit", 12_000),
    (2, "Concert tickets", -8500),
    (1, "Mobile plan", -2995),
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
                    _ => lhs.2.cmp(&rhs.2),
                };
                if sort.direction == SortDirection::Ascending {
                    cmp
                } else {
                    cmp.reverse()
                }
            });
        }
        let content = area.centered(
            Constraint::Length(48.min(area.width.saturating_sub(2))),
            Constraint::Length(11), // Header, eight visible transactions, gap, status line.
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
                        Constraint::Fill(1),
                        Constraint::Length(10),
                    ],
                )
                .column_spacing(2)
                .header([
                    Line::from("Date"),
                    Line::from("Transaction"),
                    Line::from("Amount").right_aligned(),
                ])
                .paint_item(move |_, item| {
                    let (day, name, amount) = RECORDS[*item.value];
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
                        Line::from(money(amount)).right_aligned(),
                    ])
                })
                .item_focus(|s: &State| s.row, |id, _| Msg::Row(id))
                .header_focus(|s| s.on_header, || Msg::Header)
                .selection(|s| s.selected, Msg::Select)
                .column_focus(|s| Some(s.column), Msg::Column)
                .sort([0, 1, 2], |s| s.sort, Msg::Sort);
                ctx.component("transactions", table, grid);

                let summary = match self.state.selected {
                    Some(id) => {
                        let (day, name, amount) = RECORDS[id];
                        format!("Selected: Nov {day:02} · {name} ({})", money(amount))
                    }
                    None => "Selected: None".to_owned(),
                };
                ctx.paint_widget(
                    Paragraph::new(summary).style(Style::default().fg(muted)),
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
            on_header: true,
            ..State::default()
        };

        state.update(Msg::Select(4));

        assert_eq!(state.selected, Some(4));
        assert_eq!(state.row, Some(4));
        assert!(!state.on_header);
    }

    #[test]
    fn sorting_updates_the_active_column_and_direction() {
        let mut state = State::default();
        let sort = TableSort {
            column: 2,
            direction: SortDirection::Ascending,
        };

        state.update(Msg::Sort(sort));

        assert_eq!(state.column, 2);
        assert_eq!(state.sort, Some(sort));
    }
}
