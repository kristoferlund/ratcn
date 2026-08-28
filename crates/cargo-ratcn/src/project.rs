use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use cargo_metadata::{DependencyKind, Metadata, MetadataCommand, Node, Package, PackageId};
use toml::Value;

pub(crate) const COMPONENTS_PATH: &str = "src/components";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RatcnConfig {
    pub(crate) version: String,
}

#[derive(Debug)]
pub(crate) struct Project {
    pub(crate) root: PathBuf,
    pub(crate) manifest_path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct ResolvedRatcn {
    pub(crate) version: String,
    pub(crate) components_dir: PathBuf,
}

/// Finds the closest package manifest, ignoring virtual workspace manifests.
pub(crate) fn find_package_manifest(cwd: &Path) -> Result<PathBuf> {
    cwd.ancestors()
        .map(|directory| directory.join("Cargo.toml"))
        .filter(|manifest_path| manifest_path.is_file())
        .find(|manifest_path| manifest_has_package(manifest_path).unwrap_or(false))
        .context("run inside a Cargo package")
}

/// Finds an initialized project from its configuration file, rather than a
/// nearby workspace manifest.
pub(crate) fn find_initialized_project(cwd: &Path) -> Result<Project> {
    let root = cwd
        .ancestors()
        .find(|root| root.join("ratcn.toml").is_file())
        .context("could not find ratcn.toml; run cargo ratcn init")?;
    read_config(&root.join("ratcn.toml"))?;
    let manifest_path = root.join("Cargo.toml");
    if !manifest_path.is_file() || !manifest_has_package(&manifest_path)? {
        bail!(
            "{} is not a Cargo package; run cargo ratcn init",
            root.display()
        );
    }

    Ok(Project {
        root: root.to_path_buf(),
        manifest_path,
    })
}

/// The managed destination is `src/components/`; `src/components.rs` is the
/// other spelling of the same module and would make rustc reject both.
pub(crate) fn validate_components_destination(root: &Path) -> Result<()> {
    let components_file = root.join("src/components.rs");
    if components_file.exists() {
        bail!(
            "{} conflicts with the configured src/components/mod.rs destination",
            components_file.display()
        );
    }
    Ok(())
}

pub(crate) fn read_config(path: &Path) -> Result<RatcnConfig> {
    let source =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    parse_config(&source).with_context(|| format!("invalid {}", path.display()))
}

pub(crate) fn parse_config(source: &str) -> Result<RatcnConfig> {
    let document: Value = toml::from_str(source).context("could not parse TOML")?;
    let ratcn = document
        .get("ratcn")
        .and_then(Value::as_table)
        .context("expected a [ratcn] table")?;
    let version = ratcn
        .get("version")
        .and_then(Value::as_str)
        .context("expected ratcn.version to be a string")?;
    let components = ratcn
        .get("components")
        .and_then(Value::as_str)
        .context("expected ratcn.components to be a string")?;
    if components != COMPONENTS_PATH {
        bail!("expected ratcn.components to be \"{COMPONENTS_PATH}\"");
    }

    Ok(RatcnConfig {
        version: version.to_owned(),
    })
}

pub(crate) fn render_config(version: &str) -> String {
    include_str!("../templates/ratcn.toml").replace("{{version}}", version)
}

/// Resolved dependency graph for the package at `manifest_path`, with the
/// lockfile honored so a copy always matches what the project builds against.
pub(crate) fn metadata(manifest_path: &Path) -> Result<Metadata> {
    MetadataCommand::new()
        .manifest_path(manifest_path)
        .other_options(vec!["--locked".to_owned()])
        .exec()
        .with_context(|| {
            format!(
                "could not resolve dependencies for {}",
                manifest_path.display()
            )
        })
}

pub(crate) fn root_package(metadata: &Metadata) -> Result<&Package> {
    metadata
        .root_package()
        .context("cargo metadata did not report a root package")
}

/// The single conventional crate root (`src/main.rs` or `src/lib.rs`) `add`
/// may register `mod components;` in, or `None` when there is no unambiguous
/// place — then the user is told to add the line themselves.
pub(crate) fn conventional_crate_root(package: &Package) -> Option<PathBuf> {
    let package_root = package.manifest_path.parent()?;
    let conventional = [
        package_root.join("src/main.rs"),
        package_root.join("src/lib.rs"),
    ];
    let mut roots: Vec<_> = package
        .targets
        .iter()
        .filter(|target| target.is_bin() || target.is_lib())
        .map(|target| &target.src_path)
        .filter(|source_path| conventional.contains(source_path))
        .collect();
    roots.dedup();
    match roots.as_slice() {
        [root] => Some(root.as_std_path().to_path_buf()),
        _ => None,
    }
}

/// The `ratcn` package the project depends on directly. Also checks that the
/// project's own `ratatui` is the one `ratcn` resolved, since a copied
/// component uses both and they must agree.
pub(crate) fn resolve_ratcn(metadata: &Metadata) -> Result<ResolvedRatcn> {
    let root = root_package(metadata)?;
    let resolve = metadata
        .resolve
        .as_ref()
        .context("cargo metadata did not return a dependency graph")?;
    let root_node = node(&resolve.nodes, &root.id)?;
    let ratcn_id = direct_dependency(root_node, "ratcn")
        .context("ratcn must be a direct dependency; run cargo ratcn init")?;
    let ratatui_id = direct_dependency(root_node, "ratatui")
        .context("ratatui must be a direct dependency; run cargo ratcn init")?;
    let ratcn_ratatui_id = direct_dependency(node(&resolve.nodes, ratcn_id)?, "ratatui")
        .context("ratcn does not depend on ratatui")?;
    if ratatui_id != ratcn_ratatui_id {
        bail!("ratatui must resolve to the version used by ratcn; run cargo ratcn init");
    }

    let ratcn = package(&metadata.packages, ratcn_id)?;
    let components_dir = ratcn
        .manifest_path
        .parent()
        .context("ratcn package manifest has no parent directory")?
        .as_std_path()
        .join(COMPONENTS_PATH);

    Ok(ResolvedRatcn {
        version: ratcn.version.to_string(),
        components_dir,
    })
}

/// The `ratatui` version `ratcn` resolved, so `init` can add the same one.
pub(crate) fn ratcn_ratatui_version(metadata: &Metadata) -> Result<String> {
    let root = root_package(metadata)?;
    let resolve = metadata
        .resolve
        .as_ref()
        .context("cargo metadata did not return a dependency graph")?;
    let ratcn_id = direct_dependency(node(&resolve.nodes, &root.id)?, "ratcn")
        .context("ratcn must be a direct dependency; run cargo ratcn init")?;
    let ratatui_id = direct_dependency(node(&resolve.nodes, ratcn_id)?, "ratatui")
        .context("ratcn does not depend on ratatui")?;
    Ok(package(&metadata.packages, ratatui_id)?.version.to_string())
}

fn manifest_has_package(path: &Path) -> Result<bool> {
    let source =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    let document: Value =
        toml::from_str(&source).with_context(|| format!("could not parse {}", path.display()))?;
    Ok(document.get("package").is_some_and(Value::is_table))
}

fn node<'a>(nodes: &'a [Node], id: &PackageId) -> Result<&'a Node> {
    nodes
        .iter()
        .find(|node| node.id == *id)
        .with_context(|| format!("cargo metadata did not resolve {id}"))
}

fn package<'a>(packages: &'a [Package], id: &PackageId) -> Result<&'a Package> {
    packages
        .iter()
        .find(|package| package.id == *id)
        .with_context(|| format!("cargo metadata did not return package {id}"))
}

fn direct_dependency<'a>(node: &'a Node, name: &str) -> Option<&'a PackageId> {
    node.deps
        .iter()
        .find(|dependency| {
            dependency.name == name
                && dependency
                    .dep_kinds
                    .iter()
                    .any(|kind| kind.kind == DependencyKind::Normal)
        })
        .map(|dependency| &dependency.pkg)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{COMPONENTS_PATH, find_initialized_project, parse_config, render_config};

    #[test]
    fn parses_the_expected_config() {
        let config =
            parse_config("[ratcn]\nversion = \"0.0.2\"\ncomponents = \"src/components\"\n")
                .expect("the generated configuration should parse");

        assert_eq!(config.version, "0.0.2");
    }

    #[test]
    fn rejects_a_different_component_location() {
        let error = parse_config("[ratcn]\nversion = \"0.0.2\"\ncomponents = \"components\"\n")
            .expect_err("copied components must stay in the fixed location");

        assert!(error.to_string().contains(COMPONENTS_PATH));
    }

    #[test]
    fn renders_the_expected_config() {
        assert_eq!(
            render_config("1.2.3"),
            "[ratcn]\nversion = \"1.2.3\"\ncomponents = \"src/components\"\n"
        );
    }

    #[test]
    fn initialized_project_uses_the_configuration_root() {
        let directory = tempdir().expect("temporary directory should exist");
        fs::write(
            directory.path().join("ratcn.toml"),
            "[ratcn]\nversion = \"0.0.2\"\ncomponents = \"src/components\"\n",
        )
        .expect("configuration should write");
        fs::write(
            directory.path().join("Cargo.toml"),
            "[package]\nname = \"example\"\nversion = \"0.1.0\"\n",
        )
        .expect("manifest should write");
        let nested = directory.path().join("src").join("nested");
        fs::create_dir_all(&nested).expect("nested directory should create");

        let project = find_initialized_project(&nested).expect("project should be found");

        assert_eq!(project.root, directory.path());
    }
}
