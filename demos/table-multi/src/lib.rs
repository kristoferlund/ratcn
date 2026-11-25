//! Multi-selection table: toggle any number of expenses and total the selection.
use std::collections::HashSet;

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

// November day, expense, and amount in cents.
const EXPENSES: [(u8, &str, u32); 14] = [
    (24, "Flight to Stockholm", 28_400),
    (23, "Hotel (2 nights)", 42_000),
    (22, "Airport rail pass", 3_250),
    (21, "Team dinner", 14_800),
    (20, "Conference badge", 35_000),
    (19, "Client lunch", 6_450),
    (18, "Coworking day pass", 4_500),
    (17, "USB-C dock", 8_990),
    (16, "Cloud test cluster", 11_200),
    (15, "Taxi to station", 2_600),
    (14, "Domain renewal", 1_800),
    (13, "Notebook & pens", 1_450),
    (12, "Coffee with partner", 1_120),
    (11, "Postage & courier", 2_380),
];

fn money(cents: u32) -> String {
    let dollars = cents / 100;
    let whole = if dollars >= 1000 {
        format!("{},{:03}", dollars / 1000, dollars % 1000)
    } else {
        dollars.to_string()
    };
    format!("${whole}.{:02}", cents % 100)
}

#[derive(Default)]
struct State {
    focus: FocusState,
    cursor: Option<TableCursor<usize>>,
    selected: HashSet<usize>,
    column: usize,
    sort: Option<TableSort>,
}

impl State {
    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::Focus(f) => self.focus = f,
            Msg::Cursor(cursor) => self.cursor = Some(cursor),
            Msg::Toggle(id) => {
                self.cursor = Some(TableCursor::Row(id));
                if !self.selected.remove(&id) {
                    self.selected.insert(id);
                }
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
    Cursor(TableCursor<usize>),
    Toggle(usize),
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
                cursor: Some(TableCursor::Row(0)),
                selected: HashSet::from([0, 2]),
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
        let mut order = (0..EXPENSES.len()).collect::<Vec<_>>();
        if let Some(sort) = self.state.sort {
            order.sort_by(|&a, &b| {
                let lhs = EXPENSES[a];
                let rhs = EXPENSES[b];
                let cmp = match sort.column {
                    0 => lhs.0.cmp(&rhs.0),
                    1 => lhs.1.cmp(rhs.1),
                    _ => lhs.2.cmp(&rhs.2),
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
                    order.iter().map(|&i| ListItem::new(i, EXPENSES[i].1)),
                    [
                        Constraint::Length(6),
                        Constraint::Fill(1),
                        Constraint::Length(10),
                    ],
                )
                .column_spacing(2)
                .header([
                    Line::from("Date"),
                    Line::from("Expense"),
                    Line::from("Amount").right_aligned(),
                ])
                .paint_item(move |_, item| {
                    let (day, name, amount) = EXPENSES[*item.value];
                    let marker = if item.selected {
                        Span::styled("■ ", Style::new().fg(primary))
                    } else {
                        Span::styled("□ ", Style::new().fg(muted))
                    };
                    Row::new([
                        Line::from(format!("Nov {day:02}")),
                        Line::from(vec![marker, Span::raw(name)]),
                        Line::from(money(amount)).right_aligned(),
                    ])
                })
                .item_focus(|s: &State| s.cursor, |cursor, _| Msg::Cursor(cursor))
                .multi_selection(|s, id| s.selected.contains(id), Msg::Toggle)
                .column_focus(|s| Some(s.column), |column, _| Msg::Column(column))
                .sort([0, 1, 2], |s| s.sort, Msg::Sort)
                .sort_key('s');
                ctx.component("expenses", table, grid);

                let total: u32 = self.state.selected.iter().map(|&id| EXPENSES[id].2).sum();
                let summary = if self.state.selected.is_empty() {
                    format!("Selected: 0 of {}", EXPENSES.len())
                } else {
                    format!(
                        "Selected: {} of {} ({})",
                        self.state.selected.len(),
                        EXPENSES.len(),
                        money(total)
                    )
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
    fn toggling_a_row_twice_returns_it_to_unselected() {
        let mut state = State::default();

        state.update(Msg::Toggle(3));
        assert!(state.selected.contains(&3));

        state.update(Msg::Toggle(3));
        assert!(!state.selected.contains(&3));
    }

    #[test]
    fn toggling_a_row_moves_the_cursor_off_the_header() {
        let mut state = State {
            cursor: Some(TableCursor::Header),
            ..State::default()
        };

        state.update(Msg::Toggle(4));

        assert!(state.selected.contains(&4));
        assert_eq!(state.cursor, Some(TableCursor::Row(4)));
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
