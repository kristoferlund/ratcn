//! Makes the copy a consumer of `ratcn` makes, on every build.
//!
//! Each component module is meant to be copied into someone else's project, so
//! each one has to compile as an ordinary external crate against `ratcn`'s
//! public API — no `pub(crate)` reach into the engine, no `super::` reach at a
//! sibling component. This script performs that copy (drop the test module,
//! rewrite `crate::` to `ratcn::`) into `OUT_DIR`, where one binary target per
//! component compiles it alone. Nothing generated is committed, and there is no
//! sync step to run or forget.
//!
//! Line numbers survive the copy: the transform rewrites and truncates, never
//! inserts, so a compile error at `<component>.rs:412` in `OUT_DIR` is line 412
//! of `crates/ratcn/src/components/<component>.rs`.
//!
//! The inventory is `src/bin/`: one two-line stub per component, each including
//! the module generated for the binary it is compiled as. This script fails if
//! a component has no stub or a stub has no component, so the set cannot drift
//! from `crates/ratcn/src/components/`.

use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let components_dir = manifest_dir.join("../ratcn/src/components");
    let stubs_dir = manifest_dir.join("src/bin");

    println!("cargo::rerun-if-changed={}", components_dir.display());
    println!("cargo::rerun-if-changed={}", stubs_dir.display());

    // `mod.rs` is the module's own documentation, not a component anyone copies.
    let mut components = module_names(&components_dir);
    components.remove("mod");
    let stubs = module_names(&stubs_dir);

    let unchecked = joined(components.difference(&stubs));
    assert!(
        unchecked.is_empty(),
        "no compile target for the component module(s): {unchecked}.\n\
         Every module in crates/ratcn/src/components is copied into a consumer's project, so each \
         one is compiled here in isolation. Add crates/copy-fixture/src/bin/<component>.rs with \
         the same two lines as its neighbours."
    );
    let orphaned = joined(stubs.difference(&components));
    assert!(
        orphaned.is_empty(),
        "compile target(s) with no component module in crates/ratcn/src/components: {orphaned}.\n\
         Delete the matching crates/copy-fixture/src/bin/<component>.rs."
    );

    for component in &components {
        let source_path = components_dir.join(format!("{component}.rs"));
        let source = fs::read_to_string(&source_path)
            .unwrap_or_else(|err| panic!("read {}: {err}", source_path.display()));
        let copy = copy_of(&source, &source_path);
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

/// One component module as a consumer would paste it into their own crate.
///
/// The test module goes: a copied module's tests reach the engine internals that
/// `ratcn`'s own suite covers, and this crate proves the copy compiles rather
/// than re-running behavior. Inner doc comments become ordinary comments because
/// the copy is included into the stub that compiles it, and `include!` cannot
/// carry an inner attribute.
fn copy_of(source: &str, source_path: &Path) -> String {
    let mut lines: Vec<&str> = source.lines().collect();
    if let Some(tests) = lines.iter().position(|line| line.starts_with("mod tests")) {
        assert!(
            tests > 0 && lines[tests - 1] == "#[cfg(test)]",
            "{}: the tests module is not preceded by #[cfg(test)]",
            source_path.display()
        );
        lines.truncate(tests - 1);
    }

    let mut copy = String::with_capacity(source.len());
    for line in lines {
        assert!(
            !line.starts_with("#!["),
            "{}: an inner attribute cannot survive the copy — this crate includes each \
             generated module into the target that compiles it",
            source_path.display()
        );
        let (prefix, body) = match line.strip_prefix("//!") {
            Some(rest) => ("//", rest),
            None => ("", line),
        };
        copy.push_str(prefix);
        copy.push_str(&rewrite_crate_paths(body));
        copy.push('\n');
    }
    copy
}

/// Point `crate::` paths at the published crate, leaving identifiers that merely
/// end in `crate` (`some_crate::`) alone.
fn rewrite_crate_paths(line: &str) -> String {
    const NEEDLE: &str = "crate::";

    let mut rewritten = String::with_capacity(line.len());
    let mut cursor = 0;
    while let Some(offset) = line[cursor..].find(NEEDLE) {
        let start = cursor + offset;
        rewritten.push_str(&line[cursor..start]);
        let inside_identifier = line[..start]
            .chars()
            .next_back()
            .is_some_and(|character| character.is_alphanumeric() || character == '_');
        rewritten.push_str(if inside_identifier { NEEDLE } else { "ratcn::" });
        cursor = start + NEEDLE.len();
    }
    rewritten.push_str(&line[cursor..]);
    rewritten
}
