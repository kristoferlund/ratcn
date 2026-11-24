---
description: "A themed Ratatui table with keyed row selection, fixed headers and footers, keyboard navigation, and sortable columns."
---

# Table

Use `Table` when people need to browse, select, and sort records. The table
handles input; your app decides how to order the data. A currency column can
sort by cents while displaying formatted amounts. Keep the row's ID as its
value, so sorting leaves the cursor and selection on the same record.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 420px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span><span class="ratcn-dot"></span><span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p table</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/table-demo/index.html" title="Sortable transactions table"></iframe>
  </div>
</div>

```rust
let table = Table::new(items, [Constraint::Fill(1), Constraint::Length(12)])
    .header(["Description", "Amount"])
    .paint_item(|s: &State, row| s.transaction_row(*row.value))
    .item_focus(|s| s.cursor, Msg::Move)
    .header_focus(|s| s.on_header, || Msg::Header)
    .selection(|s| s.selected, Msg::Select)
    .column_focus(|s| Some(s.column), Msg::Column)
    .sort([0, 1], |s| s.sort, Msg::Sort);
ctx.component("transactions", table, area);
```

The table works like a [List](./list) with columns. There is one cursor; arrow,
Home, End and page keys move it, and so does the pointer, which leaves it where
it was last. Enter, Space or a left click selects; the wheel scrolls without
changing the cursor. Left/Right choose a column.

Clicking a sortable heading requests ascending order, then descending. With
`header_focus` bound, Up from the first row moves the cursor onto the header,
where Enter or Space sorts the active column and Down returns to the rows.
Store `on_header` in your state (set it on the header message and clear it when
moving or selecting a row), store the `TableSort` request, and pass items in
the new order. Other letter keys bubble to your app, and Tab leaves the table.
For larger collections, `from_shared` reuses an `Arc<[ListItem<T>]>`;
`paint_item` only formats visible rows.

Use `.block(...)`, `.flex(...)`, `.column_spacing(...)`, `.footer(...)` and native
`Row`/`Cell` styles for layout. Headings and footer cells are `Line`s and keep
their own alignment; a sorted heading gets a `▲`/`▼` mark when it fits alongside
the label. Set `.row_height(n)` for multi-line body rows. Full rows are painted
so clicks and page movement agree.

## Styling

Colors come from the theme, the same way a list's do, so a table next to a list
looks like part of the same set. With a column cursor, the cursor row takes a
subtler fill and only the active cell keeps the full cursor fill. A hovered
heading also takes the subtler fill, so pointing never looks like the
keyboard's position. `.style(...)` overrides the colors:

```rust
use ratcn::TableStyle;

Table::new(items, widths).style(|theme| {
    let mut style = TableStyle::from_theme(theme);
    style.active_header_foreground = theme.primary;
    style
})
```

## Paint-only widget

Without the runtime, `TableWidget` accepts native rows directly, including
variable heights and margins. It works with `frame.render_widget` or
`frame.render_stateful_widget` and Ratatui's own `TableState`. Call `into_inner()`
after theming to keep using Ratatui builders such as `highlight_symbol`:

```rust
let widget = TableWidget::new(rows, widths)
    .header(Row::new(["Name", "Amount"]))
    .themed(&theme)
    .focused(true)
    .into_inner()
    .highlight_symbol("› ");
frame.render_stateful_widget(widget, area, &mut table_state);
```

Copy the component with `cargo ratcn add table`. The [ledger demo source](https://github.com/kristoferlund/ratcn/blob/main/demos/table/src/lib.rs)
shows the complete sorting and selection bindings.

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
| Wheel | Scroll the view, leaving the cursor |

Every other key bubbles, so single-letter app hotkeys keep working while a
table has focus. See [Keyboard](../concepts/keyboard) and
[Mouse input](../concepts/mouse) for the rules every component shares.

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
