---
description: "A scrollable, focusable list for Ratatui apps. Arrow keys move the cursor, Enter or a click selects, and the wheel scrolls the view. Single and multi-selection."
---

# List

A scrollable, focusable list. Arrow keys move a cursor through the items, Enter,
Space, or a click selects one, and long lists scroll to follow the cursor.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 420px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p list</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/list-demo/index.html" title="ratcn list demo"></iframe>
  </div>
</div>

```rust
use ratcn::{List, ListItem};

let list = List::new([
    ListItem::new(Folder::Inbox, "Inbox"),
    ListItem::new(Folder::Archive, "Archive"),
    ListItem::new(Folder::Settings, "Settings"),
])
.item_focus(
    |s: &AppState| s.focused_folder,
    |folder, offset| Msg::FolderFocused { folder, offset },
)
.selection(|s: &AppState| s.selected_folder, Msg::FolderSelected);

ctx.component("folders", list, area);
```

Items are identified by your own values, not by row index, so sorting or
filtering the list keeps the same item selected. The values must be unique
within one list, or focus, selection, and clicks are ambiguous; a debug build
panics on duplicates.

`item_focus` is the cursor and `selection` is the committed choice. They are
separate so a user can browse without changing anything.

## Multi-selection

Any number of items at once, with checkbox markers. Instead of a selected value
you give a predicate: `List` asks "is this one selected?" for each row it
paints, so the selection can live in a `HashSet`, a `Vec`, or a flag on each
record.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 400px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p list-multi</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/list-multi-demo/index.html" title="ratcn multi-select list demo"></iframe>
  </div>
</div>

```rust
List::new(items)
    .item_focus(
        |s: &AppState| s.focused_topic,
        |topic, offset| Msg::TopicFocusChanged { topic, offset },
    )
    .multi_selection(
        |s: &AppState, topic| s.subscribed.contains(topic),
        Msg::TopicToggled,
    )
```

The message reports the item the user flipped, and your update function adds or
removes it. Enter or Space toggles the cursor item. Pick one mode:
`.selection(...)` and `.multi_selection(...)` together panic.

## Custom rows

`paint_item` replaces the default marker-and-label line with anything you can
paint. For rows taller than one line, return a `Text` and set `.row_height(...)`
to match.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 380px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p list-people</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/list-people-demo/index.html" title="ratcn custom-row list demo"></iframe>
  </div>
</div>

```rust
List::new(people)
    .multi_selection(|s: &AppState, name| s.invited.contains(name), Msg::Toggled)
    .row_height(2)
    .paint_item(move |state: &AppState, row| {
        let marker = if row.selected { "[x]" } else { "[ ]" };
        Text::from(vec![
            Line::from(format!("{marker}  {}", row.label)),
            Line::from(Span::styled(
                format!("     {}", state.title_for(row.label)),
                Style::default().add_modifier(Modifier::DIM),
            )),
        ])
    })
```

Every item is the same height, which keeps clicking and paging exact. The
default markers are `●`/`○` for a single selection and `■`/`□` for a
multi-selection. To change only the markers, to ASCII `[x]`/`[ ]` say, use
`.selected_marker(...)` and `.unselected_marker(...)` instead of repainting the
whole row:

```rust
List::new(todos)
    .multi_selection(|s: &AppState, item| s.done.contains(item), Msg::Toggled)
    .selected_marker("[x]")
    .unselected_marker("[ ]")
```

The row's state colors are painted underneath what `paint_item` returns, so unstyled text picks up the focused, selected, or disabled colors,
and any color you set explicitly on a `Text`, `Line`, or `Span` is kept.

`.focus_symbol("> ")` adds a marker in front of the cursor row without
replacing the row. Like the cursor highlight, it shows only while the list is
focused or hovered.

## Disabled

`ListItem::disabled(true)` dims one row and skips it for keys and clicks.
`.disabled(true)` on the list disables the whole thing, and Tab skips it.

```rust
ListItem::new(Folder::Settings, "Settings").disabled(!state.is_admin)
```

## Scrolling

The list scrolls itself to keep the cursor visible, and the wheel scrolls it
whether or not anything is bound. Bind `.scroll(...)` only when something
outside needs the offset, such as a scrollbar alongside. The offset is an item
index even when items occupy several terminal rows.

```rust
List::new(items).scroll(|s: &AppState| s.scroll, Msg::ScrollChanged)
```

`item_focus` hands its message both the item and the resulting top-item
offset. With scroll bound, store both in one update, so repeated navigation
events stay correct even when several arrive before a redraw:

```rust
enum Msg {
    ItemFocused { item: ItemId, offset: usize },
    ScrollChanged(usize),
}

Msg::ItemFocused { item, offset } => {
    state.focused_item = Some(item);
    state.scroll = offset;
}
```

If scroll is unbound, ignore the offset.

The wheel scrolls the view and leaves the cursor where it is, so the cursor can
scroll out of sight. Moving the cursor, or changing the items around it,
scrolls the cursor back into view. The list always handles the wheel, even at
the ends of its range, so it never scrolls an enclosing pane.

To scroll something that is not a list (a form, a pane, a tile grid), see
[ScrollArea](./scroll-area).

## Styling

Colors come from the theme. Focus separates the list's backdrop subtly from the
background, and hover separates it a little further, so the pointer stays
visible when the list already has keyboard focus. Which way that goes comes
from the theme: a dark theme's well lightens, a light theme's darkens.

`.style(...)` overrides the colors. The closure gets the active theme each
render, so a derived style follows theme switches:

```rust
use ratcn::ListStyle;

List::new(items).style(|theme| {
    let mut style = ListStyle::from_theme(theme);
    style.focused_row_background = theme.accent;
    style
})
```

## Paint-only widget

`ListWidget` paints a list without focus or events. It is an ordinary Ratatui
widget, so it works in a plain Ratatui app with no `Ratcn` runtime. Rows are
`Text`s you build yourself and everything else is addressed by index. Explicit
colors in those `Text`s are preserved:

```rust
use ratatui::text::Text;
use ratcn::ListWidget;

let rows = vec![Text::from("Inbox"), Text::from("Archive")];

frame.render_widget(
    ListWidget::new(&rows[scroll_offset.min(rows.len())..])
        .first_item(scroll_offset)
        .focused_item(Some(0))
        .selected_items(&[1])
        .disabled_items(&[false, true])
        .focused(list_has_focus)
        .hovered(pointer_is_over_list)
        .focus_symbol("> ")
        .themed(&theme),
    area,
);
```

Scrolling is yours: hand over the rows that are on screen and say where they
start with `first_item`. Every other index counts from the start of the list,
so scrolling changes only that number and the rows. You build `Text`s only for
the rows you hand over. Keep their heights uniform, as the `List` component
does for you.

`selected_items` lists the selected indices, while `disabled_items` is one flag
per item. A windowed caller pads `disabled_items` up to its window; entries
past the end read as enabled.

`.focused(...)` picks the focus backdrop and shows the cursor row and focus
symbol. `.disabled(true)` dims the whole widget. Replace `.themed(...)` with
`.style(...)` to supply exact colors.

## Keyboard and mouse

| Input | Does |
|---|---|
| `↑` `↓` &nbsp;`k` `j` &nbsp;`Ctrl+P` `Ctrl+N` | Move the cursor one item |
| `Home` `End` | Move to the first / last enabled item |
| `Page Up` `Page Down` | Move a visible page |
| `Ctrl+U` `Ctrl+D` | Move half a page |
| `Enter` `Space` | Select, or toggle in a multi-selection |
| Pointer motion | Move the cursor |
| Click | Select the item, or toggle it in a multi-selection |
| Wheel | Scroll the view, leaving the cursor |

Every other key bubbles, so a single-letter app hotkey keeps working while a
list has focus. Pointer motion moves the cursor whether or not the list has
focus, but the cursor is painted only on a focused or hovered list. See
[Keyboard](../concepts/keyboard) for the rules every component shares.

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## Full API

Every method, with binding requirements and edge-case detail:
[`List`](https://docs.rs/ratcn/latest/ratcn/struct.List.html),
[`ListItem`](https://docs.rs/ratcn/latest/ratcn/struct.ListItem.html),
[`ListItemState`](https://docs.rs/ratcn/latest/ratcn/struct.ListItemState.html),
[`ListWidget`](https://docs.rs/ratcn/latest/ratcn/struct.ListWidget.html),
[`ListStyle`](https://docs.rs/ratcn/latest/ratcn/struct.ListStyle.html).

## See also

The horizontal counterpart, with the same cursor and selection split:
[Tabs](./tabs).
