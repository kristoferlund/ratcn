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
filtering the list keeps the same item selected. Those values must be unique
within one list, or focus, selection, and clicks are ambiguous; a debug build
panics on duplicates as the list declares, and a release build takes the items
on trust — the scan is quadratic and every frame would repeat it. `item_focus`
is the cursor and `selection` is the committed choice — separate, so a user can
browse without changing anything.

Arrow keys move the cursor one item at a time; Home and End jump to the first
and last enabled item, and PageUp and PageDown move it a visible page at a time.
`j`/`k` and Ctrl+N/Ctrl+P step as well, and Ctrl+D/Ctrl+U move half a page — see
[Keyboard](../concepts/keyboard).

Enter or Space commits the cursor. Every other key is ignored and bubbles, so a
single-letter app hotkey keeps working while a list has focus.

## Multi-selection

Any number of items at once, with checkbox markers. Instead of a selected value
you give a predicate: `List` asks "is this one selected?" per row it paints, so
the selection can live in a `HashSet`, a `Vec`, or a flag on each record.

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

`on_toggle` reports the item the user flipped; your update function adds or
removes it. Enter or Space toggles the cursor item. Pick one mode —
`.selection(...)` and `.multi_selection(...)` together will panic.

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
default markers are `■`/`□` and `●`/`○`. To change only the markers — ASCII
`[x]`/`[ ]`, say — use `.selected_marker(...)` and `.unselected_marker(...)`
instead of repainting the whole row:

```rust
List::new(todos)
    .multi_selection(|s: &AppState, item| s.done.contains(item), Msg::Toggled)
    .selected_marker("[x]")
    .unselected_marker("[ ]")
```

The row's state colors are painted underneath what `paint_item`
returns, so unstyled text picks up the focused, selected, or disabled colors,
and any color you set explicitly on a `Text`, `Line`, or `Span` is kept.

`.focus_symbol("> ")` adds a marker in front of the cursor row without replacing
the row. It is painted only while the list is focused or hovered, so a list at
rest shows no cursor.

## Disabled

`ListItem::disabled(true)` dims one row and skips it for keys and clicks.
`.disabled(true)` on the list disables the whole thing, and Tab skips it.

```rust
ListItem::new(Folder::Settings, "Settings").disabled(!state.is_admin)
```

## Scrolling

The list scrolls itself to keep the cursor visible, and the wheel scrolls it
whether or not anything is bound. Bind `.scroll(...)` only when something
outside needs the offset — a scrollbar alongside, say. The offset is an item
index even when items occupy multiple terminal rows.

```rust
List::new(items).scroll(|s: &AppState| s.scroll, Msg::ScrollChanged)
```

To scroll something that is not a list — a form, a pane, a tile grid — see
[ScrollArea](./scroll-area).

`item_focus` calls its message constructor with both the target item and the
resulting top-item offset. A bound-scroll app must store both in one update:

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

This keeps repeated navigation events correct even when several arrive before
redraw. If scroll is unbound, ignore the second callback argument.

The mouse wheel is the one input that does not follow the cursor. It scrolls
the view and leaves the cursor where it is, so the cursor can scroll out of
sight — the same behavior a scrollable list has elsewhere. The wheeled view is
held only while the list stays as it was: the item under the cursor is still
that item, still on that row, in a list of the same length. Move the cursor,
replace that item, reorder, filter, insert, or remove, and the hold ends and
the cursor is scrolled back into view — a held row number means nothing once
the rows have moved. The list always handles the wheel, including at the top and
bottom of the range, so it never scrolls an enclosing pane.

Pointer motion moves the cursor whether or not the list has focus, from the
motion that enters the list onward. The cursor is only *painted* on a focused
or hovered list, and the runtime keeps hover itself, so the highlight appears
on the same frame the pointer arrives.

## Styling

Colors come from the theme. `.style(...)` overrides them, and the closure gets
the active theme each render so a derived style follows theme switches. Focus
separates the field backdrop subtly from the background; hover separates it a
little further, so the pointer remains visible when the list already has
keyboard focus. Which way that is comes from the theme: a dark theme's well
lightens, a light theme's darkens.

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
start with `first_item`. Every other index — `focused_item`, `selected_items`,
`disabled_items` — counts from the start of the list, so scrolling changes only
that number and the rows. The widget holds no scroll position and never adjusts
one, so your app stays the only scroll policy. Offscreen rows are free: you
build `Text`s for the rows you hand over, and the widget allocates nothing per
item it does not paint.

The widget is area-driven — it fills the area you give it and has nothing to
measure — and it paints each item at whatever height its `Text` is. Keeping
those heights uniform is yours to do here, because the arithmetic that maps a
screen row back to an item counts items rather than lines; the `List` component
does it for you.

The two row inputs use different encodings: `selected_items` is a list of
selected indices, since selection is sparse, while `disabled_items` is one flag
per item. Both are read at the item's own index, and only for the rows
the widget paints — so a windowed caller can name just the selected rows inside
its window, but `disabled_items` is a positional mask and has to be padded up to
the window: entry *n* describes item *n*, and entries past the end of the slice
read as enabled.

`.disabled(true)` dims the whole widget, as `.disabled_items(...)` does for
single rows. Replace `.themed(...)` with `.style(...)` to supply exact widget
colors instead of deriving them from a theme.

## Full API

Every method, with binding requirements and edge-case detail:
[`List`](https://docs.rs/ratcn/latest/ratcn/struct.List.html),
[`ListItem`](https://docs.rs/ratcn/latest/ratcn/struct.ListItem.html),
[`ListItemState`](https://docs.rs/ratcn/latest/ratcn/struct.ListItemState.html),
[`ListWidget`](https://docs.rs/ratcn/latest/ratcn/struct.ListWidget.html),
[`ListStyle`](https://docs.rs/ratcn/latest/ratcn/struct.ListStyle.html).

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## See also

The horizontal counterpart, with the same cursor and selection split:
[Tabs](./tabs).
