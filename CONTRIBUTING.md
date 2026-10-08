# Contributing to ratcn

Thanks for taking an interest. This file covers what you need to know before
opening a pull request.

ratcn is below 1.0, so breaking changes are acceptable when they make the
library better. They still need agreement before implementation.

[issues]: https://github.com/kristoferlund/ratcn/issues

## An issue is required before implementation

**You must [open an issue][issues] before starting implementation and before
submitting a pull request**, unless your change qualifies for the small-change
exception below. If a relevant issue already exists, join it rather than
creating a duplicate.

For non-trivial changes, wait for a maintainer to agree on the scope and approach
before coding. This includes:

- New components, features, CLI commands, options, or demos.
- Changes to public APIs, behavior, defaults, or state ownership.
- Refactors, dependency changes, or raising the minimum Rust version.
- Build, CI, release, packaging, or docs-site infrastructure changes.

Describe the problem and who it affects, the proposed solution and compatibility
implications, and how you plan to verify it. For bugs, include reproduction
steps, the ratcn version, backend, and platform.

Opening an issue is not approval to implement it. Agreement on an approach does
not guarantee that a pull request will be merged.

### Small-change exception

You may submit a pull request without first opening an issue for:

- A small, clearly scoped bug fix that restores intended behavior without
  changing interfaces, defaults, or configuration.
- A typo, broken link, or minor documentation correction.

Explain in the pull request why the exception applies. A short diff is not
automatically a small change: features, refactors, dependency updates, and build
or packaging changes still require an issue.

If you are unsure, open an issue first. If the work grows beyond the exception,
stop and discuss it in an issue before continuing. Non-exempt pull requests that
bypass this process may be closed without review.

## Keep pull requests focused

- Address one agreed problem per pull request.
- Link the issue and explain what changed, why, and what is out of scope.
- Avoid unrelated cleanup, formatting, or refactoring.
- Follow existing code structure and conventions.
- Describe API, dependency, backend, and compatibility changes explicitly.
- Never include credentials or private data in fixtures, logs, or screenshots.

## Before you open a PR

For code changes, run these checks from the repository root. They match the
test and lint commands in CI:

```sh
cargo fmt --all -- --check
cargo test -p ratcn --locked
cargo test -p ratcn --all-features --locked
cargo fetch --locked
cargo test --workspace --exclude ratcn --locked
cargo check -p copy-fixture --examples --locked
cargo clippy -p ratcn --all-features --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --exclude cargo-ratcn --all-targets --target wasm32-unknown-unknown --locked -- -D warnings
```

`cargo fetch` supplies all-platform dependencies for the CLI's offline tests.
The wasm check excludes `cargo-ratcn`, which is a host-only Cargo subcommand.

`rust-toolchain.toml` pins the toolchain, so `rustup` will fetch the right
version on your first build. That pin exists because clippy's lints change
between Rust releases — without it your local output would not match CI's.

Three of those need explaining.

**Copyability is checked by the build.** Every component module is meant to be
copied into someone else's project, so it has to compile as an external crate
against `ratcn`'s public API alone. `crates/copy-fixture` makes that copy in its
build script — `crate::` rewritten to `ratcn::`, the test module dropped — and
compiles each component as its own example target, so a reach at a private item
or at a sibling component fails there. The explicit
`cargo check -p copy-fixture --examples --locked` above verifies those targets.
Nothing is generated into the repository and there is nothing to run by hand;
edit a component and the next build re-copies it. Adding a component means
adding `crates/copy-fixture/examples/<component>.rs`, two lines copied from its
neighbours — the build script fails with that instruction if you forget.

**`--all-targets` matters.** Over half this crate's source is test code. Without
that flag clippy skips all of it.

**The MSRV is not the pinned toolchain.** The pin is for consistent lints; the
minimum supported version is lower (see below). To check against it, override
the pin explicitly:
`cargo +1.88.0 test -p ratcn --features crossterm,termina --locked`.

### Verify the affected behavior

- Add or update automated tests for code changes. Where practical, a bug fix
  should include a regression test that fails without the fix.
- Manually exercise affected components or CLI workflows. For interaction
  changes, check keyboard and mouse input, focus, disabled states, and relevant
  edge cases in a demo; screenshots or a short recording can help reviewers.
- Report the platform, backend, and feature flags tested. A terminal build does
  not verify browser behavior, and compilation alone does not verify interaction.
- For new or changed components, verify the copy-fixture check. A component must
  work against the public API without private items or sibling components.
- For docs or demo changes, run `pnpm install --frozen-lockfile` and
  `pnpm run docs:build`. The build needs Trunk `0.21.14` and the pinned toolchain's
  `wasm32-unknown-unknown` target. Verify the affected demo in the browser too.
- For packaging changes, verify the affected crate with `cargo package`.

Report commands, results, and manual verification in the pull request. Identify
failed, skipped, or unavailable checks and explain why. Do not describe untested
behavior as verified or mark skipped checks as complete.

Documentation-only changes do not require Rust tests or lints. Review rendered
Markdown, links, and changed commands or technical claims; docs-site and demo
changes still need the site build above.

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

**Dependencies are kept to a minimum.** A PR that adds a dependency needs to
explain why existing dependencies or a simpler implementation are not enough.

**The app owns its state.** Components read state and return messages; they
never write it. If a change needs a component to hold durable state, that is
usually a sign the design should move it to the app instead. See
[State and messages](https://ratcn.com/docs/concepts/state-and-messages).

**Naming follows a fixed vocabulary.** `render` means declare *and* paint,
`paint` means write cells, `declare` means state that a component exists.
`resolve` computes an effective value. Matching the surrounding code matters
more than personal preference.

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
`pnpm run demo:dev <name>` — the name is required, and a wrong one lists the
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
pnpm install --frozen-lockfile
pnpm run docs:dev
```

## Releases

User-visible changes go under `[Unreleased]` in [`CHANGELOG.md`](CHANGELOG.md).
Update rustdoc, relevant docs pages, and CLI help when behavior changes. Version
bumps, release tags, and publishing are handled by maintainers. Security issues
have their own path — see [`SECURITY.md`](SECURITY.md).

An entry runs one to four lines and covers a change a user can see; internal
refactors and test work stay out. `**Breaking:**` entries come first within
their section, and a rename gives the old name and the new one on one line.

## Commit and PR style

Small, focused PRs review faster than large ones. If a change has a mechanical
part (a rename, a formatting sweep) and a substantive part, splitting them into
separate commits makes both easier to read.

Describe *why* in the PR body. The diff already shows what.

Complete the pull request template. Required CI checks must pass before merge,
and outstanding verification gaps must be resolved with a maintainer.
