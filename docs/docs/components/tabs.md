---
description: "A row of tabs for Ratatui apps, one selected at a time. Left and Right move between them; Enter, Space, or a click switches. Manual and automatic activation."
---

# Tabs

A row of tabs, one selected at a time. Left and Right move between them; Enter,
Space, or a click switches.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 320px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p tabs-basic</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/tabs-basic-demo/index.html" title="ratcn small tabs demo"></iframe>
  </div>
</div>

```rust
use ratcn::{Tab, Tabs};

let tabs = Tabs::new([
    Tab::new(Screen::Overview, "Overview"),
    Tab::new(Screen::Analytics, "Analytics"),
    Tab::new(Screen::Reports, "Reports"),
])
.item_focus(|s: &AppState| Some(s.focused), Msg::ScreenFocusChanged)
.selection(|s: &AppState| Some(s.selected), Msg::ScreenSelected);

ctx.component("tabs", tabs, tabs_area);

match state.selected {
    Screen::Overview => declare_overview(ctx, content_area),
    Screen::Analytics => declare_analytics(ctx, content_area),
    Screen::Reports => declare_reports(ctx, content_area),
}
```

The row paints only the tabs — what appears below is yours, matched on the
selected value in the same frame. Tabs are identified by your own values rather
than by position, so filtering or reordering the row keeps the same tab selected.
The readers return `Option`s: `None` means no cursor or no active tab yet, and
the first arrow key then lands on the first enabled tab. For a quick row of
strings, `Tabs::new(["One", "Two"])` works too — each label doubles as its
value, like `List`'s string sugar.

Left and Right move along the strip, Home and End jump to the first and last
enabled tab, and Enter or Space commits. A tab strip is horizontal, so its `vi`
keys are `h`/`l` rather than `j`/`k`; Ctrl+P/Ctrl+N step either way. See
[Keyboard](../concepts/keyboard).

Pointer hover is paint-only here, unlike List and Select, where it moves the
cursor: under automatic activation the cursor *is* the selection, so hovering
would switch the panel's content on the way past.

## Activation

Whether arrow keys switch tabs or only move a cursor. It matters when switching
is expensive or destructive: manual lets the user look before committing.

Manual is the default. `TabsActivation::Automatic` drops the separate cursor and
selects immediately, so it needs only `.selection(...)`.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 320px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p tabs-automatic</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/tabs-automatic-demo/index.html" title="ratcn direct-selection tabs demo"></iframe>
  </div>
</div>

```rust
use ratcn::TabsActivation;

Tabs::new(tabs)
    .selection(|s: &AppState| Some(s.selected), Msg::ScreenSelected)
    .activation(TabsActivation::Automatic)
```

## Large

`TabsSize::Large` gives a taller tab shape.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 340px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p tabs-large</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/tabs-large-demo/index.html" title="ratcn large tabs demo"></iframe>
  </div>
</div>

```rust
use ratcn::TabsSize;

Tabs::new(tabs).size(TabsSize::Large)
```

Use `.height()` and `.width()` for layout constraints rather than hard-coding
numbers. A large row given fewer than three rows paints nothing and is excluded
from keyboard and pointer interaction. If the supplied area is taller than the
tabs, only the first one or three rows participate; blank excess rows are not
focus or click targets.

## Disabled

Disabled tabs paint dimmed, ignore clicks, and are skipped by arrow keys. A
selected tab that becomes disabled stays visibly selected, so the panel on screen
always has an identifiable tab.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 320px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p tabs-disabled</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/tabs-disabled-demo/index.html" title="ratcn disabled tabs demo"></iframe>
  </div>
</div>

```rust
Tab::new(Screen::Reports, "Reports").disabled(!state.reports_enabled)
```

Current state is in scope while declaring, so pass the flag directly.

`.disabled(true)` on the row disables the whole control — every tab greys out,
events are ignored, and Tab traversal skips it — the same whole-control switch
`Button` and `List` have.

## Overflow

When the row is narrower than its tabs, the widget keeps the focused tab, or the
selected tab when there is no separate cursor, visible and marks hidden sides
with `‹` and `›`. Click a marker to move toward the nearest hidden enabled tab.
Even a one-cell-wide row remains interactive; the selected or focused tab clips
to the available width. Hover highlights the tab under the pointer without
switching content.

## Styling

The selected tab uses the theme's default-button colors and the rest use
secondary-button colors, so a tab row stays consistent with the buttons around
it. Override one row with `.style(...)`:

```rust
use ratcn::TabsStyle;

Tabs::new(tabs).style(|theme| {
    let mut style = TabsStyle::from_theme(theme);
    style.selected_background = theme.accent;
    style
})
```

`TabsStyle::fallback()` is the no-theme starting point: plain ANSI colors that
render on any terminal.

## Paint-only widget

`TabsWidget` paints a row without focus or events. It is an ordinary Ratatui
widget, so it works in a plain Ratatui app with no `Ratcn` runtime. Everything
is addressed by index rather than by value — which tab is selected, which the cursor is on
(`.focused_item(...)`), which are disabled, and whether the row itself has focus:

```rust
use ratcn::TabsWidget;

frame.render_widget(
    TabsWidget::new(&["Overview", "Analytics", "Reports"])
        .selected_item(Some(selected_index))
        .focused_item(Some(cursor_index))
        .disabled_items(&[false, true, false])
        .focused(row_has_focus)
        .hovered_item(hovered_index)
        .themed(&theme),
    area,
);
```

`.disabled(true)` dims the whole row, as `.disabled_items(...)` does for single
tabs. `.size(...)` picks the row height, the same `TabsSize` the component
takes. Call `.height()` and `.width()` on the built widget when the surrounding
layout needs to reserve space for it — `width()` sums every label plus its
padding, the width the row wants before it starts scrolling. Replace
`.themed(...)` with `.style(...)` to supply exact widget colors.

## Full API

Every method, with binding requirements and edge-case detail:
[`Tabs`](https://docs.rs/ratcn/latest/ratcn/struct.Tabs.html),
[`Tab`](https://docs.rs/ratcn/latest/ratcn/type.Tab.html),
[`TabsWidget`](https://docs.rs/ratcn/latest/ratcn/struct.TabsWidget.html),
[`TabsActivation`](https://docs.rs/ratcn/latest/ratcn/enum.TabsActivation.html),
[`TabsSize`](https://docs.rs/ratcn/latest/ratcn/enum.TabsSize.html),
[`TabsStyle`](https://docs.rs/ratcn/latest/ratcn/struct.TabsStyle.html).

Mouse input needs capture enabled in the host. See [Mouse input](../concepts/mouse).

## See also

For tabs coordinating several screens, each with its own state and messages, see
[Structuring a larger app](../concepts/composition).
