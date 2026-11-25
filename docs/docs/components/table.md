---
description: "A scrollable, sortable table for Ratatui apps with keyed row selection, column focus, frozen columns, and horizontal scrolling."
---

# Table

A scrollable, focusable table of value-keyed records with fixed headers and
footers, optional column focus, and sortable columns.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 420px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p table</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/table-demo/index.html" title="ratcn table demo"></iframe>
  </div>
</div>

```rust
use ratatui::{layout::Constraint, widgets::Row};
use ratcn::{ListItem, Table, TableSort};

let table = Table::new(
    items,
    [Constraint::Fill(1), Constraint::Length(12)],
)
.header(["Description", "Amount"])
.paint_item(|s: &AppState, row| s.transaction_row(*row.value))
.item_focus(|s| s.cursor, Msg::RowFocused)
.header_focus(|s| s.on_header, || Msg::HeaderFocused)
.selection(|s| s.selected, Msg::RowSelected)
.column_focus(|s| Some(s.column), Msg::ColumnFocused)
.sort([0, 1], |s| s.sort, Msg::SortChanged);

ctx.component("transactions", table, area);
```

Rows use the same value-keyed `ListItem` as [List](./list). Identifying each
row by its domain value keeps the cursor and selection on the same record across
sorting and filtering. Values must be unique within one `Table`; a debug build
panics on duplicates.

`item_focus` tracks the row cursor, `selection` commits a choice on Enter, Space,
or click, and `column_focus` tracks the active column for Left/Right movement
and cell highlighting. `paint_item` receives app state and a `ListItemState`,
returning a Ratatui `Row` for each visible item. Explicit foreground colors and
modifiers set on `Row`, `Cell`, or `Span` values are preserved over the row and
cursor styles.

## Sorting and header focus

Pass the sortable column indices and a `(read, change)` binding to `.sort(...)`.
Clicking a sortable heading emits a `TableSort` requesting ascending order, or
descending if that column is already sorted ascending. Your app reorders the
records and passes them back in the new order; the sorted heading displays a
`▲` or `▼` mark when it fits alongside the label.

Bind `.header_focus(read, on_enter)` to reach the header from the keyboard.
Stepping up from the first row moves the cursor onto the header, where Left and
Right choose a column, Enter or Space sorts it, and Down returns to the rows.
Set `on_header` in `on_enter`'s update and clear it when handling row focus or
selection messages.

## Frozen columns and horizontal scrolling

When the declared column widths exceed the table's inner width, columns scroll
horizontally to keep the active column visible. Horizontal wheel input scrolls
columns without moving the active column, and bubbles to parent containers at
both ends.

Call `.frozen_columns(n)` to pin the first `n` columns on the left. Once
scrolled right past them, a `│` separator is drawn in the spacing gap beside
the frozen columns across header, body, and footer. Bind
`.horizontal_scroll(read, change)` when your app needs to observe or drive the
column scroll offset.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 420px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p table-frozen</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/table-frozen-demo/index.html" title="ratcn table frozen columns demo"></iframe>
  </div>
</div>

```rust
let table = Table::new(
    items,
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
.column_focus(|s: &AppState| Some(s.column), Msg::ColumnFocused)
.horizontal_scroll(|s| s.scroll_x, Msg::ScrollXChanged);
```

## Layout and long tables

Use `.header(...)`, `.footer(...)`, `.block(...)`, `.flex(...)`, and
`.column_spacing(...)` to configure table layout. Headings and footer cells are
Ratatui `Line`s and keep their own alignment and spans.

Set `.row_height(n)` for multi-line body rows. Bind `.scroll(read, change)`
when something outside the table needs the vertical scroll offset; unbound
tables still scroll with keys and the wheel.

For large collections, keep the items in app state as an `Arc<[ListItem<T>]>`
and construct the table with `Table::from_shared(Arc::clone(&state.rows), widths)`
so declarations do not copy the items each frame. `paint_item` is called only
for rows in the visible viewport.

## Disabled

`ListItem::disabled(true)` dims one row and skips it for keys and clicks.
`.disabled(true)` disables the whole table and removes it from Tab traversal.

```rust
ListItem::new(record.id, &record.name).disabled(record.locked)
```

## Styling

`TableStyle` controls the well, row, cursor, selection, header, and disabled
colors derived from the active theme. When `column_focus` is bound, the cursor
row uses `focused_band_background` with the active cell highlighted in
`focused_row_background`, and the active column's heading uses
`active_header_foreground`.

Override colors per table with `.style(...)`. The closure receives the active
theme each render:

```rust
use ratcn::TableStyle;

Table::new(items, widths).style(|theme| {
    let mut style = TableStyle::from_theme(theme);
    style.active_header_foreground = theme.primary;
    style
})
```

`TableStyle::fallback()` provides plain ANSI colors for painting without a
theme.

## Paint-only widget

`TableWidget` paints a themed table without focus or events. It is an ordinary
Ratatui `Widget` and `StatefulWidget` (`TableState`), usable in a plain Ratatui
app with no `Ratcn` runtime. Call `.into_inner()` after theming to continue
chaining native Ratatui `Table` builders such as `highlight_symbol`:

```rust
use ratatui::widgets::Row;
use ratcn::TableWidget;

let widget = TableWidget::new(rows, widths)
    .header(Row::new(["Name", "Amount"]))
    .themed(&theme)
    .focused(table_has_focus)
    .into_inner()
    .highlight_symbol("› ");

frame.render_stateful_widget(widget, area, &mut table_state);
```

Replace `.themed(...)` with `.style(...)` to supply explicit colors.

## Keyboard and mouse

| Input | Does |
|---|---|
| `↑` `↓` &nbsp;`k` `j` &nbsp;`Ctrl+P` `Ctrl+N` | Move the row cursor one item; `↑` from the first row enters the header when `header_focus` is bound |
| `←` `→` &nbsp;`h` `l` | Move the column cursor when `column_focus` is bound |
| `Home` `End` | Move to the first / last enabled row |
| `Page Up` `Page Down` | Move a visible page |
| `Ctrl+U` `Ctrl+D` | Move half a page |
| `Enter` `Space` | Select the cursor row, or sort the active column while on the header |
| Pointer motion | Move the row cursor over rows, or highlight a sortable heading |
| Click | Select a row, or toggle sort on a sortable heading |
| Vertical wheel | Scroll rows without moving the cursor |
| Horizontal wheel | Scroll non-frozen columns when widths overflow |

Every other key bubbles, so app hotkeys keep working while a table has focus.
See [Keyboard](../concepts/keyboard) for the rules every component shares.

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## Full API

See
[`Table`](https://docs.rs/ratcn/latest/ratcn/struct.Table.html),
[`TableWidget`](https://docs.rs/ratcn/latest/ratcn/struct.TableWidget.html),
[`TableStyle`](https://docs.rs/ratcn/latest/ratcn/struct.TableStyle.html),
[`TableSort`](https://docs.rs/ratcn/latest/ratcn/struct.TableSort.html),
[`SortDirection`](https://docs.rs/ratcn/latest/ratcn/enum.SortDirection.html),
[`ListItem`](https://docs.rs/ratcn/latest/ratcn/struct.ListItem.html),
and [`ListItemState`](https://docs.rs/ratcn/latest/ratcn/struct.ListItemState.html)
on docs.rs for the full API.

## See also

For a single-column list with built-in radio/checkbox markers and
multi-selection: [List](./list). For scrolling arbitrary layouts:
[ScrollArea](./scroll-area).
