---
description: "Transient corner notifications for Ratatui apps. Toast is one message, ToasterState is the stack your app owns, and ToasterWidget draws it."
---

# Toast

Transient notifications stacked in a corner. `Toast` is one message,
`ToasterState` is the stack your app keeps, and `ToasterWidget` draws it.

<div class="ratcn-preview-window" style="--ratcn-preview-height: 420px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p toast</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/toast-demo/index.html" title="ratcn toast demo"></iframe>
  </div>
</div>

```rust
use ratcn::{Toast, ToasterState, ToasterWidget};

// In update(), when something happens:
state.toasts.push(Toast::success("Saved"), now);

// In draw(), after everything else, so toasts sit on top:
frame.render_widget(
    ToasterWidget::new(&state.toasts, now).themed(&theme),
    frame.area(),
);
```

Toasts take no focus and handle no events. They are paint-only, so they work in
a plain Ratatui app with no `Ratcn` runtime.

## Kinds

The kind sets the accent color and icon. Each has a shorthand constructor, or
pass one to `.kind(...)`.

| Kind | Shorthand | Use for |
| --- | --- | --- |
| `Default` | `Toast::new` | Neutral news. |
| `Success` | `Toast::success` | Something worked. |
| `Error` | `Toast::error` | Something failed. Consider `.persistent()`. |
| `Warning` | `Toast::warning` | Needs attention, but did not fail. |
| `Info` | `Toast::info` | Neutral information, accented to stand out. |
| `Loading` | `Toast::loading` | Work in progress. Usually persistent. |

```rust
Toast::error("Upload failed")
    .description("Check your connection and try again.")
    .persistent()
```

`.description(...)` adds a second line under the title, and `.border(false)`
drops the border on one toast.

## Your app owns the clock

Ratcn never calls `Instant::now`. Every method that cares about time takes a
`Duration` from you, which is what lets toasts work in the browser and be tested
without sleeping.

The loop is three steps:

```rust
// 1. Push with the current reading from your clock.
state.toasts.push(Toast::success("Saved"), now);

// 2. Ask when to wake up next, and wait that long.
match state.toasts.time_until_next_expiry(now) {
    Some(timeout) => poll_for_input(timeout)?,
    None => wait_for_input()?, // nothing expires; block until input
}

// 3. When the timer fires, drop what expired and redraw if anything changed.
if state.toasts.prune_expired(now) {
    redraw();
}
```

Without pruning, expired entries remain in `ToasterState`, but
`ToasterWidget` hides them while painting. In a browser loop that redraws
continuously, pruning once per frame is enough.

Toasts expire after 4 seconds by default. `.duration(...)` changes that and
`.persistent()` disables it.

`ToasterState` can prune expired entries or clear the entire stack, including
persistent toasts.

## Addressing a toast by id

Give a toast an id and it can be dismissed or replaced later — the way to end a
persistent "working…" toast when the work finishes:

```rust
// When the upload starts:
state.toasts.push(Toast::loading("Uploading…").persistent().id("upload"), now);

// When it finishes — swap in place, restarting the 4-second lifetime from now:
state.toasts.replace("upload", Toast::success("Uploaded").id("upload"), now);

// Or just remove it:
state.toasts.dismiss("upload");
```

Both return `false` when no toast carries the id, so a redraw can be skipped.
Ids are not deduplicated; with several matches, the oldest entry is affected
whether it has expired or not. Prune first when expired entries should not take
precedence. `dismiss` needs no clock reading; `replace` uses `now` only to start
the replacement's lifetime. Apps that need a different lifecycle entirely can
own a custom collection of `ToastEntry` values and render it with
`ToasterWidget::from_entries(...)`.

To let Escape dismiss the most recently pushed toast, identified or anonymous:

```rust
if key.code == KeyCode::Esc {
    let _ = state.toasts.pop_newest();
}
```

## Placement

`.position(...)` picks the corner or edge; the stack grows away from it, so the
newest toast is always nearest. `.toast_width(...)`, `.gap(...)`, and
`.inset(x, y)` size and inset the stack, and `.max_visible_toasts(n)` caps how
many are candidates to show at once. Older ones stay in the state and still
expire on schedule, while candidates may still not fit the available area.

```rust
use ratcn::ToastPosition;

ToasterWidget::new(&state.toasts, now)
    .themed(&theme)
    .position(ToastPosition::TopRight)
    .max_visible_toasts(5)
```

Positions are `TopLeft`, `TopCenter`, `TopRight`, `BottomLeft`, `BottomCenter`,
and `BottomRight`. `ToastPosition::is_top()` reports which way a stack grows, if
your own layout needs to know.

## Sizing

Toasts have no fixed height — the title and description wrap at the stack width
and the widget measures the result. If the area cannot hold every visible toast,
the newest that fit whole are drawn and the rest wait for the next frame. A toast
is never clipped mid-content and toasts never overlap.

## Styling

`.themed(&theme)` derives every color. `.style(ToasterStyle)` takes explicit
ones — one background and border shared by all toasts, plus an accent per kind.

```rust
use ratcn::ToasterStyle;

let mut style = ToasterStyle::from_theme(&theme);
style.error = theme.accent;

ToasterWidget::new(&state.toasts, now).style(style)
```

Every toast draws a border by default. `.border(false)` on a toast drops it for
that toast, and `ToasterStyle::border_style` picks the line-drawing glyphs for
all of them.

## Custom stacks

`ToasterState` covers the common lifecycle. An app that needs a different one
can keep its own collection of `ToastEntry` values —
`ToastEntry::new(toast, created_at)` pairs a toast with its creation time — and
paint it with `ToasterWidget::from_entries(...)`:

```rust
use ratcn::ToastEntry;

let entries = vec![ToastEntry::new(Toast::success("Saved"), now)];
frame.render_widget(
    ToasterWidget::from_entries(&entries, now).themed(&theme),
    frame.area(),
);
```

For writing your own renderer against `entries()`, `Toast` and `ToastEntry`
expose read accessors: `title`, `description_text`, `toast_kind`, `toast_id`,
`is_bordered`, `is_expired_after`, `created_at`, `age`, and `is_expired`.
`ToasterWidget::visible()` iterates what would be drawn.

## Full API

Every method, with parameter and edge-case detail:
[`Toast`](https://docs.rs/ratcn/latest/ratcn/toast/struct.Toast.html),
[`ToastKind`](https://docs.rs/ratcn/latest/ratcn/toast/enum.ToastKind.html),
[`ToasterState`](https://docs.rs/ratcn/latest/ratcn/toast/struct.ToasterState.html),
[`ToastEntry`](https://docs.rs/ratcn/latest/ratcn/toast/struct.ToastEntry.html),
[`ToasterWidget`](https://docs.rs/ratcn/latest/ratcn/struct.ToasterWidget.html),
[`ToastPosition`](https://docs.rs/ratcn/latest/ratcn/enum.ToastPosition.html),
[`ToasterStyle`](https://docs.rs/ratcn/latest/ratcn/struct.ToasterStyle.html).

## See also

Loop and backend wiring, including the browser case:
[Host integration](../concepts/host-integration).
