# Getting started

ratcn is a component library for Ratatui: components you copy, theme, and own
in your source, plus a small runtime for focus, hover, and events. It never
takes over your app loop.

**Preview release.** The API is unstable: pin an exact version and expect to
edit when you upgrade. Twelve components today: Button, List, Select, Tabs,
Dialog, ScrollArea, Checkbox, Cycle, Tooltip, ToasterWidget, BarChartWidget,
and ProgressWidget.

## Install

Requires Rust 1.88. The `cargo-ratcn` CLI sets terminal projects up and copies
components into them.

```sh
cargo install cargo-ratcn
cargo new my-app
cd my-app
cargo ratcn init
```

`init` adds ratcn with its `termina` feature, a compatible Ratatui,
`ratcn.toml`, and `src/components/mod.rs`. Over Cargo's untouched default
`main.rs`, and only on a terminal, it also offers a starter: keep it, a minimal
app, or a demo app with a button and a Hello World toast. Source you wrote
yourself is never replaced.

Take the demo app, run `cargo run`, and a ratcn app is on screen.

## Copy a component

Every component module is self-contained, so you can own one:

```sh
cargo ratcn add --list
cargo ratcn add dialog
```

`add` copies the source from the exact ratcn package your project resolved,
registers the module, and never replaces a file unless you pass `--force`.
Import `crate::components::dialog::Dialog` to use your copy.

## Your app stays in charge

The runtime enters your loop at exactly two call sites. Remove them and the
rest of the loop is untouched.

```rust
ratcn.render(frame, area, &state, &theme, |ctx| {
    let button = Button::new("Hello")
        .on_press(|| Msg::Hello);
    ctx.component("hello", button, area);
});
```

```rust
match ratcn.handle_event(event, &state) {
    EventResult::Emit(msg) => state.update(msg),
    EventResult::Consumed => {}
    EventResult::Ignored => {}
}
```

`render` declares what is on screen this frame; `handle_event` answers `Emit`,
`Consumed`, or `Ignored`, and `Ignored` leaves the key to your own shortcuts.
Components read your state and never write it; `update` is the only writer.

Or skip the runtime: most components have a paint-only half. `ButtonWidget`,
`ListWidget`, and the rest are plain Ratatui widgets that take a theme and some
bools. Where a component has one, it paints through that same widget.

## Next

**Demos**, in the header, runs every demo in the repository. The rest of the
documentation is at [ratcn.kristoferlund.se](https://ratcn.kristoferlund.se),
including Crossterm and browser builds through ratzilla.
