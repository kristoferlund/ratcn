---
description: "Initialize a terminal app with cargo ratcn and get a focusable button on screen — the smallest complete Ratatui app using the runtime."
---

# Getting started

The recommended way to set up a terminal project is with the `cargo-ratcn` CLI.

## Initialize a terminal app

Install the CLI, create a Cargo package, and initialize it:

```sh
cargo install cargo-ratcn
cargo new my-app
cd my-app
cargo ratcn init
```

`init` adds `ratcn` with its `termina` feature and a compatible `ratatui`, writes
`ratcn.toml`, and creates `src/components/mod.rs`. It configures terminal apps
only.

In an interactive terminal, when `src/main.rs` is Cargo's untouched default,
`init` offers three options:

- **Keep it unchanged**
- **Create a minimal app**
- **Create a demo app**

Projects with custom application source keep it unchanged. Noninteractive runs
also keep `src/main.rs` unchanged and complete project setup without a prompt.

Use `cargo ratcn --help` for commands or `cargo ratcn add --help` for add options.
Use `cargo ratcn --version` (or `-V`) to check the installed CLI version.

## A first app

Choose **Create a demo app** during `init`, then run `cargo run` to start it.
The app follows the terminal's colors, centers a primary **Hello** button, and
shows a **World** toast when pressed. `Ctrl+C` exits.

The generated `src/main.rs`:

<<< ../../crates/cargo-ratcn/templates/first-app.rs

Two calls do the work. `render` declares what is on screen this frame and
paints it; `handle_event` routes one input event and hands back a message if
something happened. The generated loop opens and restores the terminal through
`Session`; its `update` function remains the only state writer.

Keeping `update` in its own function means every state change is a plain call
you can test without a terminal, and messages from elsewhere (a timer, a
background task) get the same single path into state.

## Copy a component

From your initialized project, list the available components and copy one:

```sh
cargo ratcn add --list
cargo ratcn add dialog
```

`add` copies source from the exact `ratcn` package your project resolved into
`src/components/` and registers the component module. It adds `mod components;`
when there is a single conventional crate entrypoint; otherwise, it asks you to
add that declaration yourself. Import `crate::components::dialog::Dialog` to use
your copy.

Existing component files are preserved unless you pass `--force`.
**`cargo ratcn add dialog --force` overwrites `src/components/dialog.rs`, including
your edits.**

## Try the wizard

The wizard below is itself a ratcn app — buttons, a select, and a list. Press `Enter` to move through it, or `Tab` into a step to make its choice. Its source is [`demos/wizard`](https://github.com/kristoferlund/ratcn/tree/main/demos/wizard).

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
its own — `Dialog` and `ScrollArea` are composites and have no widget half:

```rust
frame.render_widget(
    ButtonWidget::new("Save").themed(&theme).focused(is_focused),
    area,
);
```

It takes a theme and a couple of bools. If you already have focus and event
handling you like, keep it — and adopt the runtime later, one component at a
time, if you want to.

## Running the demos

Every demo runs in your terminal from a checkout of the repository:

```sh
git clone https://github.com/kristoferlund/ratcn
cd ratcn
cargo run -p ledger93
```

See [Demos](./demos) for what each one shows.

## Where to go next

- **[Demos](./demos)** — run something and read its source.
- **[Components](./components/button)** — what each built-in can do, with live
  previews.
- **[State and messages](./concepts/state-and-messages)** — the ownership rules
  everything else builds on. The best next read if you plan to build something
  real.
- **[Themes](./concepts/themes)** — presets, and writing your own palette.
