# Changelog

Notable changes to `ratcn`, a preview release. Breaking changes are listed first.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The `ui-f-state` demo: a short film about `ui = f(state)` with a synthesized
  soundtrack, ending in a live counter. Terminal only:
  `cargo run --release -p ui-f-state`.
- `List::from_shared` declares a list over an `Arc<[ListItem<T>]>` the app
  already holds, so a long list can be declared without copying its items
  every frame.
- `BrowserInput` (wasm, `ratzilla` feature), a guard beside `BrowserClipboard`
  that delivers what Ratzilla misses: non-ASCII keys and the mouse wheel.
- `list_core::PressedItem`, so a custom item component can hold a click to the
  item that was pressed, as `List`, `Select` and `Tabs` now do.

### Changed

- In an app with a focus binding, the press that closes a popup by landing on
  a control without focus now emits the popup's `on_dismiss` and focuses that
  control on release, without activating it. A control that already holds
  focus is clicked as usual.
- A bound `ScrollArea` is strictly controlled, like a bound `List`: an offset
  the app does not apply is never shown. A focus reveal still holds the view
  until the next scroll.
- A `List` lets the wheel bubble where it cannot scroll, so an enclosing
  `ScrollArea` keeps scrolling over it.
- After Esc closes a bound `Tooltip`, it stays closed until the pointer leaves
  the trigger.
- `Tabs` implements `Component` only for `T: 'static`; `ctx.component` already
  required that.

### Fixed

- A click on a `List` row, a tab, or a `Select` option acts only when press and
  release land on the same item. A redraw that moved another item under the
  pointer no longer commits it.
- A press whose target vanished from a rendered frame no longer clicks whatever
  returns at its path.
- Motion with a button held is never `Ignored`, as the mouse docs promise.
- Pressing blank space in a scrolled, unfocused `ScrollArea` focuses a control
  on screen instead of scrolling back to the first one.
- Controls show hover fill and a hand or text-beam pointer only where a click
  acts. An all-disabled `Select` no longer opens on a click.
- Dragging a `Dialog` after a resize clamped it no longer has a dead zone.
- Browser demos type Option and AltGr characters such as `@` on a Swedish Mac,
  and scroll with the mouse wheel without scrolling the page.

## [0.1.0] - 2026-11-11

The first release in the 0.1 series. From here on, a patch release (0.1.x) is
never breaking; breaking changes wait for 0.2.

### Breaking

- `terminal::SessionBackend` is a type of its own rather than an alias for
  `TerminaBackend<PlatformTerminal>`, generic over the termina terminal with
  `PlatformTerminal` as the default. It is still the backend
  `Session::terminal_mut` draws through.
  It does not implement Ratatui's `scrolling-regions` feature, so an app that
  turns that feature on no longer builds against a session.

### Added

- `terminal::Session::set_progress` shows a `TerminalProgress` in the
  terminal's own chrome (OSC 9;4). It writes only on change and clears on exit.
- `terminal::Session::set_title` sets the terminal's window title (OSC 2). It
  writes only on change, and gives the user's own title back on exit through
  xterm's title stack.
- A `terminal::Session` sends every frame as one synchronized update (mode
  2026), so the terminal shows it only once it is whole: large redraws and
  resizes no longer tear. Terminals without the mode ignore it. A frame left
  open by a failed draw or a bare `clear` is closed before `Session::next`
  waits.
- `terminal::Session::set_program_status` reports what the app is doing (idle,
  working, blocked, done, error) with the OSC 7501 Program Status Protocol,
  revision 0.4, writing each record only on change; `clear_program_status`
  removes a task's record. On exit the session takes back records still
  working or blocked. The `program-status` demo shows each state.

### Fixed

- A `terminal::Session` on a terminal without 24-bit color draws with the
  nearest of the 256 or 16 colors it has, instead of rgb it misreads:
  Terminal.app before macOS 26 dimmed the text and painted stray backgrounds.
  The depth is read from the environment as termenv does, and confirmed with
  Neovim's DECRQSS probe where the environment does not say 24-bit (not on
  the Linux console). On 16 colors each color keeps its hue rather than
  falling to the nearest gray. Set `COLORTERM=truecolor` to force 24-bit.
  That probe goes out for every session, adaptive or not, so such a session
  opens one round trip later, or up to a second on a terminal that answers
  nothing.
- Adaptive terminal sessions no longer fail to open immediately on macOS due to
  a rustix timeout rounding bug.

## [0.0.6] - 2026-10-06

### Breaking

- `runtime::BrowserPasteListener` is `BrowserClipboard`. `install` takes the
  app's element, acting only while focus is within it, and a closure returning
  `Ratcn::take_clipboard`. It carries copy and cut, and stops Cmd chords and
  (off a Mac) Ctrl+C/X/V reaching ratzilla as keys.

### Added

- `Input` and `TextArea`, single-line and multi-line text fields, each with its
  paint widget and style. Their text lives in app state as an `InputState` or a
  `TextAreaState`; `text_edit` holds the state types and the editing mechanics
  a text component of your own builds on. `Input` takes a `prefix` and `suffix`.
- Clipboard writes: `EventCtx::set_clipboard`, taken by the host with
  `Ratcn::take_clipboard` and written with `Session::set_clipboard` (OSC 52) or
  `BrowserClipboard::write`. `Event::Copy` and `Event::Cut` carry the
  platform's copy and cut gestures.
- Pointer shapes: components ask with `PaintCtx::set_pointer_shape` (a hand
  when pressable, a text beam in fields, `Grabbing` while `pointer_captured`);
  the host shows `Ratcn::pointer_shape` with `Session::set_pointer_shape`
  (OSC 22) or `runtime::set_browser_pointer_shape`.

## [0.0.5] - 2026-10-03

### Breaking

- Layers are transparent: a modal, popup, or hint covers only the cells its
  content paints. Paint `Clear` first to hide what is beneath. Every layer now
  declares in screen coordinates, so paint at the layer's `ctx.area()`.
- `DeclareCtx::defer_paint` is removed: declare a `hint` for decoration above
  later siblings, or paint onto the buffer after `render`.
- `Ratcn::render`/`render_into` take `FnMut`: a frame whose focus reveal scrolls
  a viewport is declared twice. Build what the closure consumes inside it.
- `Component`: `prepare` is removed, `scope_options` and `interaction_area`
  receive the state, and `reveal_in_viewport` returns whether it scrolled.
- `DeclareCtx::transient::<T>()` returns `&mut T`, like `EventCtx::transient`;
  `transient_mut` is removed.
- `PaintCtx::with_buffer(area, |area, buf| …)` takes the area it writes;
  `stateful_widget` is removed.
- `Cycle::width()` includes its padding and grows by 2.
- `Ratcn::has_rendered` and `BrowserEventError` are removed.

### Added

- `runtime::BrowserPasteListener` (`ratzilla` feature) forwards browser pastes
  as `Event::Paste`.

### Fixed

- Focus onto a clipped target declared that same frame (startup focus, an
  appended row) scrolls it into view immediately, not on the next render.
- A List without a `scroll` binding, and Select's panel, no longer drag the view
  when the cursor moves within it.
- A click right after a wheel scroll hits the row still on screen.
- Clicking the empty space of a scope that holds focus no longer moves focus.
- A popup without `on_dismiss` no longer blocks the popup beneath it from
  dismissing.
- `terminal::Session` no longer writes to the terminal on a panic after it has
  closed.

## [0.0.4] - 2026-09-26

### Added

- `EventCtx::pointer_captured` reports whether the current event arrived through
  this component's own pointer capture, rather than by hit-test or from a
  captured descendant.

### Changed

- A path keeps one transient per type, so a component can hold its own
  `EventCtx::transient` state alongside `EventCtx::drag`. Asking for a second
  type no longer panics.
- Landing demo Tab traversal now continues between tiles; arrows and Vim keys
  remain within item controls. The scroll-area demo uses Tab/Shift+Tab between
  buttons rather than remapping arrows to focus traversal.
- The landing demo's Quake III asset checklist is one multi-select List:
  Up/Down or `k`/`j` navigates, Enter/Space toggles, and Tab leaves the list.

### Fixed

- `ScrollArea`'s scrollbar thumb can be dragged with the mouse. A press on the
  thumb holds until movement and keeps the grabbed point; a press on the track
  jumps the view. Previously the thumb was paint-only.
- Enter/Space in the showcase demo list selects and focuses the chosen demo
  without forwarding that key to its controls; Right still enters the preview.
- Shift+Tab traverses backward whether a backend sends `BackTab` or shifted
  `Tab`; open Selects recognize both forms when closing their panel.
- An already-focused Select can still close with Esc or Tab after its items
  become empty. Cycle and Checkbox changes compose between redraws.
- Plain Esc dismisses the landing screensaver and restores focus. Modified Esc
  no longer leaves an embedded showcase demo at the host level.

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

[Unreleased]: https://github.com/kristoferlund/ratcn/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/kristoferlund/ratcn/compare/v0.0.6...v0.1.0
[0.0.6]: https://github.com/kristoferlund/ratcn/compare/v0.0.5...v0.0.6
[0.0.5]: https://github.com/kristoferlund/ratcn/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/kristoferlund/ratcn/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/kristoferlund/ratcn/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/kristoferlund/ratcn/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/kristoferlund/ratcn/releases/tag/v0.0.1
