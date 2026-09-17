---
description: Every key ratcn components respond to, in one table — traversal, navigation, commit, and dismiss — plus the rules that decide which keys a component claims.
---

# Keyboard

Tab moves between independent controls. Arrows and their Vim aliases move
within a control that has items or values to navigate. A panel containing
several buttons or checkboxes does not turn them into a list.

## The map

### Traversal — moving between components

| Key | Does |
|---|---|
| `Tab` | Focus the next focusable component |
| `Shift+Tab` | Focus the previous one |

Traversal belongs to the runtime, not to components. `Tab` wrapping is
per-scope — see [Focus, hover, and identity](./focus-hover-identity). Apps add
their own jumps with [`focus_key`](./focus-hover-identity#focus).

For example, in the showcase's Agent settings, Tab chooses a setting and
Left/Right or `h`/`l` changes its value. In Themes, Notifications, and the Quake III
asset checklist, Up/Down or `k`/`j` moves among the rows of one List. Buttons and
individual checkboxes use Tab/Shift+Tab, not arrows or Vim letters, to move focus.

### Navigation — moving a cursor inside a component

Vertical controls — [List](../components/list) and an open
[Select](../components/select):

| Keys | Moves the cursor |
|---|---|
| `↓` &nbsp;`j` &nbsp;`Ctrl+N` | one item forward |
| `↑` &nbsp;`k` &nbsp;`Ctrl+P` | one item back |
| `Home` / `End` | to the first / last enabled item |
| `PageDown` / `PageUp` | one viewport |
| `Ctrl+D` / `Ctrl+U` | half a viewport |

Horizontal controls — [Tabs](../components/tabs):

| Keys | Moves the cursor |
|---|---|
| `→` &nbsp;`l` &nbsp;`Ctrl+N` | one tab forward |
| `←` &nbsp;`h` &nbsp;`Ctrl+P` | one tab back |
| `Home` / `End` | to the first / last enabled tab |

[Cycle](../components/cycle) uses the same horizontal step keys to change its
value, but wraps at both ends. It has no Home/End or paging behavior.

List, Select, and Tabs skip disabled items and clamp at the ends. They consume
navigation keys even at a boundary: pressing `j` at the last List item does not
move to the next component. If a List has no item cursor yet, its first
navigation key establishes one on the first enabled item.

A closed Select opens on Enter/Space or a vertical step key (Up/Down, `k`/`j`,
Ctrl+P/N). While open, the first Tab or Shift+Tab closes its panel without moving
component focus; the next traversal key moves focus normally.

### Scrolling content

[ScrollArea](../components/scroll-area) uses PageUp/PageDown to scroll one
viewport and Home/End to reach the bounds, without changing component focus.
Descendants receive these keys first, so a focused List pages its own items.
ScrollArea does not claim arrows, Vim letters, or Ctrl navigation chords.
Unlike item navigation, a scrolling key that cannot change the offset bubbles
to the app. Tab traversal reveals a focused descendant that was offscreen.

### Commit and dismiss

| Key | Does |
|---|---|
| `Enter` &nbsp;`Space` | Press a [Button](../components/button), toggle a [Checkbox](../components/checkbox), advance a Cycle, or commit the cursor in List, Select, or Tabs |
| `Esc` | Request dismissal of a [Dialog](../components/dialog), an open Select, or a [Tooltip](../components/tooltip) with an open-change binding |

`Dialog`'s dismiss key is rebindable with
[`dismiss_key`](../components/dialog), which takes any `KeyChord`. Chord matching
ignores Shift, so its default also accepts Shift+Esc; Ctrl/Alt must match.
Select and Tooltip require plain Esc. A read-only Tooltip (`open_when`) leaves
Esc to its ancestors or the app. Components require their action bindings to
emit these changes; disabled controls do not activate.

In the showcase, plain Esc goes to the embedded demo first and returns to the
host only if the demo leaves it unhandled. The landing screensaver also closes
on plain Esc. Browser previews additionally release browser keyboard capture
on Esc; that host behavior is separate from component dismissal.

## The rules behind the map

Three rules decide whether a component claims a key at all. They matter because
they are what keeps your app's own hotkeys working.

**Components claim their documented keys.** `Ctrl+S` is not a List key and can
reach your save handler. The supported Ctrl navigation chords are exceptions
to the usual unmodified-key rule; horizontal controls do not claim Ctrl+U/D.
Dialog dismissal and app focus shortcuts follow their configured `KeyChord`.

**Shift is not item navigation.** `J` is not `j`; Shift-modified item navigation
is left unclaimed. Shift+Tab still traverses backward between controls.

**An unhandled key bubbles.** A key a component does not recognise is reported
as ignored, and travels up to its ancestors and then to your app. So a
single-letter hotkey can keep working while a List has focus, except for its
vertical navigation letters (`j`, `k`). Horizontal controls take `h` and `l`
instead. Modal scopes prevent unhandled keys from reaching the underlying app.

An ancestor hotkey does not override a key consumed by its child. If your app
deliberately needs to override `j`, check it before calling `Ratcn::handle_event`
— see [Host integration](./host-integration). Do not globally translate Vim
letters into traversal keys: a custom text editor needs to receive its own
text and editing keys first.

## Keys outside the map

Typing a letter does not jump to a matching item: there is no typeahead.

A backend key this vocabulary has no place for — a key release, a function key
beyond `F(u8)` — does not convert into an `Event` and is ignored. See
[`KeyCode`](https://docs.rs/ratcn/latest/ratcn/runtime/enum.KeyCode.html) for
the full list of what is representable.
