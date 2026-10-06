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

Options use the same value-keyed `ListItem` as [List](./list), so reordering
them does not change the selected value. The values must be unique within one
`Select`; a debug build panics on duplicates.

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

Keyboard operation needs all three bindings: `open`, `item_focus`, and
`selection`. With only some of them bound, the `Select` still paints and can
work with the pointer, but it is not a focus stop and takes no keys.

## The panel

The panel opens with its top border one row above the trigger, so the first
option covers the trigger row. It shows at most eight options and scrolls to
keep the cursor visible; `.max_visible_items(...)` changes that limit. The
panel stays put while the cursor moves, shifting only when it must to stay
inside the frame.

Esc, Tab, a click on the trigger, and a press outside the panel all close it
through the `open` binding as `on_open_change(false)`, so one message handles
every way of dismissing it. A press outside leaves the control underneath clickable.

## Disabled

`ListItem::disabled(true)` dims one option and skips it for keys and clicks.
`.disabled(true)` disables the whole Select and removes it from Tab traversal.

```rust
ListItem::new(Fruit::Durian, "Durian").disabled(!state.durian_available)
```

## Custom rows

`.paint_item(...)` paints each option yourself (columns, secondary text,
per-option icons) from the same `ListItemState` row description `List` uses.
The row's state colors are painted underneath what you return, so unstyled text
picks them up, and any color you set explicitly on a `Text`, `Line`, or `Span`
is kept. For more than one line, set `.row_height(...)` to match, so every
option is the same height and clicks land on the right one:

```rust
Select::new(items)
    .paint_item(|state: &AppState, row| Text::from(vec![
        Line::from(row.label.to_string()),
        Line::from(format!("  {}", state.subtitle_for(row.value))),
    ]))
    .row_height(2)
```

## Styling

The trigger's default, focus, and hover backdrops match
[List's](./list#styling). `SelectStyle` controls the trigger, panel, cursor,
selection, and disabled colors.

Override one `Select` with `.style(...)`. The closure receives the active theme
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

`SelectWidget` paints a select without focus or events. It is an ordinary
Ratatui widget, so it works in a plain Ratatui app with no `Ratcn` runtime.
Its open panel paints below the trigger, inside the area you give the widget,
rather than in a popup layer. Options and state are addressed by index:

```rust
use ratcn::SelectWidget;

let options = ["Mango", "Papaya", "Lychee", "Durian"];

frame.render_widget(
    SelectWidget::new(selected_label)
        .placeholder("Pick a fruit...")
        .open(true)
        .options(&options)
        .focused_item(Some(cursor_index))
        .selected_item(selected_index)
        .disabled_items(&[false, false, true, false])
        .first_item(first_item)
        .focused(select_has_focus)
        .hovered(pointer_is_over_select)
        .disabled(select_is_disabled)
        .themed(&theme),
    area,
);
```

For custom rows, `.visible_item_rows(...)` takes the rows you build for the
options actually painted (from `first_item` on), and `.row_height(...)` sets
their height; `.options(...)` still takes every option, since the panel is
sized from their count. Replace `.themed(...)` with `.style(...)` to supply
exact colors.

## Keyboard and mouse

| Input | Does |
|---|---|
| `Enter` `Space` | Open the panel; select the cursor option while open |
| `↑` `↓` &nbsp;`k` `j` &nbsp;`Ctrl+P` `Ctrl+N` | Open the panel; move the cursor while open |
| `Home` `End` `Page Up` `Page Down` &nbsp;`Ctrl+U` `Ctrl+D` | Move the cursor while open |
| `Esc` | Close the panel |
| `Tab` `Shift+Tab` | Close the panel; the next press moves focus |
| Click the trigger | Open or close the panel |
| Pointer motion | Move the cursor |
| Click | Select the option under the pointer |
| Wheel | Scroll the panel, leaving the cursor |

As in [List](./list), the wheel can scroll the cursor out of sight, and moving
the cursor or changing the options brings it back into view. Other letters,
other modified keys, and pastes bubble to your app: `Select` has no typeahead.
See [Keyboard](../concepts/keyboard) for the rules every component shares.

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## Full API

See
[`Select`](https://docs.rs/ratcn/latest/ratcn/struct.Select.html),
[`SelectWidget`](https://docs.rs/ratcn/latest/ratcn/struct.SelectWidget.html),
[`SelectStyle`](https://docs.rs/ratcn/latest/ratcn/struct.SelectStyle.html),
[`ListItem`](https://docs.rs/ratcn/latest/ratcn/struct.ListItem.html),
and [`ListItemState`](https://docs.rs/ratcn/latest/ratcn/struct.ListItemState.html)
on docs.rs for the full API.

## See also

Use [List](./list) when several options should remain visible instead of opening
from a trigger.
