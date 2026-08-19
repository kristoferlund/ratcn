---
description: "A select control for Ratatui apps: choose one option from a panel that opens in a popup layer, so it overlays surrounding content and works inside dialogs."
---

# Select

A control for choosing one option from a panel that opens on demand. The panel
opens in a popup layer, so it overlays surrounding content and also works
inside dialogs.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 340px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p select</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/select-demo/index.html" title="ratcn select demo"></iframe>
  </div>
</div>

```rust
use ratcn::{ListItem, Select};

let select = Select::new([
    ListItem::new(Fruit::Mango, "Mango"),
    ListItem::new(Fruit::Papaya, "Papaya"),
])
.placeholder("Pick a fruit...")
.open(|s: &AppState| s.open, Msg::OpenChanged)
.item_focus(|s: &AppState| s.cursor, Msg::Focused)
.selection(|s: &AppState| s.selected, Msg::Selected);

ctx.component("fruit", select, area);
```

Options use the same value-keyed `ListItem` as `List`, so reordering them does
not change the selected value. Those values must be unique within one Select; a
debug build panics on duplicates as the Select declares, and a release build
takes them on trust, exactly as in [List](./list). In Select terminology, an item
supplies value identity, an option is one choice, and a row is the terminal space
used to draw that option.

## State

The app owns three values: whether the panel is open, the option cursor, and the
committed selection. A selection update should store the choice, align the
cursor, and close the panel in one message:

```rust
Msg::Selected(fruit) => {
    state.selected = Some(fruit);
    state.cursor = Some(fruit);
    state.open = false;
}
```

Keyboard operation requires `open`, `item_focus`, and `selection`. Partial
binding combinations remain available for paint-only or pointer-only uses, but
do not make the Select a keyboard focus stop or consume keyboard input.

## Interaction

Enter or Space opens a focused Select, and so does any key that would step the
cursor — Up, Down, `k`, `j`, Ctrl+P, or Ctrl+N. While open, the full
[navigation key map](../concepts/keyboard) moves the cursor; Enter or Space
selects; Esc closes. The first Tab closes the panel, and the next Tab moves
focus. Every closing gesture arrives through the `open` binding as
`on_open_change(false)`, so one message handles them all.

Pointer motion moves the cursor, and a left click selects the option under the
pointer, including the first option where it overlays the trigger row. Pressing
outside dismisses the panel while leaving the underlying control clickable.
The mouse wheel is the one input that does not follow the cursor. It scrolls
the panel and leaves the cursor where it is, so the cursor can scroll out of
sight — the same wheel behavior as [List](./list), and held under the same rule:
only while the option under the cursor is still that option, still on that row,
in an option list of the same length. Moving the cursor, or changing the options
under it, scrolls the cursor back into view.
Modified keys other than the closing Tab chords bubble to app hotkeys,
as does a typed character matching no option. Paste events also bubble because
Select has no text-editing behavior.

The panel shows at most eight options by default and scrolls to keep the cursor
visible, except while the wheel is holding the view elsewhere.
`.max_visible_options(...)` changes that limit. Its top border starts
one row above the trigger, so the first option covers the trigger row. The panel
stays fixed while the cursor moves, shifting only when needed to remain inside
the frame.

## Disabled

`ListItem::disabled(true)` dims one option and skips it for keys and clicks.
`.disabled(true)` disables the whole Select and removes it from Tab traversal.

```rust
ListItem::new(Fruit::Durian, "Durian").disabled(!state.durian_available)
```

## Custom rows

`.render_item(...)` draws each option yourself — columns, secondary text,
per-option icons — from the same `ListItemState` row description `List` uses.
The row's state colors are painted underneath what you return, so unstyled text
picks them up, and any color you set explicitly on a `Text`, `Line`, or `Span`
is kept.
Return more than one line and set `.row_height(...)` to match, so every option
is the same height and clicks land on the right one:

```rust
Select::new(items)
    .render_item(|state: &AppState, row| Text::from(vec![
        Line::from(row.label.to_string()),
        Line::from(format!("  {}", state.subtitle_for(row.value))),
    ]))
    .row_height(2)
```

## Styling

The trigger's default, focus, and hover backdrops match List. Hover sits one
step further from the background than focus, so it remains visible when the
trigger already has keyboard focus. `SelectStyle` controls trigger, panel, cursor, selection, and disabled
colors. `SelectWidget` is the paint-only ratatui widget for using the same
appearance without the runtime.

Override one Select with `.style(...)`. The closure receives the active theme
each render, so a derived style follows theme switches:

```rust
use ratcn::SelectStyle;

Select::new(items).style(|theme| {
    let mut style = SelectStyle::from_theme(theme);
    style.selected_marker = theme.accent;
    style
})
```

`SelectStyle::fallback()` is the no-theme starting point: plain ANSI colors that
render on any terminal.

## Paint-only widget

`SelectWidget` draws a Select without focus or events. It is an ordinary
Ratatui widget, so it works in a plain Ratatui app with no `Ratcn` runtime.
Unlike the interactive component's overlaid popup, its open panel paints below
the trigger inside the area passed to the widget. Options and state are
addressed by index:

```rust
use ratcn::SelectWidget;

let options = ["Mango", "Papaya", "Lychee", "Durian"];

frame.render_widget(
    SelectWidget::new(selected_label)
        .placeholder("Pick a fruit...")
        .open(&options)
        .focused_option(Some(cursor_index))
        .selected_option(selected_index)
        .disabled_options(&[false, false, true, false])
        .scroll_offset(scroll_offset)
        .focused(select_has_focus)
        .hovered(pointer_is_over_select)
        .disabled(select_is_disabled)
        .themed(&theme),
    area,
);
```

Call `.height(...)` and `.visible_options(...)` on the built widget when the
surrounding layout needs to reserve exactly the rows it will paint — they read
openness, disabled state, option count, and row height from the instance.
`.visible_option_rows(...)` accepts pre-rendered screen rows for the options
actually painted — the ones from `scroll_offset` on, in paint order — while
`.open(...)` still takes every option, because the panel's height is measured
from their count. Pair it with `.row_height(...)` for multi-line rows. Together
they are the paint-only counterpart of the component's `.render_item(...)`.
Replace `.themed(...)` with `.style(...)` to supply exact widget colors.

## Full API

Every method, with binding requirements and edge-case detail:
[`Select`](https://docs.rs/ratcn/latest/ratcn/struct.Select.html),
[`SelectWidget`](https://docs.rs/ratcn/latest/ratcn/struct.SelectWidget.html),
[`SelectStyle`](https://docs.rs/ratcn/latest/ratcn/struct.SelectStyle.html),
[`ListItem`](https://docs.rs/ratcn/latest/ratcn/struct.ListItem.html),
[`ListItemState`](https://docs.rs/ratcn/latest/ratcn/struct.ListItemState.html).

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## See also

Use [List](./list) when several options should remain visible instead of opening
from a trigger.
