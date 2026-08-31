use std::{fs, path::Path, process::Command};

use anyhow::{Context, Result, bail};

use crate::project::{
    COMPONENTS_PATH, find_package_manifest, metadata, ratcn_ratatui_version, read_config,
    render_config, resolve_ratcn, validate_components_destination,
};

const MODULE_TEMPLATE: &str = include_str!("../../templates/components-mod.rs");
const MINIMAL_APP_TEMPLATE: &str = include_str!("../../templates/minimal-app.rs");
const FIRST_APP_TEMPLATE: &str = include_str!("../../templates/first-app.rs");
const CARGO_NEW_MAIN: &str = "fn main() {\n    println!(\"Hello, world!\");\n}\n";

pub(crate) fn execute(cwd: &Path) -> Result<()> {
    let manifest_path = find_package_manifest(cwd)?;
    let root = manifest_path
        .parent()
        .context("Cargo manifest has no parent directory")?;
    validate_components_destination(root)?;
    let config_path = root.join("ratcn.toml");
    if config_path.exists() {
        read_config(&config_path)?;
    }

    super::intro("cargo ratcn init")?;

    let starter = choose_starter(root)?;

    let spinner = cliclack::spinner();
    spinner.start("Setting up project");
    let result = initialize(starter, &manifest_path, root);
    match &result {
        Ok(()) => {
            spinner.stop("Project setup complete");
            cliclack::outro("You're all set!").context("could not finish setup output")?;
        }
        Err(_) => {
            spinner.error("Project setup failed");
            cliclack::outro_cancel("Setup failed").context("could not finish setup output")?;
        }
    }
    result
}

/// Every step is idempotent, so a failure halfway is recovered by running
/// `init` again: `cargo add` is a no-op for a present dependency, and generated
/// files are only written when missing.
fn initialize(starter: Starter, manifest_path: &Path, root: &Path) -> Result<()> {
    run_cargo_add(&["ratcn", "--features", "termina"], manifest_path)?;
    let ratatui_version = ratcn_ratatui_version(&metadata(manifest_path)?)?;
    let ratatui_add = ratatui_add_arguments(&ratatui_version);
    let ratatui_add: Vec<&str> = ratatui_add.iter().map(String::as_str).collect();
    run_cargo_add(&ratatui_add, manifest_path)?;
    let ratcn = resolve_ratcn(&metadata(manifest_path)?)?;

    let config_path = root.join("ratcn.toml");
    if !config_path.exists() {
        fs::write(&config_path, render_config(&ratcn.version))
            .with_context(|| format!("could not write {}", config_path.display()))?;
    }

    let components_dir = root.join(COMPONENTS_PATH);
    let module_path = components_dir.join("mod.rs");
    if !module_path.exists() {
        fs::create_dir_all(&components_dir)
            .with_context(|| format!("could not create {}", components_dir.display()))?;
        fs::write(&module_path, MODULE_TEMPLATE)
            .with_context(|| format!("could not write {}", module_path.display()))?;
    }

    if let Some(template) = starter.template() {
        let main = root.join("src/main.rs");
        fs::write(&main, template)
            .with_context(|| format!("could not write {}", main.display()))?;
    }

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Starter {
    KeepMain,
    MinimalApp,
    FirstApp,
}

impl Starter {
    fn template(self) -> Option<&'static str> {
        match self {
            Self::KeepMain => None,
            Self::MinimalApp => Some(MINIMAL_APP_TEMPLATE),
            Self::FirstApp => Some(FIRST_APP_TEMPLATE),
        }
    }
}

/// Offers a starter only over Cargo's untouched default `main.rs`; anything the
/// user has written is never replaced.
fn choose_starter(root: &Path) -> Result<Starter> {
    if !has_cargo_new_main(root) {
        return Ok(Starter::KeepMain);
    }

    let starter = cliclack::select("What should cargo ratcn do with src/main.rs?")
        .item(
            Starter::KeepMain,
            "Keep it unchanged",
            "I will wire the application myself",
        )
        .item(
            Starter::MinimalApp,
            "Minimal app loop",
            "Draw a terminal app and exit with Ctrl+C",
        )
        .item(
            Starter::FirstApp,
            "First app demo",
            "Install the Getting started button example",
        )
        .interact()
        .context("could not select a starter application")?;
    Ok(starter)
}

fn has_cargo_new_main(root: &Path) -> bool {
    fs::read_to_string(root.join("src/main.rs")).is_ok_and(|main| main == CARGO_NEW_MAIN)
}

fn ratatui_add_arguments(version: &str) -> [String; 4] {
    [
        format!("ratatui@{version}"),
        "--no-default-features".to_owned(),
        "--features".to_owned(),
        "layout-cache,std".to_owned(),
    ]
}

fn run_cargo_add(arguments: &[&str], manifest_path: &Path) -> Result<()> {
    let attempted = format!(
        "cargo add {} --manifest-path {}",
        arguments.join(" "),
        manifest_path.display()
    );
    let output = Command::new("cargo")
        .arg("add")
        .args(arguments)
        .arg("--manifest-path")
        .arg(manifest_path)
        .output()
        .with_context(|| format!("could not run {attempted}"))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    bail!("{attempted} failed with {}: {detail}", output.status)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{CARGO_NEW_MAIN, has_cargo_new_main, ratatui_add_arguments};

    #[test]
    fn recognizes_only_cargos_untouched_entrypoint() {
        let directory = tempdir().expect("temporary project directory should exist");
        let source = directory.path().join("src");
        fs::create_dir(&source).expect("source directory should write");
        let main = source.join("main.rs");
        fs::write(&main, CARGO_NEW_MAIN).expect("Cargo default main should write");

        assert!(has_cargo_new_main(directory.path()));

        fs::write(&main, "fn main() {}\n").expect("custom main should write");
        assert!(
            !has_cargo_new_main(directory.path()),
            "a custom entrypoint must not receive the scaffold prompt"
        );
    }

    #[test]
    fn ratatui_add_uses_the_resolved_ratcn_version() {
        let arguments = ratatui_add_arguments("1.2.3");

        assert_eq!(
            arguments.iter().map(String::as_str).collect::<Vec<_>>(),
            [
                "ratatui@1.2.3",
                "--no-default-features",
                "--features",
                "layout-cache,std"
            ]
        );
    }
}
