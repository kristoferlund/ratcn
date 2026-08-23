# Changelog

Notable changes to `ratcn`. This project is a preview release, so the API is
still moving — within each section, breaking changes are listed first.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `Checkbox`, with `CheckboxStyle` and `CheckboxWidget`: a labeled boolean
  control — marker left, label right, the whole row one hit target. The
  checked and unchecked markers are strings the app chooses (`[x]`/`[ ]`,
  `[ON]`/`[off]`), so the same component is also the switch and toggle.
- `Cycle`, with `CycleStyle` and `CycleWidget`: shows the current option and
  advances on every act — Enter, Space, arrows, their vi letters, or a click —
  wrapping at both ends. Built for settings rows: setting name left, cycle
  right.
- `selection_indicator::MarkerGlyphs`: the glyph pair a selection control
  paints. `List::selected_marker`/`unselected_marker`,
  `Select::selected_marker`/`unselected_marker`, and the same methods on both
  paint widgets override the defaults per control.
- `list_core::key_intent` and `linear_nav::Axis`: the one key map `List`,
  `Select`, and `Tabs` answer from, with the axis naming which arrows step.
  `RowViewport::wheel` is the same for the wheel.
- `SelectWidget::options`, the panel's labels as their own call.
- `BarChartWidget::span`, the length of the grouping axis.
- `ModalState` implements `Eq`.
- `ratcn::geometry`, the crate-root module holding the rect helpers components
  share: `is_border`, `wrapped_height`, and `fixed_height` — an area cropped to
  exactly the rows a fixed-height shape occupies, or empty when it cannot hold
  them, which `Button` and `Tabs` derive their interaction area from.
- `FocusState` implements `Eq`.
- `ScrollArea`, with `ScrollAreaStyle`: a vertical viewport for arbitrary
  interactive descendants. Paint and pointer input are clipped to the visible
  rows while descendants lay out against their own full rectangles — a block
  overhanging the viewport keeps its borders, and the viewport clips them.
  Focus on a clipped descendant scrolls it into view. `scroll(read, on_change)`
  binds the first visible content row to app state as a `u16`.
- `DeclareCtx::viewport`, the runtime mechanism behind `ScrollArea`, and
  `Component::reveal_in_viewport`, which the runtime calls at the start of a
  frame when focus lands on a descendant a viewport clips, however focus got
  there — a path the app's own update function stores included.
- A `modal` declared inside a `viewport` is screen-level: it opens at the place
  on screen its area names, held against the viewport's top edge for a row
  scrolled past, and declares in screen coordinates from there, so it may hold
  a viewport of its own.
- `DeclareCtx::pointer_within_area`, `pointer_within` narrowed to the
  declaration's own rectangle.
- `terminal::Session` (feature `termina`) opens the terminal — raw mode, the
  alternate screen, the input modes you ask for — and restores every one on any
  exit, panic included. `Session::next` is the event source.
- `SessionOptions::adaptive()` has the session ask the terminal what colors it
  uses and follow them: it asks again when the window regains focus, shortly
  after every change signal, and when input resumes after a pause.
  `session.theme()` and `theme_with_fallback(preset)` answer per frame.
- A `termina` feature, converting [termina](https://docs.rs/termina) events into
  `runtime::Event` the way `crossterm` does. Both features can be on at once,
  and neither is on by default.
- The `termina` feature re-exports the crate as `ratcn::terminal::termina`,
  where the types inside `SessionEvent::Input` live.
- `Theme::adaptive(background, foreground, palette16)` derives a full theme
  from externally chosen colors (e.g. queried from the terminal). A light
  background yields a light theme, and every derived theme passes the same
  contrast floors the presets are held to.
- `color::luminance` and `color::contrast` (WCAG 2.1), `color::away_from` and
  `color::nearest_to` (which end of the ramp a color shifts toward).
- `color::ROW_FOCUS_SHIFT`, previously duplicated privately in `List` and
  `Select`.
- `color::blendable`, picking the first of two colors that has channels to
  blend — how disabled fills derive on a theme whose background is the
  terminal's own.
- `DeclareCtx::pointer_within`, asking whether the pointer rests on the current
  declaration or anything inside it. Hover is known before a pass starts — it
  was resolved against the last committed frame — so unlike focus it is
  readable while declaring, and structure may depend on it. `Tooltip` is the
  reference consumer.
- `Component::paint`, where a component draws. It is queued where
  `Component::declare` declared the component and replayed once the whole tree
  is declared and focus has resolved, so it runs once per frame and a
  component draws before its own descendants. It defaults to drawing nothing.
- `runtime::PaintCtx`, the context `paint` receives: the paint surface
  (`widget`, `stateful_widget`, `with_buffer`), the theme, the declared area,
  the app state, the pointer position, and the four interaction flags as
  accessors — `focused()`, `contains_focus()`, `hovered()`, `contains_hover()`.
- `DeclareCtx::paint`, the app-level counterpart for chrome with no component
  of its own. It queues a `'static` closure at the point it is reached.
- `DeclareCtx::paint_widget`, the shorthand for a paint op that is one write:
  `ctx.paint_widget(widget, area)` queues exactly what
  `ctx.paint(move |ctx| ctx.widget(widget, area))` queues, at the same position
  in the paint queue. The widget is `'static`, so it owns its content —
  `Paragraph::new(String)` qualifies, a widget borrowing from state does not.
  Writes that share captured data, read the interaction flags, or belong
  together as one op stay with `DeclareCtx::paint`.
- `theme` is a public module, holding `theme::resolve_style` — the one fork
  between a declared `style(...)` override and a component's own `from_theme`
  derivation. Every styled component resolves through it, so a theme switch
  cannot reach some of them and miss others. `Theme` and `BorderStyle` keep
  their root re-exports.
- `list_core::row_intent` and `list_core::RowIntent`: what a pointer gesture
  over a column of value-keyed rows asks for — block a press on a disabled row,
  move the cursor, commit, rest, or bubble. `List` and a `Select` panel decide
  from this one answer and map it to their own bindings, so the two cannot drift
  on what a hover or a click over a row means.
- `list_core::windowed_rows`, which maps a window of items to the rows a widget
  paints, handing the row closure each item's index in the *whole* list and
  forcing every row to the declared height.
- `list_core::WheelHold::settle_transient`, settling the hold stored at the
  current declaration's identity — or an unheld view when a wheel has never
  stored one.
- `selection_indicator::marker_line`, the default row of a selection control:
  the marker, colored, then the label, uncolored. `List` and `Select` drew the
  identical line separately.
- `linear_nav::has_enabled`, the focusability question every item control asks:
  is there any index a cursor could land on?

### Removed

- **Breaking:** `selection_indicator::marker` takes a `MarkerGlyphs` pair
  instead of a `multiple` flag, and `marker_line` follows it — the glyph pair
  is now the app's to choose.
- **Breaking:** `ButtonFill` and `ButtonStyle::mode`. A border color is
  `Option<Color>` per state; `Some` paints bordered.
- **Breaking:** `Button::height`, `ButtonWidget::height`, `SelectWidget::height`,
  `SelectWidget::visible_items`, `BarChartWidget::vertical`,
  `BarChartWidget::width`/`height` (see `span`), `SelectStyle::selected_disabled_*`
  (never distinct from `disabled_*`), `color::darken` (`dim` toward black),
  `linear_nav::ScrollStep` and `has_reserved_modifier`.
- **Breaking:** public items nothing reaches: `color::lighten`,
  `DialogStyle::fallback`, `ToasterState::clear`, `ToasterWidget::visible`,
  `Select::height`, `Select::DEFAULT_MAX_VISIBLE_OPTIONS`,
  `SelectWidget::TRIGGER_HEIGHT`, `linear_nav::clamp_scroll_offset`, and
  `linear_nav::page_enabled`.
- **Breaking:** `runtime::Painter`. `DeclareCtx::defer_paint` takes
  `FnOnce(&mut PaintCtx<'_, '_, State>)`, which carries the theme and the app
  state and reports every interaction flag as false.
- **Breaking:** `DeclareCtx::hover_position`. A declaration reads hover through
  `pointer_within` and `pointer_within_area`, which answer with identity and
  geometry; the raw pointer position is a paint-time value, on
  `PaintCtx::hover_position`.
- **Breaking:** `FocusState::is_path`. Compare `FocusState::path()` against the
  path you have; `contains_path` tests prefixes and answers a different
  question.
- **Breaking:** `Component::focuses_on_click`, the doc-hidden hook a component
  could implement to take focus on the click. Every component focuses on the
  press.
- **Breaking:** `runtime::drag` and `runtime::geometry` as public modules.
  `CellOffset`, `DragOptions`, `DragPhase`, `clamp_offset`, and `offset_rect`
  keep their `ratcn::runtime` paths; `fixed_height`, `is_border`, and
  `wrapped_height` move to `ratcn::geometry`.
- **Breaking:** `Ratcn::hover` and `runtime::HoverState`. Hover is the
  runtime's own state now: every pointer event records where the pointer is,
  every committed frame resolves hover from that position against the surface
  it just declared, and a motion resolves it immediately. A gesture freezes
  it — from press to release hover stays on what the gesture started on,
  unless that target is covered or stops being declared. Apps drop the field, the message
  variant, the `update` arm, and the binding; `PaintCtx::hovered` /
  `contains_hover` and the new `DeclareCtx::pointer_within` are how it is read.
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
  `DeclareCtx::paint` from an app declaration and `Component::paint` from a
  component. The scratch buffer `with_buffer` used to run against is gone with
  them — a paint closure runs once, against the real surface.
- **Breaking:** the `RenderCtx::focused`, `contains_focus`, `hovered`, and
  `contains_hover` fields. Focus resolves against the tree a declaration is
  still building, so no flag exists while declaring; `PaintCtx` reports all
  four.
- **Breaking:** `runtime::compose`, with `BodyFn`, `BodySlot`, and `ChildSlots`.
  A composite now holds a caller-supplied body as
  `Option<Box<dyn FnOnce(&mut DeclareCtx<'_, S, M>)>>` and a measured child
  as a closure that declares it, both private to the component. See the custom
  components guide; `Dialog` is the reference implementation.
- **Breaking:** `runtime::PreparedComponent` and
  `RenderCtx::render_prepared_component`. Preparing a component is internal to
  `DeclareCtx::component` again.
- The `Tabs` test `duplicate_tab_values_fail_declaration`, which asserted only
  that declaring duplicate tab values panics. Its twin
  `duplicate_tab_values_panic_with_the_shared_message` has the same setup and
  asserts the exact message, so the contract is unchanged and stays checked.

### Changed

- **Breaking:** `PaintCtx<'a, State>` has one lifetime. Every `paint`
  signature loses a `'_`.
- **Breaking:** `SelectWidget::open` takes a `bool`; the labels go to
  `options`.
- **Breaking:** `ToastKind` is no longer `#[non_exhaustive]`; a new kind is a
  breaking change anyway, since `ToasterStyle` needs a color for it.
- **Breaking:** `terminal::Session::next` returns the I/O error its doc
  promised when a re-query cannot be written, instead of swallowing it.
- **Breaking:** `linear_nav::nav_key_target` and `is_step_key` take an
  `axis: Axis` naming which arrows step.
- **Breaking:** `linear_nav::wheel_offset` takes `runtime::ScrollDirection`
  and returns `Option<usize>`, `None` when the wheel moves nothing.
- **Breaking:** `list_core::windowed_rows` takes `(cursor, disabled, selected,
  row)`, building each `ListItemState` itself.
- **Breaking:** `terminal::Session::theme` and `theme_with_fallback` are no
  longer `const`.
- A panicking `Component::paint` is no longer caught and re-raised by the
  runtime's own guard; the unwind leaves `render` before commit, so the pass
  still never commits and the observable outcome is the same.
- Outline and Ghost buttons paint with `Color::Reset` at rest, so the surface
  beneath them shows through.
- A layer is one record in the retained surface; a popup or hint declared
  after a modal, outside it, paints beneath the modal and dims with it.
- The crate holds no lock: the sRGB table behind a `LazyLock` is a function.
- **Breaking:** the paint-only widgets address items by one noun.
  `ListWidget`'s `focused_row`/`selected_rows`/`disabled_rows`, `SelectWidget`'s
  `focused_option`/`selected_option`/`disabled_options`, and `TabsWidget`'s
  `selected_tab`/`focused_tab`/`disabled_tabs`/`hovered_tab` are `focused_item`,
  `selected_item(s)`, `disabled_items`, and `hovered_item`.
- **Breaking:** `SelectWidget::scroll_offset` is `first_item` and
  `SelectWidget::visible_options` is `visible_items`, the names `ListWidget`
  uses.
- **Breaking:** `Tabs::tab_focus` is `Tabs::item_focus`, the name `List` and
  `Select` use, and `Select::max_visible_options` is `max_visible_items`.
- **Breaking:** `List::render_item` and `Select::render_item` are `paint_item`,
  and `ButtonRenderMode` is `ButtonFill`. Both name cell writing.
- **Breaking:** `Toast` builders are `with_description`, `with_kind`, and
  `with_id`; their readers are `description`, `kind`, and `id`, where 0.0.1 had
  `description_text`, `toast_kind`, and `toast_id`.
- **Breaking:** `crossterm::InputModes::mouse_capture` and `bracketed_paste` are
  `mouse` and `paste`, the names `terminal::SessionOptions` uses.
- **Breaking:** two `crossterm::InputModeGuard`s over the same mode do not
  compose. Each guard switches off exactly what its own `enable` call switched
  on, so dropping either one switches the mode off.
- **Breaking:** `list_core::WheelPark` is `WheelHold` and its `park` method is
  `hold`, so "park" names focus parking alone.
- **Breaking:** `ScopeOptions::focusable` takes a `bool`, and is the one place a
  focus claim is made: `Component::is_focusable` is gone, and a component
  answers through `scope_options` from the props it was declared with, settling
  anything state-dependent in `prepare`.
- **Breaking:** `DeclareCtx::hint` and `DeclareCtx::popup` take
  `(id, area, options, declare)`, the order `scope` and `modal_scope` use.
- **Breaking:** `ModalState::ids` returns
  `impl ExactSizeIterator<Item = &ChildId> + Clone`, not `&[ChildId]`.
- **Breaking:** `ModalState::open` clears the focus path; the modal resolves
  focus to its own first focusable leaf. `close` writes the saved path back and
  returns `None` on an empty stack, leaving `focus` alone.
- **Breaking:** a paint panic a component catches leaves the declaration pass
  to finish and commit. A layer canvas composites only the rects paint
  recorded, and a paint inside a viewport reaches the frame only once it
  returns, so a widget that panicked part-way through its own area contributes
  nothing.
- **Breaking:** `MouseEvent` coordinates and `DragPhase` positions are in the
  coordinate space the receiving component was declared with, matching
  `EventCtx::area`. Outside a viewport that is the screen.
- **Breaking:** the direction-named shift constant pairs collapse into
  `color::FOCUS_SHIFT`, `HOVER_SHIFT`, `FIELD_FOCUS_SHIFT`, and
  `FIELD_HOVER_SHIFT` — the direction now comes from the theme.
- **Breaking:** `from_theme` and `themed` on `ListStyle`, `SelectStyle`,
  `TabsStyle`, and `ButtonStyle` are no longer `const fn`.
- **Breaking:** `runtime::RenderCtx` is `runtime::DeclareCtx`. `render` now
  names one thing — `Ratcn::render`, the whole frame — while `declare` names
  tree building and `paint` names cell writing. Every signature, bound, and
  stored body follows: `fn declare(&mut self, ctx: &mut DeclareCtx<'_, S, M>)`,
  `Option<Box<dyn FnOnce(&mut DeclareCtx<'_, S, M>)>>`.
- **Breaking:** `Component::render` is `Component::declare`. The hook is
  otherwise unchanged — it lays the component out, declares descendants, and
  paints nothing. `prepare`, `paint`, `scope_options`, `interaction_area`, and
  `handle_event` keep their names.
- **Breaking:** `RenderCtx::render_component` is `DeclareCtx::component`, in the
  noun family it declares alongside: `scope`, `modal`, `modal_scope`, `popup`,
  `hint`, `in_area`.
- **Breaking:** `PaintCtx::render_widget` and `PaintCtx::render_stateful_widget`
  are `PaintCtx::widget` and `PaintCtx::stateful_widget`. `with_buffer` keeps
  its name.
- **Breaking:** the declaration context loses its frame lifetime:
  `DeclareCtx<'a, State, Msg>`, where the 0.0.1 type was
  `RenderCtx<'a, 'frame, State, Msg>` — one migration with the rename above.
  Every mention in a signature drops one `'_` —
  `fn declare(&mut self, ctx: &mut DeclareCtx<'_, S, M>)`, and the same for a
  boxed `FnOnce(&mut DeclareCtx<'_, S, M>)` a composite stores for a
  caller-supplied body. `Ratcn::render` is unchanged at the call site.
  Declaring never paints, so the context no longer carries the ratatui `Frame`
  at all; it carries `frame_area` instead, which is all `frame_area()` ever
  answered from. `frame_area()` and `state()` are `const fn` now, and `Debug`
  drops `declaration_active` (nothing left to report) and gains `frame_area`.
- **Breaking:** `DeclareCtx::state`, `paint`, `defer_paint`, `scope`,
  `component`, `modal`, `modal_scope`, `hint`, and `popup` no longer panic
  "outside a `Ratcn` declaration pass". The context is only ever constructed
  inside one — the pass and the state are plain references rather than
  `Option`s — so there is no passless context left to guard against, and the
  five panic messages that named that state are gone. The duplicate-id and
  interaction-area panics are unaffected. Two methods that answered for a
  passless context now answer for a pass: `DeclareCtx::pointer_within` is never
  `false` merely for want of one, and `transient` / `transient_mut` still answer
  `None` only for the reason that remains — no event handler has stored a value
  at this path.
- **Breaking:** `Tab<T>` is a type alias for `ListItem<T>`. A tab and a list row
  carried the same three things — a value, a label, a disabled flag — behind two
  identical structs. `Tab::new`, `.disabled(true)`, `value()`, `label()`,
  `is_disabled()`, and the `&str`/`String` conversions all read as before, and
  `Tabs` still takes `Tab`s. What changes: the two are now one type, so a trait
  implemented for both no longer compiles; `Debug` on a `Tab` names it
  `ListItem { value: .., label: .., disabled: .. }`; and the docs.rs page for it
  is `type.Tab.html` rather than `struct.Tab.html`.
- **Breaking:** the `List`, `Select`, and `Tabs` rustdoc no longer describes a
  single-character typeahead. None of the three ever implemented one — a plain
  letter has always bubbled as an app hotkey, which is what
  [Keyboard](https://ratcn.kristoferlund.se/docs/concepts/keyboard) says under
  "What is not here". Only the promise is gone; no behavior changed. The `Tabs`
  test that was named for the absent feature is renamed for what it checks.
- **Breaking:** `ListWidget` takes the rows on screen rather than the whole
  list. `ListWidget::scroll_offset` is replaced by `ListWidget::first_item`,
  the index `items[0]` has in the whole list; `focused_item`, `selected_items`,
  and `disabled_items` still count from the start of the list, so scrolling
  changes only that number and the rows. `new(&items).scroll_offset(n)` becomes
  `new(&items[n.min(items.len())..]).first_item(n)` — the clamp matters, because
  the old call painted an empty list for an offset past the end where slicing
  panics. The widget holds no scroll position, and a long list costs a long
  list's worth of `Text` only if the caller builds one.
- **Breaking:** `SelectWidget::option_rows` is `visible_item_rows` and takes one
  row per *painted* option, in paint order, starting at `first_item`. `open`
  still takes every option, because the panel's height is measured from their
  count.
- **Breaking:** `List`'s `Component` implementation requires `T: 'static`.
  `DeclareCtx::component` already required it of any `List` declared
  through the runtime, so a declared list is unaffected; a `List` driven
  directly through the `Component` trait — `prepare`/`handle_event` against an
  `EventCtx::default()`, which is how the transient docs suggest testing one —
  now needs an owned item type too. The hold below keeps an item value in the
  transient store, and while a transient is keyed by its identity *path*, the
  type stored at that path is asserted on read, so the value's type is part of
  what reader and writer must agree on.
- **Breaking:** `WheelHold` is generic over the item value, and the wheel's
  hold persists only while the list has not moved under it: the
  anchored item is still at its anchored row, in a list of the same length,
  with the cursor still on it. Replacing that item, reordering, filtering,
  inserting, removing, or moving the cursor releases the hold and scrolls the
  cursor back into view. Before, a hold anchored by row index survived an item
  being swapped for another at the same position and left the new one
  off-screen.
  - `settle`, `record`, `cursor_to_show`, and `offset` collapse into one
    `settle(items, cursor, requested, viewport, area)` that releases the hold,
    computes the offset, and records it in the `RowViewport`.
  - `hold(offset, items, cursor)` replaces `park(offset, cursor)` and captures
    the whole anchor itself, so no caller can assemble half of one. It is no
    longer `const`, and its impl block requires `T: Clone` to keep the value.
  - The type is no longer `Copy`, and `Debug`, `Clone`, `PartialEq`, and `Eq`
    hold only when the item type does. `Default` is unconditional.
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
- **Breaking:** `Component::declare` is declaration only. It lays the component
  out, declares its descendants, and records what `handle_event` reads back;
  it writes no cells and is not offered the interaction flags. Everything that
  draws moves to `Component::paint`.
- **Breaking:** `Dialog` and `Tooltip` hold their bodies, footer, and actions in
  private types. Their builders are unchanged.
- Fills shift by the background's polarity instead of a hardcoded direction.
  The seven presets render identically; light backgrounds get the mirror image.
- Every theme preset is retuned. Wells, dialogs, and text are re-derived from
  each palette's official tones into one consistent model — `background` <
  `surface` < `field`, with the focus, hover, and cursor fills stepping up from
  there — and every painted text pair is held to contrast floors by permanent
  tests. The tone-by-tone mapping and each judgment call are documented on the
  presets themselves in `theme.rs`.
- Each preset's `destructive_foreground` sits on whichever side of its own red
  keeps a delete button's label legible as the fill darkens under focus and the
  pointer, and destructive buttons join the presets' contrast tests. Default and
  Gruvbox move to darker labels, Nord to a lighter one. Two palettes ship a red
  no tone of theirs can label at 4.5:1 from either side, so Nord and Gruvbox
  settle for a documented 3.5:1 on that one pair.
- `Theme::terminal`'s `primary` is neutral white rather than `LightBlue`: a
  named accent's derived focus and hover states resolved through a fixed VGA
  table into pure blue regardless of the terminal's palette; a neutral one
  adapts.
- `List` and a `Select` panel build only the rows they paint. A thousand-item
  list showing fifteen rows builds fifteen, `paint_item` still receives each
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
  must cover a composite's descendants belongs in `DeclareCtx::defer_paint`. The
  kanban demo's drag ghost was already written that way; nothing in-tree had to
  move.
- A rejected pass paints nothing. Declaration, validation, and the modal-stack
  check all complete before the first cell is written, so a poisoned or
  modal-mismatched pass leaves the previous frame on screen as well as the
  previous surface.
- The declaration closure and every `Component::declare` now run once per
  frame rather than twice. Anything a declaration does beyond declaring — a
  counter, a log, an `Rc<RefCell>` write — happens half as often, and
  `DeclareCtx::transient_mut` writes once rather than twice.
- `DeclareCtx::in_area` is documented rather than hidden: it is how a composite
  hands a caller-supplied body the area it laid out for it.
- A `Dialog` action is prepared where it is declared rather than in the dialog's
  own `prepare`. Preparation reads only the declaring state, which cannot change
  within a frame.
- Rendering one retained composite instance twice by hand no longer panics; the
  second render declares nothing. Every frame builds a fresh instance, so this
  was never reachable through `Ratcn::render`.
- `linear_nav::nav_key_target` returns `None` for every movement when no index in
  `0..len` is enabled, as its contract already said. Given a `Some(cursor)`, a
  step or a page used to answer `NavOutcome::Stay` — the answer that tells a
  control to consume the key — while the Home and End edges declined. `List` no
  longer needs its own all-disabled check before asking.
- `BarChartWidget::width` and `height` measure what a grouped chart paints. Two
  errors met at the group boundary:
  - A boundary costs a `bar_gap` as well as a `group_gap`. Ratatui advances by
    `bar_gap + bar_width` after every bar and only then adds the `group_gap` —
    which is what `group_gap` being extra space *on top of* the bar gap has
    always meant — while the measurement counted the group gap alone. Every
    multi-group chart with a bar gap, and the default bar gap is 1, measured
    `bar_gap` per boundary short of what it painted, so a layout that trusted the
    measurement clipped the last bar. The docs' own grouped example was one.
  - A group with no bars occupies no space. Ratatui drops empty groups before
    painting, so a chart holding one measured a `group_gap` too wide, wherever
    the empty group sat.
  The two errors partly cancelled, which is why some grouped charts measured
  correctly by accident.
- `BarChartWidget` holds one list of groups, and its derived traits follow:
  `new([bar])` equals `grouped([BarChartGroup::new([bar])])`, two charts
  differing only by an empty group are equal, and `Debug` prints a `groups` list
  rather than a `Bars`/`Groups` enum.
- Cutting text to a cell width costs a walk over the text rather than a
  measurement per grapheme cluster. Every component that wraps or truncates
  goes through it — `Dialog`, `Tooltip`, `Toast`, `Button`, `Tabs` — so a dialog
  whose description wraps into a dozen rows drops from 71 µs to 51 µs per frame
  in a release build. Where a ligature spans a cluster boundary, as Arabic
  lam-alef does in ordinary prose, the cut is still decided by measuring the
  prefixes themselves, so what fits is unchanged. The list and button benches,
  which share none of this, do not move.
- A toast wraps once per frame instead of twice: the height that places it in
  the stack and the lines that fill it are one measurement rather than two that
  agree.
- A `Select` shares its options with its open panel instead of copying them
  into it. Declaring an open hundred-option select drops from 30 µs to 28 µs
  per frame; the options themselves are the caller's to build, so this is the
  smaller half of that cost.

### Fixed

- Outline and Ghost buttons painted `theme.background` under their label,
  a dark stripe on any surface that is not the background, and a resting
  Ghost button grew caps it was documented not to have.
- A popup or hint declared after a modal, outside it, composited above the
  modal undimmed while every press on it was consumed.
- A press with no motion before it routed to the pressed node while hover
  stayed on the previous one, and the gesture froze it there.
- A drag transient outlived a gesture the runtime abandoned (pointer left
  the terminal, a modal opened), so the next drag reaching the component by
  hit-test moved it and its release was swallowed.
- A duplicate modal id panics wherever the modal is declared, nested inside
  another modal included.

## [0.0.1]

First public release.

[Unreleased]: https://github.com/kristoferlund/ratcn/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/kristoferlund/ratcn/releases/tag/v0.0.1
