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
use ratatui::layout::Constraint;
use ratcn::{Table, TableCursor, TableSort};

struct AppState {
    cursor: Option<TableCursor<RecordId>>,
    selected: Option<RecordId>,
    column: usize,
    sort: Option<TableSort>,
    // ...
}

enum Msg {
    CursorMoved(TableCursor<RecordId>),
    RowSelected(RecordId),
    ColumnFocused(usize, usize),
    SortChanged(TableSort),
}

let table = Table::new(
    items,
    [Constraint::Fill(1), Constraint::Length(12)],
)
.header(["Description", "Amount"])
.paint_item(|s: &AppState, row| s.transaction_row(*row.value))
.item_focus(|s| s.cursor, |cursor, _top| Msg::CursorMoved(cursor))
.selection(|s| s.selected, Msg::RowSelected)
.column_focus(|s| Some(s.column), Msg::ColumnFocused)
.sort([0, 1], |s| s.sort, Msg::SortChanged)
.sort_key('s');

ctx.component("transactions", table, area);
```

Rows use the same value-keyed `ListItem` as [List](./list). Identifying each
row by its domain value keeps the cursor and selection on the same record across
sorting and filtering. Values must be unique within one `Table`; a debug build
panics on duplicates. Columns are identified by their index in the declared
widths: if you hide or reorder columns, remap the stored column and sort to
match.

`paint_item` receives app state and a `ListItemState`, returning a Ratatui
`Row` for each visible item. Explicit foreground colors and modifiers set on
`Row`, `Cell`, or `Span` values win over the row and cursor styles, except on
the selected cursor row (or cell), which is lettered in
`selected_focused_foreground` so it stays legible on the primary fill. A
background set there replaces the row fill, so check `!row.focused` when adding
custom row backgrounds such as zebra striping. Rows take `row_height` with no
margins, and cells past the widths are dropped.

## What the app stores

Each binding is a reader and a message constructor. The app stores what each
message carries:

| Binding | Message carries | Store |
|---|---|---|
| `item_focus` | a `TableCursor` (`Row(id)` or `Header`) and the top-row offset | the cursor; the offset too when `scroll` is bound |
| `selection` | the committed row's value | the selection, and `TableCursor::Row(id)` as the cursor |
| `multi_selection` | the toggled row's value | toggle it in your selection set, and set `TableCursor::Row(id)` as the cursor |
| `column_focus` | the column index and the horizontal column offset | the column; the offset too when `horizontal_scroll` is bound |
| `sort` | the requested `TableSort` | the sort; reorder the records, and move the column if you like |
| `scroll`, `horizontal_scroll` | an offset from the wheel | the offset |

Store both values of a two-value message in the same update. Adjustments the
table makes while rendering, such as keeping the cursor or column in view after
a sort or a resize, emit no message. Leave `scroll` and `horizontal_scroll`
unbound and the table scrolls itself.

A table is a Tab stop when it is enabled, has at least one column, and a key can
act there: `column_focus` is bound, the header is reachable, a `sort_key` is set
and a heading sorts, or `item_focus` is bound and some row is enabled.

With `column_focus` bound, pointer motion over another wholly visible cell in
the cursor row moves the column cursor there without scrolling. A click selects
the row or sorts a sortable heading.

## Multi-selection

Bind `.multi_selection(read, on_toggle)` to select any number of rows at once.
`read` is a predicate asked per visible row so the selection can live in a
`HashSet`, a `Vec`, or a flag on each record, and `paint_item` receives
`row.selected` to render checkboxes or badges however you like.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 420px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p table-multi</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/table-multi-demo/index.html" title="ratcn table multi-selection demo"></iframe>
  </div>
</div>

```rust
Table::new(items, widths)
    .item_focus(|s: &AppState| s.cursor, |cursor, _| Msg::CursorMoved(cursor))
    .multi_selection(|s, id| s.chosen.contains(id), Msg::RowToggled)
```

Enter, Space, or a left click emits `on_toggle` with the row's value. Pick one
mode: `.selection(...)` and `.multi_selection(...)` together panic.

## Sorting and the header

Pass the sortable column indices and a `(read, on_change)` binding to
`.sort(...)`. Clicking a sortable heading emits a `TableSort` requesting
ascending order, or descending if that column is already sorted ascending. Your
app reorders the records and passes them back in the new order; the sorted
heading displays a `▲` or `▼` mark when it fits alongside the label.

With `item_focus` bound and at least one sortable heading, the header is a place
the cursor can go: Up from the first row emits `TableCursor::Header`. On the
header the cursor sits on one heading — the column cursor if `column_focus` is
bound, otherwise the sorted column, otherwise the first sortable heading. Left
and Right move it between headed columns (with `column_focus` bound), Enter or
Space sorts it, and Down returns to the rows. Without a sortable heading, Up at
the top row stays put, as on a list.

Set `.sort_key('s')` (any `KeyChord`) to sort the cursor's column from the rows
as well — the column cursor, or without one the sorted column or the first
sortable heading. No key is set by default. It takes precedence over the
table's own keys, so pick one that does not collide with `h`/`j`/`k`/`l`; where
the column does not sort, the key bubbles to the app.

## Frozen columns and horizontal scrolling

When the columns' natural widths exceed the table's inner width, columns scroll
horizontally to keep the active column visible. Horizontal wheel input scrolls
columns without moving the active column, and bubbles to parent containers at
both ends.

Columns that fit are laid out exactly as Ratatui lays them out. They overflow
only when the rigid widths — `Length`, `Min`, and each `Fill` column's heading
(with room for a sort mark) or footer cell — plus spacing exceed the inner
width; `Percentage`, `Ratio`, and `Max` columns give way first. An overflowing
table lays every column out at a fixed width and ignores `flex`: `Max` columns
take their maximum and `Percentage` and `Ratio` columns their share. Prefer
`Length` or `Min` widths for a table that can scroll. Frozen columns should fit
in the view; freezing every column leaves nothing to scroll.

Call `.frozen_columns(n)` to pin the first `n` columns on the left. Once
scrolled right past them, a `│` separator is drawn in the spacing gap beside
the frozen columns across header, body, and footer — only when
`column_spacing` is above 0. Bind `.horizontal_scroll(read, on_change)` when your
app needs to observe or drive the column scroll offset, and store the offset
`column_focus` reports in the same update as the column.

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
.header(["Date", "Transaction", "Category", "Account", "Status", "Amount"])
.paint_item(|s: &AppState, row| s.ledger_row(*row.value))
.item_focus(|s| s.cursor, |cursor, _top| Msg::CursorMoved(cursor))
.column_focus(|s| Some(s.column), Msg::ColumnFocused)
.horizontal_scroll(|s| s.scroll_x, Msg::ScrollXChanged);

// In update: one message, both values.
Msg::ColumnFocused(column, offset) => {
    state.column = column;
    state.scroll_x = offset;
}
Msg::ScrollXChanged(offset) => state.scroll_x = offset,
```

## Layout and long tables

Use `.header(...)`, `.footer(...)`, `.block(...)`, `.flex(...)`, and
`.column_spacing(...)` to configure table layout. Headings and footer cells are
Ratatui `Line`s and keep their own alignment and spans.

Set `.row_height(n)` for multi-line body rows. Bind `.scroll(read, on_change)`
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

`TableStyle` controls the well, row, cursor, selection, header, footer, frozen
divider, and disabled colors derived from the active theme. When `column_focus` is bound, the cursor
row uses `focused_band_background` with the active cell highlighted in
`focused_row_background`, and the active column's heading uses
`active_header_foreground`. A table row has no selection marker, so a selected
row under the cursor (or its cursor cell) takes the theme's `primary` fill,
`selected_focused_background`, as a selected tab does. Off the cursor, a
selected row shows only as brighter text; mark it in `paint_item` if you need
more, as the demos do.

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
| `↑` `↓` &nbsp;`k` `j` &nbsp;`Ctrl+P` `Ctrl+N` | Move the row cursor one item; `↑` from the first row enters the header when it has a sortable heading |
| `←` `→` &nbsp;`h` `l` | Move the column cursor when `column_focus` is bound |
| `Home` `End` | Move to the first / last enabled row |
| `Page Up` `Page Down` | Move a visible page |
| `Ctrl+U` `Ctrl+D` | Move half a page |
| `Enter` `Space` | Select (or toggle in multi-selection) the cursor row; on the header, sort the header cursor's column (the column cursor if it is on a heading, else the sorted column, else the first sortable heading) |
| The key set with `sort_key` | Sort the cursor's column, from the rows or the header |
| Pointer motion | Move the row cursor over rows, the column cursor to a whole cell within the cursor row, or highlight a sortable heading |
| Click | Select (or toggle) a row, or toggle sort on a sortable heading |
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
[`TableCursor`](https://docs.rs/ratcn/latest/ratcn/enum.TableCursor.html),
[`TableSort`](https://docs.rs/ratcn/latest/ratcn/struct.TableSort.html),
[`SortDirection`](https://docs.rs/ratcn/latest/ratcn/enum.SortDirection.html),
[`ListItem`](https://docs.rs/ratcn/latest/ratcn/struct.ListItem.html),
and [`ListItemState`](https://docs.rs/ratcn/latest/ratcn/struct.ListItemState.html)
on docs.rs for the full API.

## See also

For a single-column list with built-in radio/checkbox markers and
multi-selection: [List](./list). For scrolling arbitrary layouts:
[ScrollArea](./scroll-area).
