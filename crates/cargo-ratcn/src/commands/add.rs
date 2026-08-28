use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use syn::Item;

use crate::{
    cli::AddArgs,
    component_copy::copy_of,
    project::{
        COMPONENTS_PATH, conventional_crate_root, find_initialized_project, metadata,
        resolve_ratcn, root_package, validate_components_destination,
    },
};

const MODULE_TEMPLATE: &str = include_str!("../../templates/components-mod.rs");

/// The type a user imports first from each built-in component. Pinned to the
/// component inventory by a test, so a new component cannot ship without a row.
const PRIMARY_TYPES: &[(&str, &str)] = &[
    ("barchart", "BarChartWidget"),
    ("button", "Button"),
    ("checkbox", "Checkbox"),
    ("cycle", "Cycle"),
    ("dialog", "Dialog"),
    ("list", "List"),
    ("progress", "ProgressWidget"),
    ("scroll_area", "ScrollArea"),
    ("select", "Select"),
    ("tabs", "Tabs"),
    ("toast", "ToasterWidget"),
    ("tooltip", "Tooltip"),
];

pub(crate) fn execute(args: AddArgs, cwd: &Path) -> Result<()> {
    let project = find_initialized_project(cwd)?;
    validate_components_destination(&project.root)?;
    let metadata = metadata(&project.manifest_path)?;
    let entrypoint = conventional_crate_root(root_package(&metadata)?);
    let ratcn = resolve_ratcn(&metadata)?;
    let available = source_names(&ratcn.components_dir)?;

    if args.list {
        for name in available {
            println!("{name}");
        }
        return Ok(());
    }

    // Plan every change before writing any: a bad name or a collision must
    // leave the project untouched.
    validate_requested_names(&args.components, &available)?;
    let destination_dir = project.root.join(COMPONENTS_PATH);
    let mut writes = Vec::new();
    for name in &args.components {
        let source_path = ratcn.components_dir.join(format!("{name}.rs"));
        let source = fs::read_to_string(&source_path)
            .with_context(|| format!("could not read {}", source_path.display()))?;
        let copied = copy_of(&source, &source_path).map_err(anyhow::Error::msg)?;
        let destination = destination_dir.join(format!("{name}.rs"));
        if destination.exists() && !args.force {
            bail!(
                "component file already exists: {}; use --force to replace it",
                destination.display()
            );
        }
        writes.push(FileChange {
            path: destination,
            content: format!(
                "// Copied from ratcn {}: src/components/{name}.rs\n\n{copied}",
                ratcn.version
            ),
        });
    }
    let component_module = register_modules(
        &destination_dir.join("mod.rs"),
        MODULE_TEMPLATE,
        &args.components,
        "pub mod",
    )?;
    let entrypoint_module = match &entrypoint {
        Some(path) => register_entrypoint_module(path)?,
        None => None,
    };

    fs::create_dir_all(&destination_dir)
        .with_context(|| format!("could not create {}", destination_dir.display()))?;
    let registrations: Vec<_> = component_module.iter().chain(&entrypoint_module).collect();
    for change in writes.iter().chain(registrations.iter().copied()) {
        fs::write(&change.path, &change.content)
            .with_context(|| format!("could not write {}", change.path.display()))?;
    }

    for change in &writes {
        println!(
            "added {} (ratcn {})",
            relative_to_project(&project.root, &change.path),
            ratcn.version
        );
    }
    for change in registrations {
        println!(
            "registered in {}",
            relative_to_project(&project.root, &change.path)
        );
    }
    if entrypoint.is_none() {
        println!("add `mod components;` to your crate entrypoint (src/main.rs or src/lib.rs)");
    }
    println!();
    for name in &args.components {
        println!("{}", import_example(name));
    }
    println!("A copy warns as dead code until something imports it.");

    Ok(())
}

#[derive(Debug)]
struct FileChange {
    path: PathBuf,
    content: String,
}

fn source_names(components_dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(components_dir)
        .with_context(|| format!("could not read {}", components_dir.display()))?
    {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "rs") && path.is_file() {
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .with_context(|| format!("non-Unicode file name: {}", path.display()))?;
            if stem != "mod" {
                names.push(stem.to_owned());
            }
        }
    }
    names.sort_unstable();
    Ok(names)
}

fn validate_requested_names(requested: &[String], available: &[String]) -> Result<()> {
    let mut seen = HashSet::new();
    for name in requested {
        let lower = name.to_ascii_lowercase();
        if name.starts_with('@') || lower.starts_with("http://") || lower.starts_with("https://") {
            bail!("third-party registries are not supported");
        }
        if !seen.insert(name) {
            bail!("component requested more than once: {name}");
        }
        if available.binary_search(name).is_err() {
            bail!("unknown ratcn component: {name}");
        }
    }
    Ok(())
}

/// Appends `<keyword> <name>;` for every name the file does not declare yet.
/// An existing declaration in any form counts as registered. Returns `None`
/// when the file already declares everything.
fn register_modules(
    path: &Path,
    template: &str,
    names: &[String],
    keyword: &str,
) -> Result<Option<FileChange>> {
    let (mut content, existing) = if path.is_file() {
        let content = fs::read_to_string(path)
            .with_context(|| format!("could not read {}", path.display()))?;
        (content, true)
    } else {
        (template.to_owned(), false)
    };
    let declared =
        module_names(&content).with_context(|| format!("could not parse {}", path.display()))?;
    let missing: Vec<_> = names
        .iter()
        .filter(|name| !declared.contains(name))
        .collect();
    if missing.is_empty() && existing {
        return Ok(None);
    }
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    for name in missing {
        content.push_str(&format!("{keyword} {name};\n"));
    }
    Ok(Some(FileChange {
        path: path.to_path_buf(),
        content,
    }))
}

/// Puts `mod components;` first in the crate root, where module declarations
/// conventionally go, unless the file already declares it in any form.
fn register_entrypoint_module(path: &Path) -> Result<Option<FileChange>> {
    let content =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    let declared =
        module_names(&content).with_context(|| format!("could not parse {}", path.display()))?;
    if declared.iter().any(|name| name == "components") {
        return Ok(None);
    }
    Ok(Some(FileChange {
        path: path.to_path_buf(),
        content: format!("mod components;\n\n{content}"),
    }))
}

/// Top-level `mod` items, parsed rather than grepped so comments and strings
/// never count.
fn module_names(source: &str) -> Result<Vec<String>> {
    let file = syn::parse_file(source).context("could not parse Rust source")?;
    Ok(file
        .items
        .into_iter()
        .filter_map(|item| match item {
            Item::Mod(module) => Some(module.ident.to_string()),
            _ => None,
        })
        .collect())
}

fn relative_to_project(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn import_example(name: &str) -> String {
    match PRIMARY_TYPES.iter().find(|(module, _)| *module == name) {
        Some((_, type_name)) => format!("use crate::components::{name}::{type_name};"),
        None => format!("use crate::components::{name};"),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use super::{
        PRIMARY_TYPES, import_example, module_names, register_modules, source_names,
        validate_requested_names,
    };

    #[test]
    fn primary_types_cover_the_component_inventory_and_name_real_types() {
        let components_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ratcn/src/components");
        let inventory =
            source_names(&components_dir).expect("the checkout's components should list");
        let listed: Vec<&str> = PRIMARY_TYPES.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            inventory, listed,
            "PRIMARY_TYPES must list exactly the built-in components, sorted"
        );

        for (name, type_name) in PRIMARY_TYPES {
            let source = fs::read_to_string(components_dir.join(format!("{name}.rs")))
                .expect("component source should read");
            assert!(
                source.contains(&format!("pub struct {type_name}")),
                "{name}.rs must define pub struct {type_name}"
            );
        }
    }

    #[test]
    fn lists_sorted_single_file_component_names() {
        let directory = tempdir().expect("temporary directory should exist");
        fs::write(directory.path().join("tabs.rs"), "").expect("source should write");
        fs::write(directory.path().join("mod.rs"), "").expect("module should write");
        fs::write(directory.path().join("button.rs"), "").expect("source should write");
        fs::write(directory.path().join("notes.txt"), "").expect("text should write");
        fs::create_dir(directory.path().join("nested.rs")).expect("directory should create");

        assert_eq!(
            source_names(directory.path()).expect("sources should list"),
            ["button", "tabs"]
        );
    }

    #[test]
    fn rejects_registry_names_duplicates_and_unknown_components() {
        let available = vec!["button".to_owned()];

        let registry =
            validate_requested_names(&["https://example.test/button".to_owned()], &available)
                .expect_err("registry sources are not supported");
        let duplicate =
            validate_requested_names(&["button".to_owned(), "button".to_owned()], &available)
                .expect_err("a component should only be copied once");
        let unknown = validate_requested_names(&["tabs".to_owned()], &available)
            .expect_err("the source stem must exist");

        assert_eq!(
            registry.to_string(),
            "third-party registries are not supported"
        );
        assert!(duplicate.to_string().contains("more than once"));
        assert!(unknown.to_string().contains("unknown"));
    }

    #[test]
    fn registers_only_missing_modules() {
        let directory = tempdir().expect("temporary directory should exist");
        let module_path = directory.path().join("mod.rs");
        fs::write(&module_path, "pub mod button;\n").expect("module should write");

        let change = register_modules(
            &module_path,
            "",
            &["button".to_owned(), "tabs".to_owned()],
            "pub mod",
        )
        .expect("module file should parse")
        .expect("tabs should need registration");
        assert_eq!(change.content, "pub mod button;\npub mod tabs;\n");

        let unchanged = register_modules(&module_path, "", &["button".to_owned()], "pub mod")
            .expect("module file should parse");
        assert!(unchanged.is_none(), "an existing declaration is left alone");

        let fresh = register_modules(
            &directory.path().join("missing.rs"),
            "// header\n",
            &["button".to_owned()],
            "pub mod",
        )
        .expect("a missing file starts from the template")
        .expect("a missing file always needs writing");
        assert_eq!(fresh.content, "// header\npub mod button;\n");
    }

    #[test]
    fn detects_top_level_modules_without_matching_comments_or_strings() {
        let modules = module_names(
            "// mod ignored;\nconst LABEL: &str = r#\"mod ignored;\"#;\npub mod button;\n#[cfg(test)]\nmod tests {}\n",
        )
        .expect("valid module declarations should parse");

        assert_eq!(modules, ["button", "tests"]);
    }

    #[test]
    fn prints_a_primary_component_type_when_one_is_known() {
        assert_eq!(
            import_example("dialog"),
            "use crate::components::dialog::Dialog;"
        );
        assert_eq!(
            import_example("future_component"),
            "use crate::components::future_component;"
        );
    }
}
