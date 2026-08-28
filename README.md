# ratcn

A shadcn-inspired, themeable component library for [Ratatui](https://ratatui.rs),
plus a small interaction runtime for focus, hover, and event routing. It is a
toolkit, not a framework: every piece is usable piecemeal, and nothing takes
over your app loop.

## Preview status

This is a preview release. Three things are worth knowing before you build on
it:

- **The API will break.** The public surface is still moving. Pin an exact
  version and expect to edit when you upgrade.
- **The CLI is deliberately small.** `cargo ratcn init` configures terminal
  Cargo packages and can install a starter only over Cargo's untouched default
  `main.rs`; `cargo ratcn add` copies a built-in component when you want to own
  its source.
- **The component set is small and growing.** Twelve components ship today:
  `Button`, `List`, `Select`, `Tabs`, `Dialog`, `ToasterWidget`,
  `BarChartWidget`, `Tooltip`, `ScrollArea`, `Checkbox`, `Cycle`, and
  `ProgressWidget`. Notably missing and planned next are **text input** and a
  **multi-line text area**.

If you want specific components, patterns, or features, please
[open an issue](https://github.com/kristoferlund/ratcn/issues).

## What it is

A component module holds up to two cooperating halves:

- A **paint-only widget** (`ButtonWidget`, `ListWidget`, `BarChartWidget`, ...)
  is a plain ratatui `Widget` that only paints. It is usable on its own without
  the runtime. BarChart and Toast are paint-only and stop here.
- An **interactive component** (`Button`, `List`, `Select`, ...) — declared
  through the runtime each frame, it handles focus, events, and messages. Most
  paint via a widget half. Dialog and ScrollArea are the exceptions:
  interactive composites with no separate paint widget.

Your app owns state, events, and updates. The library enters your loop at
exactly two removable call sites: `Ratcn::render` and `Ratcn::handle_event`.
Components read state and return messages; your `update` function is the only
state writer.

## Getting started

Requires Rust 1.88 (1.90 for the browser build). Install the Cargo subcommand,
then initialize an existing package:

```sh
cargo install cargo-ratcn
cargo new my-app
cd my-app
cargo ratcn init
```

`init` adds terminal dependencies, writes `ratcn.toml`, and creates
`src/components/mod.rs`. On Cargo's default `src/main.rs`, it offers to leave
the file alone, install a minimal app loop, or install the Getting started demo.
Custom and non-interactive projects retain their application source.

For a native crossterm app that already owns its event loop:

```sh
cargo add ratcn --features crossterm
cargo add ratatui --no-default-features --features layout-cache,std,crossterm
```

For a browser app, select the ratzilla integration instead:

```sh
cargo add ratcn --features ratzilla
cargo add ratatui --no-default-features --features layout-cache,std
cargo add ratzilla
```

The crate also ships a terminal host of its own: `ratcn::terminal::Session`
(feature `termina`) opens the terminal, asks it for its background and
foreground, solves a `Theme` from the pair with `Theme::adaptive`, and
re-solves when the user changes it. Use it in place of crossterm when you want
the app to paint in the terminal's own colors.

## Copying a component

Each component module is written as one self-contained unit, so you can copy
the module into your project and modify it there when the built-in styling and
behavior hooks are not enough:

```sh
cargo ratcn add dialog
```

`cargo ratcn add --list` shows the built-ins available from the exact `ratcn`
package your project resolved. The command adds the component file and module
declarations; switch the app import to `crate::components::dialog::Dialog` to
use the copy.

A copied module still depends on:

- the `ratcn` runtime — the `Component` trait, `DeclareCtx`/`EventCtx`,
  `EventResult`, and the normalized event types;
- the theme types (`Theme`, and `BorderStyle` where a border is painted), plus
  the copy-support modules: `button_shape`, `geometry`, `linear_nav`,
  `list_core`, `selection_indicator`, and `text_width`;
- `ratatui` itself.

Components never depend on sibling components, so each module copies alone.
The `copy-fixture` crate in this repository makes that copy at build time and
compiles each component on its own, so the claim is checked by the build rather
than asserted.

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
