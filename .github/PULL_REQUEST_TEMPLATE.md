# What and why

<!-- What does this change, what problem does it solve, and what is out of scope?
     The diff shows the what; this is the place for the why. -->

## Issue or exception

<!-- Select ONE option. For non-exempt work, the issue and maintainer agreement
     must precede implementation, not just PR submission. -->

- [ ] I opened or joined an issue before implementation and before this PR, and a maintainer agreed on the scope and approach before I started coding.
- [ ] Small-change exception: this is a small, clearly scoped bug fix restoring intended behavior without interface/default/configuration changes, or a minor documentation correction. I explain why below.

Issue link and scope agreement, OR exception explanation:

## Verification

<!-- List commands and results, manual verification steps, and the platform,
     backend, and feature flags tested. For interaction changes, screenshots or
     a short recording can help. Remove credentials and private data. -->

### Automated checks

### Manual testing

### Failed, skipped, or unavailable checks

<!-- Include untested backends/platforms. Write "None" if there are no gaps.
     Documentation-only changes do not need Rust tests or lints; explain that
     here rather than marking checks you did not run as complete. -->

## Checks

<!-- These match CI's test and lint commands. Check only commands you ran
     successfully; explain any gaps above. -->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test -p ratcn --locked`
- [ ] `cargo test -p ratcn --all-features --locked`
- [ ] `cargo fetch --locked`
- [ ] `cargo test --workspace --exclude ratcn --locked`
- [ ] `cargo check -p copy-fixture --examples --locked`
- [ ] `cargo clippy -p ratcn --all-features --all-targets --locked -- -D warnings`
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [ ] `cargo clippy --workspace --exclude cargo-ratcn --all-targets --target wasm32-unknown-unknown --locked -- -D warnings`

## Compatibility and documentation

<!-- Describe public API, defaults, dependencies, backend support, or MSRV
     changes, including migration steps for breaking changes. Identify required
     rustdoc, docs-site, CLI help, and changelog updates. Write "None" if not
     applicable. Breaking changes are acceptable below 1.0, but require prior
     agreement and must be called out for release notes. -->

## Contribution checklist

<!-- Each checked item confirms completion or an explicit explanation above
     of why it does not apply. Do not silently mark skipped work as complete. -->

- [ ] I followed [CONTRIBUTING.md](https://github.com/kristoferlund/ratcn/blob/main/CONTRIBUTING.md) and kept this PR focused on one agreed problem, without unrelated cleanup.
- [ ] I added or updated tests, including regression coverage for bug fixes where practical, or explained why automated coverage is not practical.
- [ ] I reported automated and manual verification results and all verification gaps.
- [ ] I updated relevant documentation and CLI help, and added an Unreleased changelog entry for user-visible changes, or explained why neither is needed.
- [ ] I included no credentials or private data in code, fixtures, logs, or screenshots.

## If applicable

- [ ] **Added a component?** Added `crates/copy-fixture/examples/<component>.rs`, two lines copied from its neighbours, so the new module is compiled as a copy too. Editing a component needs nothing: the copy is made at build time.
- [ ] **Changed public API?** Updated the rustdoc, and the docs page if the behaviour is user-visible.
- [ ] **Changed docs or demos?** Ran `pnpm run docs:build` and verified the affected demo in the browser. A directory under `demos/` with a `Cargo.toml` is a workspace member, and one with a `Trunk.toml` is built for the docs site — nothing to register.
- [ ] **Added a dependency?** Explained why existing dependencies or a simpler implementation are not enough.
- [ ] **Needs a newer Rust?** Say so; the MSRV is 1.88 and raising it is a decision.
