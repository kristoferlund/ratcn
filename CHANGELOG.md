# Changelog

Notable changes to `ratcn`. This project is a preview release, so the API is
still moving — breaking changes are listed first for each version.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Removed

- **Breaking:** `runtime::compose`, with `BodyFn`, `BodySlot`, and `ChildSlots`.
  A composite now holds a caller-supplied body as
  `Option<Box<dyn FnOnce(&mut RenderCtx<'_, '_, S, M>)>>` and a measured child
  as a closure that declares it, both private to the component. See the custom
  components guide; `Dialog` is the reference implementation.
- **Breaking:** `runtime::PreparedComponent` and
  `RenderCtx::render_prepared_component`. Preparing a component is internal to
  `RenderCtx::render_component` again.

### Changed

- **Breaking:** `Dialog` and `Tooltip` hold their bodies, footer, and actions in
  private types. Their builders are unchanged.
- `RenderCtx::in_area` is documented rather than hidden: it is how a composite
  hands a caller-supplied body the area it laid out for it.
- A `Dialog` action is prepared where it is declared rather than in the dialog's
  own `prepare`. Preparation reads only the declaring state, which cannot change
  within a pass.
- Rendering one retained composite instance twice by hand no longer panics; the
  second render declares nothing. Each pass builds a fresh instance, so this was
  never reachable through `Ratcn::render`.

## [0.0.1]

First public release.

[Unreleased]: https://github.com/kristoferlund/ratcn/compare/v0.0.1...HEAD
[0.0.1]: https://github.com/kristoferlund/ratcn/releases/tag/v0.0.1
