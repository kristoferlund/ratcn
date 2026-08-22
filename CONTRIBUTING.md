# Contributing to ratcn

Thanks for taking an interest. This file covers what you need to know before
opening a pull request.

ratcn is a **preview release**, so the API is still moving. That makes some
kinds of contribution easier than usual (breaking changes are acceptable) and
others harder (a large feature may collide with work already in progress). If
you are planning something substantial, [open an issue][issues] first so we can
agree on the shape before you write it.

[issues]: https://github.com/kristoferlund/ratcn/issues

## Before you open a PR

Run the same checks CI runs:

```sh
cargo fmt --all
cargo test -p ratcn --all-features
cargo test --workspace
cargo clippy -p ratcn --all-features --all-targets -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target wasm32-unknown-unknown -- -D warnings
```

`rust-toolchain.toml` pins the toolchain, so `rustup` will fetch the right
version on your first build. That pin exists because clippy's lints change
between Rust releases — without it your local output would not match CI's.

Three of those need explaining.

**Copyability is checked by the build.** Every component module is meant to be
copied into someone else's project, so it has to compile as an external crate
against `ratcn`'s public API alone. `crates/copy-fixture` makes that copy in its
build script — `crate::` rewritten to `ratcn::`, the test module dropped — and
compiles each component as its own example target, so a reach at a private item
or at a sibling component fails there. `cargo test --workspace` above builds
those targets, as does `cargo check -p copy-fixture --examples` on its own.
Nothing is generated into the repository and there is nothing to run by hand;
edit a component and the next build re-copies it. Adding a component means
adding `crates/copy-fixture/examples/<component>.rs`, two lines copied from its
neighbours — the build script fails with that instruction if you forget.

**`--all-targets` matters.** Over half this crate's source is test code. Without
that flag clippy skips all of it.

**The MSRV is not the pinned toolchain.** The pin is for consistent lints; the
minimum supported version is lower (see below). To check against it, override
the pin explicitly: `cargo +1.88.0 test -p ratcn --features crossterm`.

## What CI checks

| Check | Why |
|---|---|
| `cargo fmt` | One formatting, no debates |
| Tests, default and all features | Both feature paths compile and behave |
| Tests, workspace | Demo tests run, rather than only compiling |
| Clippy on library, workspace, and wasm | Including test code |
| Each component compiles in isolation | Components stay copyable |
| Rustdoc with warnings denied | No broken doc links |
| `cargo package` | The crate can actually be published |
| Tests on Rust 1.88 | The MSRV stays true |
| Docs site build | All demos still compile to WebAssembly |

## Things worth knowing

**Minimum Rust version is 1.88.** The browser build needs 1.90, because of a
dependency. If your change needs something newer, say so in the PR — raising
the MSRV is a decision, not a detail.

**Dependencies are kept to a minimum.** The library has three: `ratatui` and
two small unicode crates that ratatui already requires. A PR that adds a
dependency needs to argue for it.

**The app owns its state.** Components read state and return messages; they
never write it. If a change needs a component to hold durable state, that is
usually a sign the design should move it to the app instead. See
[State and messages](https://ratcn.kristoferlund.se/docs/concepts/state-and-messages).

**Naming follows a fixed vocabulary.** `render` means declare *and* paint,
`paint` means write cells, `declare` means state that a component exists.
`resolve` computes an effective value; `prepare` asks a component its
declaration-time questions. Matching the surrounding code matters more than
personal preference.

**Component modules share one layout.** Imports, constants, variant enums, the
style struct (`from_theme()`, a `fallback()` where the component paints without
a theme, and the `resolve_*` methods that pick colors from the component's own
interaction state), the paint widget `XWidget`, closure type aliases, the
interactive component `X<S, M>`, its `Component` impl, private helpers, tests.
Modules vary where their own reading order wins, so match the shape rather than
the exact sequence.

**Component `handle_event` shares one silhouette**, with the same latitude.
`List`, `Select`, and `Tabs` open with an early guard returning `Ignored` when
the component cannot act — disabled, or no items — then match on the event kind,
mouse arm first and key arm second, falling through to `Ignored`. `Button`
matches both kinds into one `bool`, `Dialog` answers its dismiss key before the
border hit-test and then dispatches on the drag phase, and `ScrollArea` has
nothing to guard on. Only `Event::Mouse` and `Event::Key` are handled anywhere.

**A demo registers itself.** Any directory under `demos/` is a workspace member,
and one with a `Trunk.toml` is built for the docs site. Serve a single demo with
`npm run demo:dev -- <name>` — the name is required, and a wrong one lists the
demos that exist.

The `demos/*` glob is why every directory there must be a crate: a directory
without a `Cargo.toml` fails every `cargo` command in the workspace, not just the
one that touches it. Shared assets belong inside a crate — the fonts and scripts
the demos share live in `demos/shared/`.

## Docs

Component pages introduce what a component can do, with a live demo and short
snippets. Deep API detail belongs on docs.rs, which every component page links
to. Concept pages describe the intended path through a feature, not its edge
cases.

Aim for plain language. Someone a year into Rust should be able to read any page
without a glossary.

```sh
npm ci
npm run docs:dev
```

## Releases

User-visible changes go in [`CHANGELOG.md`](CHANGELOG.md). Security issues have
their own path — see [`SECURITY.md`](SECURITY.md).

An entry runs one to four lines and covers a change a user can see; internal
refactors and test work stay out. `**Breaking:**` entries come first within
their section, and a rename gives the old name and the new one on one line.

## Commit and PR style

Small, focused PRs review faster than large ones. If a change has a mechanical
part (a rename, a formatting sweep) and a substantive part, splitting them into
separate commits makes both easier to read.

Describe *why* in the PR body. The diff already shows what.
