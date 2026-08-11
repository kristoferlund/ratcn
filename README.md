# ratcn

A shadcn-inspired, themeable component library for [Ratatui](https://ratatui.rs),
plus a small interaction runtime for focus, hover, and event routing. It is a
toolkit, not a framework: every piece is usable piecemeal, and nothing takes
over your app loop.

## Preview status

This is a preview release. Three things are worth knowing before you build on
it:

- **The API will break.** The public surface is still moving — recent releases
  have renamed methods, changed signatures, and removed components outright.
  Pin an exact version and expect to edit when you upgrade.
- **There is no install command.** The shadcn resemblance is in how the code is
  structured, not yet in tooling. Copying a component into your project is a
  manual file copy today. A CLI is intended, but it does not exist.
- **The component set is small and growing.** Eight components ship today:
  `Button`, `List`, `Select`, `Tabs`, `Dialog`, `Toast`, `BarChart`, and
  `Tooltip`. Notably missing and planned next are **text input**, **multi-line
  text area**, and a **scroll area** for content taller than its viewport.
  Input and TextArea existed in an earlier preview and were withdrawn pending
  upstream fixes in the text-editing crate they wrap.

If you want specific components, patterns, or features, please
[open an issue](https://github.com/kristoferlund/ratcn/issues).

## What it is

A component module holds up to two cooperating halves:

- A **paint-only widget** (`ButtonWidget`, `ListWidget`, `BarChartWidget`, ...)
  is a plain ratatui `Widget` that just draws. It is usable on its own without
  the runtime. BarChart and Toast are paint-only and stop here.
- An **interactive component** (`Button`, `List`, `Select`, ...) — declared
  through the runtime each frame, it handles focus, events, and messages. Most
  paint via a widget half. Dialog is the exception: it is an interactive
  composite with no separate paint widget.

Your app owns state, events, and updates. The library enters your loop at
exactly two removable call sites: `Ratcn::render` and `Ratcn::handle_event`.
Components read state and return messages; your `update` function is the only
state writer.

## Getting started

Requires Rust 1.88 (1.90 for the browser build). For a native crossterm app:

```sh
cargo add ratcn --features crossterm
cargo add ratatui --no-default-features --features layout-cache,crossterm
```

For a browser app, select the ratzilla integration instead:

```sh
cargo add ratcn --features ratzilla
cargo add ratatui --no-default-features --features layout-cache
cargo add ratzilla
```

## Copying a component

Each component module is written as one self-contained unit, so you can copy
the module into your project and modify it there when the built-in styling and
behavior hooks are not enough. As noted above, this is a manual file copy
today; there is no registry or install command.

A copied module still depends on:

- the `ratcn` runtime — the `Component` trait, `RenderCtx`/`EventCtx`,
  `EventResult`, and the normalized event types;
- the theme types (`Theme`, and `BorderStyle` where a border is drawn), plus
  the crate's small helper modules the component uses (such as color math, text
  width, linear navigation, and the toast state module);
- `ratatui` itself.

Components never depend on sibling components, so each module copies alone.
The `copy-fixture` crate in this repository compiles each component as a copied
module, so the claim is checked by the build rather than asserted.

## Documentation

The [documentation site](https://ratcn.kristoferlund.se), [documentation
source](https://github.com/kristoferlund/ratcn/tree/main/docs), and [repository
source](https://github.com/kristoferlund/ratcn) cover the concepts, components,
and live WebAssembly previews. The demo crates under `demos/` are the canonical
integration examples.

To build the site from a checkout, use the pinned toolchain, install Trunk
`0.21.14`, run `npm ci`, then run `npm run docs:build`. The pinned toolchain
installs the `wasm32-unknown-unknown` target used by the demos.
Publishing this source does not deploy the hosted site; deployment remains a
separate release step, so the currently hosted content may lag the repository.

## License

MIT
