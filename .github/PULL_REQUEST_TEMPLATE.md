# What and why

<!-- What does this change, and what problem does it solve? The diff shows the
     what; this is the place for the why. Link an issue if there is one. -->

## Checks

<!-- CI runs all of these. Ticking them locally first saves a round trip. -->

- [ ] `cargo fmt --all`
- [ ] `cargo test -p ratcn --all-features`
- [ ] `cargo clippy -p ratcn --all-features --all-targets -- -D warnings`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo clippy --workspace --all-targets --target wasm32-unknown-unknown -- -D warnings`

## If applicable

- [ ] **Edited a component?** Ran `./crates/copy-fixture/sync.sh` and committed the result.
- [ ] **Changed public API?** Updated the rustdoc, and the docs page if the behaviour is user-visible.
- [ ] **Added a demo?** Registered it in the workspace `Cargo.toml` **and** in `package.json`.
- [ ] **Added a dependency?** Explained why below — the library keeps three.
- [ ] **Needs a newer Rust?** Say so; the MSRV is 1.88 and raising it is a decision.

<!-- Breaking changes are fine during the preview. Call them out here so they
     reach the release notes. -->
