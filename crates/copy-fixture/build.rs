//! Makes the copy a consumer of `ratcn` makes, on every build.
//!
//! Each component module is meant to be copied into someone else's project, so
//! each one has to compile as an ordinary external crate against `ratcn`'s
//! public API — no `pub(crate)` reach into the engine, no `super::` reach at a
//! sibling component. This script performs that copy (drop the test module,
//! rewrite `crate::` to `ratcn::`) into `OUT_DIR`, where one example target
//! per component compiles it alone. Nothing generated is committed, and there
//! is no sync step to run or forget.
//!
//! Line numbers survive the copy: the transform rewrites and truncates, never
//! inserts, so a compile error at `<component>.rs:412` in `OUT_DIR` is line 412
//! of `crates/ratcn/src/components/<component>.rs`.
//!
//! The inventory is `examples/`: one stub per component, each including the
//! module generated for the target it is compiled as. This script fails if a
//! component has no stub, if a stub has no component, or if a stub is not
//! [`STUB`] byte for byte — a stub that had been emptied to `fn main() {}`
//! would pass the inventory while its component went unchecked. So the set
//! cannot drift from `crates/ratcn/src/components/`, and nor can what a target
//! actually compiles.

use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

#[path = "../cargo-ratcn/src/component_copy.rs"]
mod component_copy;

/// Every stub in `examples`, byte for byte. `CARGO_BIN_NAME` is what makes one
/// text serve all of them: cargo sets it to the target being compiled, which is
/// the component whose copy that target includes.
const STUB: &str = r#"// One component module, copied at build time and compiled alone. See build.rs.
include!(concat!(env!("OUT_DIR"), "/", env!("CARGO_BIN_NAME"), ".rs"));

fn main() {}
"#;

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let components_dir = manifest_dir.join("../ratcn/src/components");
    let stubs_dir = manifest_dir.join("examples");

    println!("cargo::rerun-if-changed={}", components_dir.display());
    println!("cargo::rerun-if-changed={}", stubs_dir.display());

    // `mod.rs` declares the modules, and is not a component anyone copies.
    let mut components = module_names(&components_dir);
    components.remove("mod");
    let stubs = module_names(&stubs_dir);

    let unchecked = joined(components.difference(&stubs));
    assert!(
        unchecked.is_empty(),
        "no compile target for the component module(s): {unchecked}.\n\
         Every module in crates/ratcn/src/components is copied into a consumer's project, so each \
         one is compiled here in isolation. Add crates/copy-fixture/examples/<component>.rs with \
         the same two lines as its neighbours."
    );
    let orphaned = joined(stubs.difference(&components));
    assert!(
        orphaned.is_empty(),
        "compile target(s) with no component module in crates/ratcn/src/components: {orphaned}.\n\
         Delete the matching crates/copy-fixture/examples/<component>.rs."
    );

    for component in &components {
        let stub_path = stubs_dir.join(format!("{component}.rs"));
        let stub = fs::read_to_string(&stub_path)
            .unwrap_or_else(|err| panic!("read {}: {err}", stub_path.display()));
        assert!(
            stub == STUB,
            "{} does not compile the component's copy. A target here is only a gate while it \
             includes the generated module, so replace the file with exactly:\n\n{STUB}",
            stub_path.display()
        );

        let source_path = components_dir.join(format!("{component}.rs"));
        let source = fs::read_to_string(&source_path)
            .unwrap_or_else(|err| panic!("read {}: {err}", source_path.display()));
        let copy = component_copy::copy_of(&source, &source_path)
            .unwrap_or_else(|message| panic!("{message}"));
        let copy_path = out_dir.join(format!("{component}.rs"));
        fs::write(&copy_path, copy)
            .unwrap_or_else(|err| panic!("write {}: {err}", copy_path.display()));
    }
}

/// Names for a failure message, comma-separated, or empty when nothing drifted.
fn joined<'a>(names: impl Iterator<Item = &'a String>) -> String {
    names.cloned().collect::<Vec<_>>().join(", ")
}

/// The `.rs` module names directly inside `dir`, which must exist.
///
/// Single-file modules only: a component written as a directory —
/// `components/widget/mod.rs` — is invisible here, so it would be copied by
/// nothing and the inventory would not notice. Every component is one file today,
/// which is also what "copy the module into your project" means; the day one is
/// not, this function is what has to learn about it.
fn module_names(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
                .path()
        })
        .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
        .map(|path| {
            path.file_stem()
                .expect("a .rs path has a file stem")
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}
