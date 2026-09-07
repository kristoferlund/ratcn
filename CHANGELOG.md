# Changelog

Notable changes to `ratcn`, a preview release. Breaking changes are listed first.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.3] - 2026-09-07

Try ratcn in your own terminal, no Rust installation or account needed:

```sh
ssh ratcn.com
```

Browse interactive components, switch themes, and explore demo apps in the
live showcase. Start with a button, then try a Kanban board or a multi-step wizard.

### Breaking

- `Ratcn::render(frame, area, state, theme, declare)` now takes `area` second.
  Pass `frame.area()` for the whole frame or a pane rectangle. The area bounds
  floating placement, layer copies, and modal dimming, but does not clip
  arbitrary base paint or restrict input routing.

### Added

- `Ratcn::render_into(buffer, area, state, theme, declare)` renders into a
  caller-owned buffer for offscreen content and previews. The caller allocates
  and clears the buffer and handles windowing; no cursor metadata is returned.
- `FocusState::none()` and `is_none()` represent no focused component, distinct
  from default startup focus. This does not disable input: input can focus the
  tree again, and closing a modal restores a saved no-focus state.
- The `cargo-ratcn` CLI: `cargo ratcn init` sets up an
  existing terminal project, keeping Cargo's default main or installing a starter;
  `cargo ratcn add` copies components from the project's resolved `ratcn` package.
- A terminal showcase with reusable buffer-based demos and a Getting started guide.

### Changed

- The website and documentation are now at [ratcn.com](https://ratcn.com).

### Fixed

- Wide glyphs cut by a layer's clipping edge no longer paint over host content
  outside the render area; complete glyphs at the edge remain intact.

## [0.0.2] - 2026-08-26

### Breaking

- `runtime::RenderCtx` becomes `runtime::DeclareCtx<'a, State, Msg>`, dropping
  the frame lifetime. `Component::render` becomes `Component::declare`, and
  `RenderCtx::render_component` becomes `DeclareCtx::component`.
- Declaration runs once per frame, and `Ratcn::render` takes `FnOnce`, not
  `FnMut`. Move drawing out of declaration into `Component::paint` or
  `DeclareCtx::paint`; use `DeclareCtx::paint_widget` for an owned widget.
- Interaction fields move from `RenderCtx` to accessors on `PaintCtx<'a, State>`.
  `PaintCtx::render_widget` becomes `widget`; `render_stateful_widget` becomes
  `stateful_widget`. `with_buffer` is paint-only; `runtime::Painter` is removed.
- Components paint before their descendants. Use `DeclareCtx::defer_paint`
  for overlays; its closure now receives `PaintCtx` instead of `Painter`.
- Hover is runtime-owned: remove `Ratcn::hover`, `HoverState`, and app hover
  fields, messages, and update arms. Read `PaintCtx::hovered`/`contains_hover`
  when painting, or `DeclareCtx::pointer_within`/`pointer_within_area` when
  declaring. Raw pointer position is available through `PaintCtx::hover_position`.
- Pointer motion no longer emits hover messages. Redraw on `EventResult::Consumed`;
  motion is consumed when a routing surface exists, even within the same control.
- `Component::is_focusable` is replaced by `ScopeOptions::focusable(bool)` via
  `scope_options`. `Component::focuses_on_click` is removed: focus occurs on press.
  Replace `FocusState::is_path` with a `path()` comparison, not `contains_path`.
- `runtime::compose`, `BodyFn`, `BodySlot`, and `ChildSlots` are removed. Store
  composite bodies as `Option<Box<dyn FnOnce(&mut DeclareCtx<'_, S, M>)>>`.
  `PreparedComponent` and `render_prepared_component` are removed; use `component`.
- `DeclareCtx::hint` and `popup` take `(id, area, options, declare)`.
  `ModalState::ids` returns a clonable exact-size iterator, not a slice.
  Opening a modal clears focus for resolution inside it; closing restores the
  saved path, or returns `None` without changing focus when the stack is empty.
- `MouseEvent` coordinates and `DragPhase` positions use the receiving
  component's declaration coordinates, matching `EventCtx::area` inside viewports.
- `ListWidget`, `SelectWidget`, and `TabsWidget` rename row/option/tab state
  setters to `focused_item`, `selected_item(s)`, `disabled_items`, and `hovered_item`.
  `Tabs::tab_focus` becomes `item_focus`; `Select::max_visible_options` becomes
  `max_visible_items`. `List::render_item` and `Select::render_item` become `paint_item`.
- `ListWidget::scroll_offset` becomes `first_item`; pass only the visible rows.
  Replace `new(&items).scroll_offset(n)` with
  `new(&items[n.min(items.len())..]).first_item(n)`. Focus, selection, and disabled
  indices still refer to the whole list.
- `SelectWidget::scroll_offset` becomes `first_item`; `option_rows` becomes
  `visible_item_rows`, with one row per painted option. `open` takes a `bool`;
  pass all labels through `options` so the panel can measure its height.
- `list_core::WheelPark` becomes `WheelHold<T>`; `park` becomes
  `hold(offset, items, cursor)`. Use `settle(items, cursor, requested, viewport, area)`
  instead of separate settle/record/cursor/offset calls. The hold requires
  `T: Clone` and is no longer `Copy`; `List`'s component impl requires `T: 'static`.
- `Tooltip` opens on hover by default. Its `open_when` and `open` readers now
  take `Fn(&S, bool) -> bool`, with the second argument reporting hover.
- `ButtonRenderMode`/`ButtonFill` and `ButtonStyle::mode` are removed; border
  colors are `Option<Color>` per state, with `Some` painting a border.
  `BarChartWidget::width`/`height` become `span`, measuring the grouping axis.
- `Toast` builders become `with_description`, `with_kind`, and `with_id`;
  readers `description_text`, `toast_kind`, and `toast_id` become `description`, `kind`, and `id`.
  `Tab<T>` aliases `ListItem<T>`; separate trait implementations for both now conflict.
- `selection_indicator::marker(selected, multiple)` becomes `MarkerGlyphs::marker(selected)`.
  Direction-specific color shifts become `FOCUS_SHIFT`, `HOVER_SHIFT`,
  `FIELD_FOCUS_SHIFT`, and `FIELD_HOVER_SHIFT`; `color::darken` becomes `dim` toward black.
  `ListStyle`, `SelectStyle`, `TabsStyle`, and `ButtonStyle` theme builders are no longer `const`.
- `runtime::geometry` helpers move to `ratcn::geometry`; import drag helpers
  from `ratcn::runtime`, not the now-private `runtime::drag` module.
- `crossterm::InputModes::mouse_capture` becomes `mouse`; `bracketed_paste` becomes
  `paste`. Overlapping `InputModeGuard`s do not compose: dropping either disables
  the modes it enabled.
- `linear_nav::nav_key_target` and `is_step_key` require `Axis`; `wheel_offset`
  takes `ScrollDirection` and returns `Option<usize>`. `list_core::windowed_rows`
  takes `(cursor, disabled, selected, row)` and builds each `ListItemState`.

See the [full version comparison][0.0.2] for all API removals and smaller changes.

### Added

- `Checkbox` and `Cycle`, with styles and paint-only widgets, plus
  `ProgressWidget` for a themed progress bar with optional label and percentage.
- `ScrollArea` clips descendant paint and pointer input and reveals focused
  descendants. Bind its first visible row with `scroll(read, on_change)`.
  Custom viewports use `DeclareCtx::viewport` and `Component::reveal_in_viewport`;
  modals inside viewports escape the viewport clip.
- `terminal::Session` with the optional `termina` feature manages terminal setup,
  events, and restoration, including on panic. `termina` and `crossterm` can be
  enabled together; neither is enabled by default.
- `SessionOptions::adaptive()` follows terminal colors. Read `session.theme()`
  or `theme_with_fallback(preset)` each frame; `Theme::adaptive` derives light or
  dark themes from supplied colors. New color helpers include luminance and contrast.
- Custom selection markers for `List`, `Select`, and `SelectWidget` through
  `selected_marker`/`unselected_marker` and `selection_indicator::MarkerGlyphs`.

### Changed

- Theme presets are retuned for consistent surfaces and readable text, including
  destructive buttons. Fills adapt to light backgrounds; `Theme::terminal` uses
  neutral white rather than `LightBlue` as its primary color.
- Lists and select panels build only painted rows, and selects share panel options.
  Duplicate item-value checks run only in debug builds. Text wrapping and truncation
  are faster, and toasts wrap once per frame rather than twice.
- Invalid declarations leave the previous frame and routing surface intact.

### Fixed

- Outline and Ghost buttons preserve the underlying surface instead of painting
  a background stripe; resting Ghost buttons no longer grow unwanted caps.
- Popups and hints outside a modal stay below it and dim with the background.
  Duplicate modal IDs are rejected even when nested.
- Presses update hover before freezing it for a gesture. Abandoned drags no longer
  leave stale state that moves a component or swallows a later release.
- Pointer motion reaches a control on entry, so list/select cursors, tooltips,
  and hover focus respond to the first motion rather than the next one.
- Wheel scrolling releases its hold when items or the cursor change, keeping
  the cursor visible after replacement, reordering, filtering, or removal.
- Grouped bar-chart measurement includes bar gaps and ignores empty groups,
  avoiding clipped final bars. All-disabled controls no longer swallow navigation keys.
- `terminal::Session::next` reports I/O errors when a color re-query cannot be written.

## [0.0.1]

First public release.

[Unreleased]: https://github.com/kristoferlund/ratcn/compare/v0.0.3...HEAD
[0.0.3]: https://github.com/kristoferlund/ratcn/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/kristoferlund/ratcn/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/kristoferlund/ratcn/releases/tag/v0.0.1
