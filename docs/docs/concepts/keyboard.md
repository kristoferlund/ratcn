---
description: Every key ratcn components respond to, in one table — traversal, navigation, commit, and dismiss — plus the rules that decide which keys a component claims.
---

# Keyboard

Every component uses the same vocabulary, so a key means the same thing
wherever you meet it. This page is the whole map.

## The map

### Traversal — moving between components

| Key | Does |
|---|---|
| `Tab` | Focus the next focusable component |
| `Shift+Tab` | Focus the previous one |

Traversal belongs to the runtime, not to components. `Tab` wrapping is
per-scope — see [Focus, hover, and identity](./focus-hover-identity). Apps add
their own jumps with [`focus_key`](./focus-hover-identity#focus).

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

Three names for each movement: arrows for everyone, `hjkl` for `vi`, and the
Ctrl chords `readline` put in every shell and text field. None of them collide,
so a user reaches for whichever they already know.

Disabled items are skipped rather than landed on and stepped over again, and
movement clamps at the ends — no wrapping.

### Commit and dismiss

| Key | Does |
|---|---|
| `Enter` &nbsp;`Space` | Press a [Button](../components/button); commit the cursor in List, Select, or Tabs |
| `Esc` | Close a [Dialog](../components/dialog), a [Tooltip](../components/tooltip), or an open Select panel |

`Dialog`'s dismiss key is rebindable with
[`dismiss_key`](../components/dialog), which takes any `KeyChord`. Everything
else in this table is fixed.

## The rules behind the map

Three rules decide whether a component claims a key at all. They matter because
they are what keeps your app's own hotkeys working.

**A component claims only unmodified keys.** `Ctrl+S` reaches your save handler
even while a List has focus. The navigation chords above are the deliberate
exception — `Ctrl+N` is a control's own key, because every control with a
cursor wants the same four.

**Shift is never navigation.** `J` is not `j`. Shift is left unclaimed so
range-selection can be added later without changing what any key means today.

**An unhandled key bubbles.** A key a component does not recognise is reported
as ignored, and travels up to its ancestors and then to your app. So a
single-letter hotkey keeps working while a list has focus — except for the four
letters the navigation map takes (`h`, `j`, `k`, `l`).

That last exception is a real trade. If your app needs `j` as a global hotkey,
bind it outside the focused control, or check it before calling
`Ratcn::handle_event` — see [Host integration](./host-integration).

## What is not here

**No typeahead.** Typing a letter does not jump to a matching item. Single
character matching only helps on a sorted list, and needs a keystroke buffer
with a timeout to do better — which needs a clock, and this library never reads
one. The navigation keys cover the same ground more predictably.

**No key repeat handling, no chords beyond `KeyChord`.** A backend key that this
vocabulary has no place for — a key release, a function key beyond `F(u8)` —
does not convert into an `Event` and is ignored. See
[`KeyCode`](https://docs.rs/ratcn/latest/ratcn/runtime/enum.KeyCode.html) for
the full list of what is representable.
