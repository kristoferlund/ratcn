//! Compile-time guard for the shadcn-style copy workflow: generated copies of
//! every component module, built as an ordinary external crate against
//! `ratcn`'s public API only.
//!
//! `sync.sh` removes test modules, changes `crate::` to `ratcn::`, and applies
//! the workspace formatter. No `pub(crate)` import or `super::` sibling
//! dependency is introduced. If a future change requires either, this crate
//! stops compiling.
//!
//! Test modules are not copied here (a copied module's own tests would need
//! `RenderCtx`'s `pub(crate)` engine-internal fields); `ratcn`'s test suite
//! already covers the behavior. This crate only proves the copy compiles.

mod barchart;
mod button;
mod dialog;
mod list;
mod select;
mod tabs;
mod toaster;
mod tooltip;
