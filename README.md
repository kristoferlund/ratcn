<h1 align="center">ratcn</h1>

<p align="center">
  <strong>Themeable terminal UI components for Ratatui. Copy, customize, and own them.</strong>
</p>

<p align="center">
  <a href="https://crates.io/crates/ratcn"><img src="https://img.shields.io/crates/v/ratcn?style=flat" alt="crates.io version"></a>
  <a href="https://github.com/kristoferlund/ratcn/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/kristoferlund/ratcn/ci.yml?branch=main&style=flat&label=CI" alt="CI status"></a>
  <a href="https://github.com/kristoferlund/ratcn/stargazers"><img src="https://img.shields.io/github/stars/kristoferlund/ratcn?style=flat" alt="GitHub stars"></a>
  <a href="https://github.com/kristoferlund/ratcn/blob/main/LICENSE"><img src="https://img.shields.io/github/license/kristoferlund/ratcn?style=flat" alt="MIT license"></a>
</p>

<p align="center">
  <a href="#try-the-showcase">Try the Showcase</a> &middot;
  <a href="#getting-started">Getting Started</a> &middot;
  <a href="https://ratcn.com">Website</a> &middot;
  <a href="https://ratcn.com/docs/introduction">Docs</a>
</p>

<div align="center">
  <video src="https://github.com/user-attachments/assets/bc3e4b4d-b27e-47ee-8e0e-f0393962920c" controls width="800">
    <a href="https://github.com/user-attachments/assets/bc3e4b4d-b27e-47ee-8e0e-f0393962920c">Watch the ratcn showcase video.</a>
  </video>
</div>

ratcn is an opinionated library of beautifully designed terminal UI components
for [Ratatui](https://ratatui.rs). You copy them into your application code,
theme them, and make them your own.

Every component paints through a plain Ratatui widget, so it fits into the app
you already have. Add the ratcn engine and the components come alive: it handles
focus, keyboard and mouse input, hover, and layers, and tells your app what the
user did.

Your app stays in charge. The state lives in your app, and the screen is a
function of it: each frame describes the UI from that state, and components
report what happened as messages for your app to apply. The components are yours
as well. When one doesn't do what you need, copy its source into your project
and change it.

Terminal apps can be as pleasant to use as anything on the web. Buttons light up
under the pointer, fields take clicks and selections, and the patterns you know
from the web are here too: toasts, tooltips, dropdowns, and modal dialogs that
float above the rest of your app.

## Try the showcase

See what ratcn feels like in your own terminal. Open the live showcase with one
command, no Rust installation or account needed:

```sh
ssh ratcn.com
```

Browse interactive component demos, switch themes, drag cards across a Kanban
board, and explore full demo apps. These are working Ratatui interfaces, not
recordings. Try the controls with your keyboard and mouse, then
[explore the demos and their source](https://ratcn.com/docs/demos).

## Widgets, the engine, or both

Each component comes in up to two parts, and each part works on its own:

- **Paint-only widgets**, such as `ButtonWidget` and `BarChartWidget`, are
  ordinary Ratatui widgets. They drop into any Ratatui app with
  `frame.render_widget(...)`: no engine, no message type, no change to how your
  app already works.
- **Interactive components**, such as `Button`, `List`, `Tabs`, and `Dialog`,
  add focus, keyboard and mouse handling, and messages on top. You declare them
  through the engine, the `Ratcn` runtime, which enters your app at two calls:
  `Ratcn::render` and `Ratcn::handle_event`.

The components: `Button`, `Input`, `TextArea`, `List`, `ScrollArea`, `Select`,
`Tabs`, `Dialog`, `ToasterWidget`, `BarChartWidget`, `Tooltip`, `Checkbox`,
`Cycle`, and `ProgressWidget`. If there is a component or pattern you would like to see, please
[open an issue](https://github.com/kristoferlund/ratcn/issues).

## Getting started

ratcn needs Rust 1.88, or 1.90 for the browser build. The quickest way to a
running app is the `cargo-ratcn` CLI. Install it, create a Cargo package, and
initialize it:

```sh
cargo install cargo-ratcn
cargo new my-app
cd my-app
cargo ratcn init
```

`init` adds `ratcn` and a compatible `ratatui` to the project, and offers to
write a first app for you. Choose **Create a demo app**, then `cargo run`: a
button that pops a toast. The
[Getting started](https://ratcn.com/docs/getting-started) guide walks through
its source.

The `termina` feature's `ratcn::terminal::Session` opens and restores the
terminal, and can paint in the terminal's own colors, following them when the
user changes them.

For an app on a crossterm backend that already owns its event loop:

```sh
cargo add ratcn --features crossterm
cargo add ratatui --no-default-features --features layout-cache,std,crossterm
```

For an app in the browser, through [Ratzilla](https://github.com/orhun/ratzilla):

```sh
cargo add ratcn --features ratzilla
cargo add ratatui --no-default-features --features layout-cache,std
cargo add ratzilla
```

## Copying a component

When you want to change how a component looks or behaves beyond what its options
allow, copy its source into your project:

```sh
cargo ratcn add dialog
```

The copy lands in `src/components/`. Import `crate::components::dialog::Dialog`
instead of `ratcn::Dialog`, and the component is yours to edit. Components never
depend on each other, so each one copies on its own; the `copy-fixture` crate in
this repository builds every component from a copy to keep it that way.

## Documentation

The [documentation site](https://ratcn.com), [documentation
source](https://github.com/kristoferlund/ratcn/tree/main/docs), and [repository
source](https://github.com/kristoferlund/ratcn) cover the concepts, components,
and live WebAssembly previews. The demo crates under `demos/` are the canonical
integration examples.

To build the site from a checkout, use the pinned toolchain, install Trunk
`0.21.14`, run `pnpm install --frozen-lockfile`, then run `pnpm run docs:build`.
The pinned toolchain installs the `wasm32-unknown-unknown` target used by the demos.
Publishing this source does not deploy the hosted site; deployment remains a
separate release step, so the currently hosted content may lag the repository.

## License

MIT
