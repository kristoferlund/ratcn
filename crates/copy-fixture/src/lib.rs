//! No code of its own: the crate exists for its build script and its examples.
//!
//! A lib target is what gives the bare `cargo check -p copy-fixture` something
//! to build, so the build script — and the inventory assert that keeps the
//! example targets in step with `crates/ratcn/src/components` — runs without
//! `--examples`. Compiling the copies themselves still needs that flag.
