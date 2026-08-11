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

ctx.render_component("folders", list, area);
```

Items are identified by your own values, not by row index, so sorting or
filtering the list keeps the same item selected. Those values must be unique
within one list. `item_focus` is the cursor and
`selection` is the committed choice — separate, so a user can browse without
changing anything.

Arrow keys move the cursor one item at a time; Home and End jump to the first
and last enabled item, and PageUp and PageDown move it a visible page at a time.
`j`/`k` and Ctrl+N/Ctrl+P step as well, and Ctrl+D/Ctrl+U move half a page — see
[Keyboard](../concepts/keyboard).

Enter or Space commits the cursor. Every other key is ignored and bubbles, so a
single-letter app hotkey keeps working while a list has focus.

## Multi-selection

Any number of items at once, with checkbox markers. Instead of a selected value
you give a predicate: `List` asks "is this one selected?" per item, so the
selection can live in a `HashSet`, a `Vec`, or a flag on each record.

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

`render_item` replaces the default marker-and-label line with anything you can
draw. For rows taller than one line, return a `Text` and set `.row_height(...)`
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
    .render_item(move |state: &AppState, row| {
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
default markers are `■`/`□` and `●`/`○`; this demo draws ASCII `[x]`/`[ ]`
instead. The row's state colors are painted underneath what `render_item`
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
sight — the same behavior a scrollable list has elsewhere. Moving the cursor
again scrolls it back into view. The list always handles the wheel, including
at the top and bottom of the range, so it never scrolls an enclosing pane.

Pointer motion moves the cursor whether or not the list has focus. The cursor
is only *painted* on a focused or hovered list, so binding `item_focus`
without binding `Ratcn::hover` lets the pointer move a cursor you cannot see
yet; bind hover, or accept that the cursor is where the pointer last was.

## Styling

Colors come from the theme. `.style(...)` overrides them, and the closure gets
the active theme each render so a derived style follows theme switches. Focus
lightens the field backdrop subtly; hover lightens it a little further, so the
pointer remains visible when the list already has keyboard focus.

```rust
use ratcn::ListStyle;

List::new(items).style(|theme| {
    let mut style = ListStyle::from_theme(theme);
    style.focused_row_background = theme.accent;
    style
})
```

## Paint-only widget

`ListWidget` draws a list without focus or events. It is an ordinary Ratatui
widget, so it works in a plain Ratatui app with no `Ratcn` runtime. Rows are
pre-rendered `Text`s and everything is addressed by index. Explicit colors in
those `Text`s are preserved:

```rust
use ratatui::text::Text;
use ratcn::ListWidget;

let items = vec![Text::from("Inbox"), Text::from("Archive")];

frame.render_widget(
    ListWidget::new(&items)
        .scroll_offset(scroll_offset)
        .focused_row(Some(0))
        .selected_rows(&[1])
        .disabled_rows(&[false, true])
        .focused(list_has_focus)
        .hovered(pointer_is_over_list)
        .focus_symbol("> ")
        .themed(&theme),
    area,
);
```

Scrolling is an input: pass the index of the topmost visible item with
`scroll_offset` each frame. The widget paints exactly what it is told and never
adjusts the value, so your app stays the only scroll policy.

The widget is area-driven — it fills the area you give it and has nothing to
measure — and it paints each item at whatever height its `Text` is. Keeping
those heights uniform is yours to do here, because `scroll_offset` counts items
rather than lines; the `List` component does it for you.

The two row inputs use different encodings on purpose: `selected_rows` is a
list of selected indices, since selection is sparse, while `disabled_rows` is
one flag per row, parallel to the items.

`.disabled(true)` dims the whole widget, as `.disabled_rows(...)` does for
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
