# Changelog

Notable changes to `ratcn`. This project is a preview release, so the API is
still moving — breaking changes are listed first for each version.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

- **Breaking:** `Ratcn::render`'s declaration closure is `FnOnce` rather than
  `FnMut`. Values may be moved into the components it declares instead of
  cloned per run.
- **Breaking:** `Component::render` is declaration only. It lays the component
  out, declares its descendants, and records what `handle_event` reads back;
  it writes no cells and is not offered the interaction flags. Everything that
  draws moves to `Component::paint`.
- **Breaking:** `Dialog` and `Tooltip` hold their bodies, footer, and actions in
  private types. Their builders are unchanged.
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
