//! Recorded frames of the paint-only `ListWidget` and `SelectWidget`, driven
//! through their public index-based builders the way a plain-ratatui app
//! drives them.
//!
//! The frames were recorded from the widgets as they painted before they
//! shared a row painter with each other; a plain-ratatui caller must see the
//! same cells, colors, and modifiers for the same inputs, whatever the widgets
//! do internally. Set `RATCN_RECORD=1` to rewrite the recording on purpose.

use std::fmt::Write as _;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Widget,
};
use ratcn::{ListStyle, ListWidget, SelectStyle, SelectWidget, Theme};

const RECORDING: &str = "tests/golden/paint_only_widgets.txt";

/// Every symbol on one line per row, then a style letter per cell, then the
/// style each letter stands for.
fn styled_snapshot(buffer: &Buffer) -> String {
    let mut styles = Vec::new();
    let mut symbols = String::new();
    let mut letters = String::new();
    for y in buffer.area.top()..buffer.area.bottom() {
        for x in buffer.area.left()..buffer.area.right() {
            let cell = buffer.cell((x, y)).expect("cell");
            let style = (cell.fg, cell.bg, cell.modifier);
            let index = styles
                .iter()
                .position(|seen| *seen == style)
                .unwrap_or_else(|| {
                    styles.push(style);
                    styles.len() - 1
                });
            symbols.push_str(cell.symbol());
            letters.push(char::from(b'a' + u8::try_from(index).expect("few styles")));
        }
        // Closed off, so a row's trailing blanks survive an editor.
        symbols.push_str("|\n");
        letters.push('\n');
    }
    let mut snapshot = symbols + &letters;
    for (index, (fg, bg, modifier)) in styles.iter().enumerate() {
        let letter = char::from(b'a' + u8::try_from(index).expect("few styles"));
        writeln!(snapshot, "{letter}: {fg} on {bg} {modifier:?}").expect("infallible");
    }
    snapshot
}

fn paint(width: u16, height: u16, draw: impl FnOnce(Rect, &mut Buffer)) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    draw(area, &mut buffer);
    styled_snapshot(&buffer)
}

fn texts(labels: &[&str]) -> Vec<Text<'static>> {
    labels
        .iter()
        .map(|label| Text::from(label.to_string()))
        .collect()
}

/// Rows that set their own colors and modifiers, which the widgets must keep.
fn styled_rows() -> Vec<Text<'static>> {
    vec![
        Text::from(Span::styled(
            "bold",
            Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        )),
        Text::from(Line::from(vec![
            Span::raw("half "),
            Span::styled("filled", Style::new().bg(Color::Green)),
        ])),
        Text::from(Span::styled(
            "italic",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::ITALIC),
        )),
        Text::from("plain"),
    ]
}

#[expect(clippy::too_many_lines, reason = "a flat list of scenarios")]
fn list_scenarios() -> Vec<(&'static str, String)> {
    let dark = Theme::default_dark();
    let nord = Theme::nord();
    let five = texts(&["one", "two", "three", "four", "five"]);
    let window = texts(&["three", "four", "five"]);
    let mut scenarios = Vec::new();
    let mut add = |name, frame| scenarios.push((name, frame));

    add(
        "list: bare rows, fallback style",
        paint(10, 4, |area, buf| {
            ListWidget::new(&five[..3]).render(area, buf);
        }),
    );
    add(
        "list: focused cursor, symbol, selection, disabled mask",
        paint(14, 6, |area, buf| {
            ListWidget::new(&five)
                .focused_item(Some(1))
                .selected_items(&[1, 3])
                .disabled_items(&[false, false, true])
                .focused(true)
                .focus_symbol("> ")
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "list: unfocused shows no cursor and no gutter",
        paint(14, 5, |area, buf| {
            ListWidget::new(&five)
                .focused_item(Some(1))
                .selected_items(&[1])
                .focus_symbol("> ")
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "list: focused without a cursor item reserves no gutter",
        paint(14, 5, |area, buf| {
            ListWidget::new(&five)
                .selected_items(&[2])
                .focused(true)
                .focus_symbol("> ")
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "list: scrolled window lines up with list indices",
        paint(14, 4, |area, buf| {
            ListWidget::new(&window)
                .first_item(2)
                .focused_item(Some(3))
                .selected_items(&[2, 4])
                .disabled_items(&[false, false, false, false, true])
                .focused(true)
                .focus_symbol("> ")
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "list: cursor scrolled out of the window keeps the gutter",
        paint(14, 4, |area, buf| {
            ListWidget::new(&window)
                .first_item(2)
                .focused_item(Some(0))
                .selected_items(&[3])
                .focused(true)
                .focus_symbol("> ")
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "list: disabled overrides focus and rows",
        paint(14, 4, |area, buf| {
            ListWidget::new(&window)
                .first_item(2)
                .focused_item(Some(3))
                .selected_items(&[2])
                .focused(true)
                .disabled(true)
                .focus_symbol("> ")
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "list: hovered only",
        paint(12, 4, |area, buf| {
            ListWidget::new(&five[..3])
                .focused_item(Some(0))
                .selected_items(&[1])
                .hovered(true)
                .focus_symbol(">")
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "list: hovered and focused",
        paint(12, 4, |area, buf| {
            ListWidget::new(&five[..3])
                .focused_item(Some(1))
                .selected_items(&[1])
                .hovered(true)
                .focused(true)
                .focus_symbol(">")
                .themed(&nord)
                .render(area, buf);
        }),
    );
    add(
        "list: mixed row heights, last row cut by the area",
        paint(12, 5, |area, buf| {
            let rows = vec![
                Text::from(vec![Line::from("tall"), Line::from(" second")]),
                Text::from("short"),
                Text::default(),
                Text::from(vec![
                    Line::from("cut"),
                    Line::from(" off"),
                    Line::from(" gone"),
                ]),
            ];
            ListWidget::new(&rows)
                .focused_item(Some(0))
                .selected_items(&[3])
                .focused(true)
                .focus_symbol("*")
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "list: explicit text styles survive every row state",
        paint(14, 5, |area, buf| {
            ListWidget::new(&styled_rows())
                .focused_item(Some(1))
                .selected_items(&[0, 1])
                .disabled_items(&[false, false, true])
                .focused(true)
                .focus_symbol("> ")
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "list: explicit style, wide symbol",
        paint(10, 3, |area, buf| {
            let mut style = ListStyle::fallback();
            style.background = Color::Blue;
            style.focused_background = Color::Rgb(1, 2, 3);
            style.selected_foreground = Color::Yellow;
            ListWidget::new(&five[..3])
                .focused_item(Some(2))
                .selected_items(&[0])
                .focused(true)
                .focus_symbol("👉")
                .style(style)
                .render(area, buf);
        }),
    );
    add(
        "list: symbol wider than the area",
        paint(3, 3, |area, buf| {
            ListWidget::new(&five[..3])
                .focused_item(Some(1))
                .focused(true)
                .focus_symbol(">>>> ")
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "list: area partly outside the buffer",
        paint(10, 3, |_, buf| {
            ListWidget::new(&five)
                .focused_item(Some(1))
                .focused(true)
                .focus_symbol("> ")
                .themed(&dark)
                .render(Rect::new(4, 1, 20, 20), buf);
        }),
    );
    add(
        "list: empty area paints nothing",
        paint(6, 2, |_, buf| {
            ListWidget::new(&five)
                .focused(true)
                .themed(&dark)
                .render(Rect::new(1, 1, 0, 1), buf);
        }),
    );
    scenarios
}

#[expect(clippy::too_many_lines, reason = "a flat list of scenarios")]
fn select_scenarios() -> Vec<(&'static str, String)> {
    let dark = Theme::default_dark();
    let solarized = Theme::solarized();
    let fruits = ["Mango", "Papaya", "Lychee", "Durian", "Guava"];
    let mut scenarios = Vec::new();
    let mut add = |name, frame| scenarios.push((name, frame));

    add(
        "select: closed with a value, fallback style",
        paint(14, 2, |area, buf| {
            SelectWidget::new(Some("Mango")).render(area, buf);
        }),
    );
    add(
        "select: closed placeholder, focused",
        paint(14, 1, |area, buf| {
            SelectWidget::new(None)
                .placeholder("Pick a fruit")
                .focused(true)
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "select: closed, hovered, long value truncated",
        paint(10, 1, |area, buf| {
            SelectWidget::new(Some("A very long fruit"))
                .hovered(true)
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "select: open and disabled paints only the trigger",
        paint(14, 5, |area, buf| {
            SelectWidget::new(Some("Mango"))
                .open(true)
                .options(&fruits)
                .disabled(true)
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "select: open, default markers, cursor, selection, disabled",
        paint(16, 8, |area, buf| {
            SelectWidget::new(Some("Papaya"))
                .open(true)
                .options(&fruits)
                .focused_item(Some(2))
                .selected_item(Some(1))
                .disabled_items(&[false, false, false, true])
                .focused(true)
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "select: cursor on the selected option",
        paint(16, 6, |area, buf| {
            SelectWidget::new(Some("Mango"))
                .open(true)
                .options(&fruits[..3])
                .focused_item(Some(0))
                .selected_item(Some(0))
                .themed(&solarized)
                .render(area, buf);
        }),
    );
    add(
        "select: scrolled from first_item",
        paint(16, 6, |area, buf| {
            SelectWidget::new(Some("Papaya"))
                .open(true)
                .options(&fruits)
                .first_item(1)
                .focused_item(Some(2))
                .selected_item(Some(1))
                .disabled_items(&[false, false, false, true])
                .focused(true)
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "select: first_item near the end leaves blank panel rows",
        paint(16, 8, |area, buf| {
            SelectWidget::new(None)
                .open(true)
                .options(&fruits)
                .first_item(3)
                .focused_item(Some(4))
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "select: custom markers",
        paint(16, 6, |area, buf| {
            SelectWidget::new(Some("Mango"))
                .open(true)
                .options(&fruits[..3])
                .selected_marker("[x]")
                .unselected_marker("[ ]")
                .selected_item(Some(0))
                .focused_item(Some(1))
                .disabled_items(&[false, false, true])
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "select: panel cut to the whole options that fit",
        paint(16, 5, |area, buf| {
            SelectWidget::new(None)
                .open(true)
                .options(&fruits)
                .focused_item(Some(1))
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "select: too short for any option",
        paint(16, 3, |area, buf| {
            SelectWidget::new(None)
                .open(true)
                .options(&fruits)
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "select: two-row item rows, padded, clipped, and missing",
        paint(16, 11, |area, buf| {
            let rows = vec![
                Text::from(vec![Line::from("Mango"), Line::from(" sweet")]),
                Text::from("Papaya"),
                Text::from(vec![
                    Line::from("Lychee"),
                    Line::from(" pink"),
                    Line::from(" gone"),
                ]),
            ];
            SelectWidget::new(None)
                .placeholder("Pick")
                .open(true)
                .options(&fruits[..4])
                .visible_item_rows(&rows)
                .row_height(2)
                .focused_item(Some(0))
                .selected_item(Some(1))
                .disabled_items(&[false, false, true])
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "select: item rows from first_item with explicit styles",
        paint(16, 7, |area, buf| {
            let rows = styled_rows();
            SelectWidget::new(Some("Papaya"))
                .open(true)
                .options(&fruits)
                .visible_item_rows(&rows[..3])
                .first_item(2)
                .focused_item(Some(3))
                .selected_item(Some(2))
                .disabled_items(&[false, false, false, false, true])
                .themed(&dark)
                .render(area, buf);
        }),
    );
    add(
        "select: row_height 0 reads as 1, explicit style",
        paint(12, 5, |area, buf| {
            let mut style = SelectStyle::fallback();
            style.panel_background = Color::Blue;
            style.focused_option_background = Color::Rgb(9, 8, 7);
            style.selected_marker = Color::Red;
            SelectWidget::new(Some("Mango"))
                .open(true)
                .options(&fruits[..2])
                .row_height(0)
                .focused_item(Some(1))
                .selected_item(Some(0))
                .style(style)
                .render(area, buf);
        }),
    );
    add(
        "select: narrow trigger",
        paint(2, 4, |area, buf| {
            SelectWidget::new(Some("Mango"))
                .open(true)
                .options(&fruits[..1])
                .themed(&dark)
                .render(area, buf);
        }),
    );
    scenarios
}

#[test]
fn paint_only_widgets_paint_as_recorded() {
    let mut painted = String::new();
    for (name, frame) in list_scenarios().into_iter().chain(select_scenarios()) {
        write!(painted, "== {name}\n{frame}").expect("infallible");
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(RECORDING);
    if std::env::var_os("RATCN_RECORD").is_some() {
        std::fs::create_dir_all(path.parent().expect("dir")).expect("create golden dir");
        std::fs::write(&path, &painted).expect("write golden");
        return;
    }
    let recorded = std::fs::read_to_string(&path).expect("golden recording");
    let split = |text: &str| -> Vec<String> {
        text.split("== ")
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let (painted, recorded) = (split(&painted), split(&recorded));
    assert_eq!(painted.len(), recorded.len(), "scenario count");
    for (painted, recorded) in painted.iter().zip(&recorded) {
        assert_eq!(painted, recorded, "a frame changed");
    }
}
