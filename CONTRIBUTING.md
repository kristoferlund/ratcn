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
cargo clippy -p ratcn --all-features --all-targets -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --target wasm32-unknown-unknown -- -D warnings
./crates/copy-fixture/sync.sh
```

`rust-toolchain.toml` pins the toolchain, so `rustup` will fetch the right
version on your first build. That pin exists because clippy's lints change
between Rust releases — without it your local output would not match CI's.

Three of those need explaining.

**`sync.sh` is not optional.** Every component module is meant to be copied
into someone else's project, so `crates/copy-fixture` holds a generated copy of
each one with `crate::` rewritten to `ratcn::`. If you edit a component, run
the script and commit the result. CI fails if the copy is out of date.

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
| Clippy on library, workspace, and wasm | Including test code |
| Copy-fixture sync and compile | Components stay copyable |
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

**Adding a demo means two registrations.** Add it to `members` in the workspace
`Cargo.toml` *and* to the `demo:build` and `<name>:dev` scripts in
`package.json`. Miss the second and it silently never builds for the docs site.

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

## Commit and PR style

Small, focused PRs review faster than large ones. If a change has a mechanical
part (a rename, a formatting sweep) and a substantive part, splitting them into
separate commits makes both easier to read.

Describe *why* in the PR body. The diff already shows what.
