# Getting started

ratcn gives Ratatui apps good-looking, themed components without taking over
the application. Use them directly from the library, or copy their source
when you want to make them your own.

You get coherent defaults for appearance and interaction. You keep your app
loop, layouts, architecture, and application state.

**Preview release.** The API is unstable: pin an exact version and expect to
edit code when you upgrade. Requires Rust 1.88.

## Start with the defaults

The `cargo-ratcn` CLI sets up a terminal project:

```sh
cargo install cargo-ratcn
cargo new my-app
cd my-app
cargo ratcn init
```

`init` adds ratcn with its `termina` feature, a compatible Ratatui,
`ratcn.toml`, and `src/components/mod.rs`. In an interactive terminal, over
Cargo's untouched default `main.rs`, it also offers a starter: keep it,
create a minimal app, or create a demo app. Source you wrote is never
replaced.

Choose the demo app and run `cargo run` to see a button and a Hello World
toast. No component copying is required.

## Your state, your update logic

Manage application state however you prefer. Supply that state when you
render, and describe which components belong on screen:

```rust
ratcn.render(frame, area, &state, &theme, |ctx| {
    ctx.component(
        "hello",
        Button::new("Hello").on_press(|| Msg::Hello),
        area,
    );
});
```

Components read your state; they do not change it. Interactions produce
messages that your application decides how to handle:

```rust
match ratcn.handle_event(event, &state) {
    EventResult::Emit(msg) => state.update(msg),
    EventResult::Consumed => {}
    EventResult::Ignored => {}
}
```

Your update function remains the writer. An ignored event stays available
for your own shortcuts. ratcn keeps the interaction bookkeeping it needs
between frames, but never mutates your application state.

Already have focus and event handling? Most interactive components also
have a paint-only Ratatui widget, such as `ButtonWidget` or `ListWidget`,
that you can use without the runtime.

## Make a component your own

Use components directly for as long as their defaults suit your app. When
you want to change an implementation, copy its source into your project:

```sh
cargo ratcn add --list
cargo ratcn add dialog
```

`add` copies the component module from the exact ratcn package your project
resolved and registers it. Existing files are preserved unless you pass
`--force`.

Import your copy with `crate::components::dialog::Dialog`, then change its
appearance or behavior to fit your application. The source is yours to
modify, not a black box you have to work around.

Copied components still use ratcn's runtime and public helpers, plus
Ratatui. You own the component implementation without having to rebuild
the infrastructure underneath it.

## Explore the examples

**Demos**, in the header, runs every demo in the repository, from individual
components to larger applications with multiple views.

More documentation is available at
[ratcn.com](https://ratcn.com), including
Crossterm and browser builds through ratzilla.
