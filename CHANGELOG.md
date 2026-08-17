# Changelog

Notable changes to `ratcn`. This project is a preview release, so the API is
still moving — breaking changes are listed first for each version.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `RenderCtx::pointer_within`, asking whether the pointer rests on the current
  declaration or anything inside it. Hover is known before a pass starts — it
  was resolved against the last committed frame — so unlike focus it is
  readable while declaring, and structure may depend on it. `Tooltip` is the
  reference consumer.
- `Component::paint`, where a component draws. It is queued where
  `Component::render` declared the component and replayed once the whole tree
  is declared and focus has resolved, so it runs once per frame and a
  component draws before its own descendants. It defaults to drawing nothing.
- `runtime::PaintCtx`, the context `paint` receives: the paint surface
  (`render_widget`, `render_stateful_widget`, `with_buffer`), the theme, the
  declared area, the app state, the pointer position, and the four interaction
  flags.
- `RenderCtx::paint`, the app-level counterpart for chrome with no component
  of its own. It queues a `'static` closure at the point it is reached.

### Removed

- **Breaking:** `Ratcn::hover` and `runtime::HoverState`. Hover is the
  runtime's own state now: every pointer event records where the pointer is,
  every committed frame resolves hover from that position against the surface
  it just declared, and a motion resolves it immediately. A gesture freezes
  it — from press to release hover stays on what the gesture started on,
  unless that target is covered or stops being declared. Apps drop the field, the message
  variant, the `update` arm, and the binding; `PaintCtx::hovered` /
  `contains_hover` and the new `RenderCtx::pointer_within` are how it is read.
  A stored path could go stale, so the reconciliation that resolved it — and
  the corrections it emitted on the next pointer event — are gone with the
  type.
- **Breaking:** the hover message class. Pointer motion never returns a hover
  message. `EventResult::Consumed` is the redraw signal that replaces it: with
  a surface to route against, a motion is never `Ignored` — whether or not it
  moved hover, and whether or not a component handled it — because paint may
  read the pointer position itself through `PaintCtx::hover_position`, so
  motion within one component is as much news to the next frame as motion
  between two.
- **Breaking:** the two-pass declaration and its idempotency contract. The
  declaration closure runs **once** per frame. It may have side effects, and
  the divergence panics that policed the contract (`declaration closure is not
  idempotent: ...`) are gone with it.
- **Breaking:** `RenderCtx::render_widget`, `RenderCtx::render_stateful_widget`,
  and `RenderCtx::with_buffer`. Declaring no longer paints: use
  `RenderCtx::paint` from an app declaration and `Component::paint` from a
  component. The scratch buffer `with_buffer` used to run against is gone with
  them — a paint closure runs once, against the real surface.
- **Breaking:** the `RenderCtx::focused`, `contains_focus`, `hovered`, and
  `contains_hover` fields. Focus resolves against the tree a declaration is
  still building, so no flag exists while declaring; `PaintCtx` reports all
  four.
- **Breaking:** `runtime::compose`, with `BodyFn`, `BodySlot`, and `ChildSlots`.
  A composite now holds a caller-supplied body as
  `Option<Box<dyn FnOnce(&mut RenderCtx<'_, '_, S, M>)>>` and a measured child
  as a closure that declares it, both private to the component. See the custom
  components guide; `Dialog` is the reference implementation.
- **Breaking:** `runtime::PreparedComponent` and
  `RenderCtx::render_prepared_component`. Preparing a component is internal to
  `RenderCtx::render_component` again.

### Changed

- **Breaking:** `ListWidget` takes the rows on screen rather than the whole
  list. `ListWidget::scroll_offset` is replaced by `ListWidget::first_item`,
  the index `items[0]` has in the whole list; `focused_row`, `selected_rows`,
  and `disabled_rows` still count from the start of the list, so scrolling
  changes only that number and the rows. `new(&items).scroll_offset(n)` becomes
  `new(&items[n.min(items.len())..]).first_item(n)` — the clamp matters, because
  the old call painted an empty list for an offset past the end where slicing
  panics. The widget holds no scroll position, and a long list costs a long
  list's worth of `Text` only if the caller builds one.
- **Breaking:** `SelectWidget::option_rows` is `SelectWidget::visible_option_rows`
  and takes one row per *painted* option, in paint order, starting at
  `scroll_offset`. `open` still takes every option, because the panel's height
  is measured from their count.
- **Breaking:** `List`'s `Component` implementation requires `T: 'static`.
  `RenderCtx::render_component` already required it of any `List` declared
  through the runtime, so a declared list is unaffected; a `List` driven
  directly through the `Component` trait — `prepare`/`handle_event` against an
  `EventCtx::default()`, which is how the transient docs suggest testing one —
  now needs an owned item type too. The park below keeps an item value in the
  transient store, and while a transient is keyed by its identity *path*, the
  type stored at that path is asserted on read, so the value's type is part of
  what reader and writer must agree on.
- **Breaking:** `list_core::WheelPark` is generic over the item value, and the
  wheel's hold now persists only while the list has not moved under it: the
  anchored item is still at its anchored row, in a list of the same length,
  with the cursor still on it. Replacing that item, reordering, filtering,
  inserting, removing, or moving the cursor releases the hold and scrolls the
  cursor back into view. Before, a hold anchored by row index survived an item
  being swapped for another at the same position and left the new one
  off-screen.
  - `settle`, `record`, `cursor_to_show`, and `offset` collapse into one
    `settle(items, cursor, requested, viewport, area)` that releases the hold,
    computes the offset, and records it in the `RowViewport`.
  - `park(offset, items, cursor)` replaces `park(offset, cursor)` and captures
    the whole anchor itself, so no caller can assemble half of one. It is no
    longer `const`, and its impl block requires `T: Clone` to keep the value.
  - `WheelPark` is no longer `Copy`, and `Debug`, `Clone`, `PartialEq`, and `Eq`
    now hold only when the item type does. `Default` is unconditional.
- **Breaking:** `List`, `Select`, and `Tabs` check their item values for
  duplicates only under `cfg!(debug_assertions)`. The scan is quadratic —
  values are only `PartialEq`, so there is nothing to sort or hash by — and
  every frame declares a fresh component, so the check ran once per frame in
  every build. A debug build still runs it; a release build no longer pays for
  an answer that cannot change unless the items do. The panic message is
  unchanged, and the rustdoc and guides now say the failure is debug-only
  rather than promising it unconditionally.
- **Breaking:** a `Tooltip` with no `open_when`/`open` binding shows while the
  pointer is inside it, where before it never showed at all. Both readers take
  `Fn(&S, bool) -> bool`, the second argument being that same hover answer, so
  an app rule can gate it (`|s, hovered| hovered && !s.disabled`) or widen it
  (`|s, hovered| hovered || s.focus.contains_path([id])`).
- **Breaking:** pointer motion is no longer swallowed by the hover change it
  causes. `Moved` reaches the component under the pointer on the crossing
  event, so a `List` or `Select` cursor follows the mouse from the motion that
  enters it rather than the one after, and a `Tooltip` bound with `open` asks
  to open on that same motion. With `hover_focus` on, one motion both moves
  hover and emits the focus change.
- **Breaking:** `Ratcn::render`'s declaration closure is `FnOnce` rather than
  `FnMut`. Values may be moved into the components it declares instead of
  cloned per run.
- **Breaking:** `Component::render` is declaration only. It lays the component
  out, declares its descendants, and records what `handle_event` reads back;
  it writes no cells and is not offered the interaction flags. Everything that
  draws moves to `Component::paint`.
- **Breaking:** `Dialog` and `Tooltip` hold their bodies, footer, and actions in
  private types. Their builders are unchanged.
- `List` and a `Select` panel build only the rows they paint. A thousand-item
  list showing fifteen rows builds fifteen, `render_item` still receives each
  row's index in the whole list, and a multi-selection predicate is asked once
  per painted row rather than once per item. Declaring and painting a
  thousand-item list into a fifteen-row frame drops from 457 µs to 40 µs in a
  release build, split evenly between two causes: dropping the per-frame
  uniqueness scan is 208 µs of it and grows with the square of the item count,
  and building one screenful of rows instead of a listful is the other 209 µs
  and grows linearly. A debug build keeps the first cost. The button benches,
  which share no code with either change, do not move.
- A component's own paint now always precedes its descendants' — a component is
  queued at the point it opens, so container chrome needs no care about
  ordering. The converse is no longer expressible from `paint`: decoration that
  must cover a composite's descendants belongs in `RenderCtx::defer_paint`. The
  kanban demo's drag ghost was already written that way; nothing in-tree had to
  move.
- A rejected pass paints nothing. Declaration, validation, and the modal-stack
  check all complete before the first cell is written, so a poisoned or
  modal-mismatched pass leaves the previous frame on screen as well as the
  previous surface.
- The declaration closure and every `Component::render` now run once per
  frame rather than twice. Anything a declaration does beyond declaring — a
  counter, a log, an `Rc<RefCell>` write — happens half as often, and
  `RenderCtx::transient_mut` writes once rather than twice.
- `RenderCtx::in_area` is documented rather than hidden: it is how a composite
  hands a caller-supplied body the area it laid out for it.
- A `Dialog` action is prepared where it is declared rather than in the dialog's
  own `prepare`. Preparation reads only the declaring state, which cannot change
  within a frame.
- Rendering one retained composite instance twice by hand no longer panics; the
  second render declares nothing. Every frame builds a fresh instance, so this
  was never reachable through `Ratcn::render`.

## [0.0.1]

First public release.

[Unreleased]: https://github.com/kristoferlund/ratcn/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/kristoferlund/ratcn/releases/tag/v0.0.1
