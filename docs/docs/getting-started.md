---
description: "Initialize a terminal app with cargo ratcn and get a focusable button on screen: the smallest complete Ratatui app using the runtime."
---

# Getting started

The quickest way to a running app is the `cargo-ratcn` CLI. It sets up a
terminal project, can write a first app for you, and copies components into your
code when you want to own them. ratcn needs Rust 1.88, or 1.90 for the browser
build.

## Initialize a terminal app

Install the CLI, create a Cargo package, and initialize it:

```sh
cargo install cargo-ratcn
cargo new my-app
cd my-app
cargo ratcn init
```

`init` adds `ratcn` and a compatible `ratatui` to the project, and offers to
write a first app for you. It never touches a `main.rs` you have already written.

## A first app

Choose **Create a demo app** during `init`, then `cargo run`. The app follows
the terminal's colors and centers a **Hello** button that pops a **World**
toast. `Ctrl+C` exits.

The generated `src/main.rs`:

<<< ../../crates/cargo-ratcn/templates/first-app.rs

Two calls do the work. `render` declares what is on screen this frame and
paints it; `handle_event` routes one input event and hands back a message when
something happened. `update` applies that message, and it is the only place the
state changes, which makes every change a plain function call you can test
without a terminal.

## Copy a component

When you want to change how a component looks or behaves beyond what its
options allow, copy its source into your project:

```sh
cargo ratcn add dialog
```

The copy lands in `src/components/`. Import
`crate::components::dialog::Dialog` instead of `ratcn::Dialog`, and the
component is yours to edit. `cargo ratcn add --list` shows what is available.
An existing copy is only replaced with `--force`, which overwrites your edits.

## Try the wizard

The wizard below is itself a ratcn app, built from buttons, a select, and a list. Press `Enter` to move through it, or `Tab` into a step to make its choice. Its source is [`demos/wizard`](https://github.com/kristoferlund/ratcn/tree/main/demos/wizard).

<div class="ratcn-preview-window" style="--ratcn-preview-height: 460px">
  <div class="ratcn-preview-chrome" aria-hidden="true">
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-dot"></span>
    <span class="ratcn-preview-url">cargo run -p wizard</span>
  </div>
  <div class="ratcn-preview-body">
    <iframe class="ratcn-component-preview-frame" src="../../../demos/wizard-demo/index.html" title="ratcn getting started demo"></iframe>
  </div>
</div>

## Other backends

`init` configures terminal apps using termina. For another backend, add `ratcn`
with the matching feature:

| Feature | For |
|---|---|
| `crossterm` | Terminal apps on a crossterm backend |
| `termina` | Terminal apps using `ratcn::terminal::Session`, which opens and restores the terminal and can follow its colors |
| `ratzilla` | Running in the browser through [Ratzilla](https://github.com/orhun/ratzilla) |
| *(none)* | Paint-only widgets, or your own backend |

```sh
cargo add ratcn --features crossterm
cargo add ratatui --no-default-features --features layout-cache,std,crossterm
```

Wiring the runtime into a custom loop, native or browser, is covered in
[Host integration](./concepts/host-integration).

## Paint-only widgets

Most interactive components paint through a plain Ratatui widget you can use on
its own:

```rust
frame.render_widget(
    ButtonWidget::new("Save").themed(&theme).focused(is_focused),
    area,
);
```

It takes a theme and the interaction states you track, such as `focused`.

`Dialog` and `ScrollArea` are the exceptions: they are composites, with no
widget of their own.

## Running the demos

Every demo runs in your terminal from a checkout of the repository:

```sh
git clone https://github.com/kristoferlund/ratcn
cd ratcn
cargo run -p ledger93
```

See [Demos](./demos) for what each one shows.

## Where to go next

- **[Demos](./demos):** run something and read its source.
- **[Components](./components/button):** what each built-in can do, with live
  previews.
- **[State and messages](./concepts/state-and-messages):** the ownership rules
  everything else builds on. The best next read if you plan to build something
  real.
- **[Themes](./concepts/themes):** presets, and writing your own palette.
