use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use assert_cmd::Command as AssertCommand;
use tempfile::{TempDir, tempdir};
use toml::Value;

const COMPONENTS: &[&str] = &[
    "barchart",
    "button",
    "checkbox",
    "cycle",
    "dialog",
    "list",
    "progress",
    "scroll_area",
    "select",
    "tabs",
    "toast",
    "tooltip",
];

const MAIN_SOURCE: &str = "fn main() {\n    println!(\"existing application code\");\n}\n";
const INIT_OUTPUT: &str = "┌   cargo ratcn init \n│\n└  You're all set!\n";
const MINIMAL_APP_TEMPLATE: &str = include_str!("../templates/minimal-app.rs");
const FIRST_APP_TEMPLATE: &str = include_str!("../templates/first-app.rs");

fn ratcn_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ratcn")
        .canonicalize()
        .expect("the checkout's ratcn crate should exist")
}

fn toml_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn cargo_project(with_dependencies: bool) -> TempDir {
    let temporary = tempdir().expect("temporary project directory should exist");
    let root = temporary.path();
    fs::create_dir_all(root.join("src")).expect("project source directory should exist");

    let dependencies = if with_dependencies {
        format!(
            "\n[dependencies]\nratcn = {{ path = \"{}\" }}\nratatui = {{ version = \"0.30.2\", default-features = false, features = [\"layout-cache\", \"std\"] }}\n",
            toml_path(&ratcn_path())
        )
    } else {
        String::new()
    };
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"cargo-ratcn-integration-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{dependencies}"
        ),
    )
    .expect("fixture manifest should write");
    fs::write(root.join("src/main.rs"), MAIN_SOURCE).expect("fixture entrypoint should write");

    temporary
}

fn generate_lockfile(project: &Path) {
    let output = Command::new("cargo")
        .current_dir(project)
        .args(["generate-lockfile", "--offline"])
        .output()
        .expect("cargo should generate the fixture lockfile");
    assert!(
        output.status.success(),
        "cargo generate-lockfile --offline failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn initialize_for_add(project: &Path) {
    generate_lockfile(project);
    fs::write(
        project.join("ratcn.toml"),
        format!(
            "[ratcn]\nversion = \"{}\"\ncomponents = \"src/components\"\n",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .expect("fixture configuration should write");
}

fn run_cli(project: &Path, arguments: &[&str]) -> Output {
    let mut command = AssertCommand::cargo_bin("cargo-ratcn")
        .expect("the cargo-ratcn binary should be built for integration tests");
    command
        .current_dir(project)
        .env("CARGO_NET_OFFLINE", "true")
        .args(arguments);
    command
        .output()
        .expect("the cargo-ratcn binary should run for integration tests")
}

fn run_cargo_subcommand(project: &Path, arguments: &[&str]) -> Output {
    let binary = assert_cmd::cargo::cargo_bin("cargo-ratcn");
    let binary_directory = binary
        .parent()
        .expect("the cargo-ratcn binary should have a parent directory");
    let path = env::join_paths(
        std::iter::once(binary_directory.to_path_buf()).chain(
            env::var_os("PATH")
                .as_deref()
                .map(env::split_paths)
                .into_iter()
                .flatten(),
        ),
    )
    .expect("the PATH entries should be valid");
    Command::new("cargo")
        .current_dir(project)
        .env("CARGO_NET_OFFLINE", "true")
        .env("PATH", path)
        .arg("ratcn")
        .args(arguments)
        .output()
        .expect("Cargo should run the cargo-ratcn external subcommand")
}

fn assert_cliclack_success<I, S>(output: &Output, title: &str, outro: &str, messages: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    assert!(
        output.status.success(),
        "command unexpectedly failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with(&format!("┌  {title}\n│\n")),
        "cliclack session should start with its title:\n{stderr}"
    );
    assert!(
        stderr.ends_with(&format!("└  {outro}\n")),
        "cliclack session should end with its outro:\n{stderr}"
    );

    let mut cursor = 0;
    for message in messages {
        let message = message.as_ref();
        let offset = stderr[cursor..]
            .find(message)
            .unwrap_or_else(|| panic!("missing cliclack message {message:?}:\n{stderr}"));
        cursor += offset + message.len();
    }
}

fn assert_init_success(output: &Output) {
    assert!(
        output.status.success(),
        "init unexpectedly failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        INIT_OUTPUT,
        "init should render one complete cliclack session"
    );
}

fn assert_failure(output: &Output, expected_stdout: &str, expected_stderr: &str) {
    assert!(
        !output.status.success(),
        "command should reject this request instead of succeeding"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout,
        "a rejected request should not claim it changed anything"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        expected_stderr,
        "the diagnostic should tell the user how to correct the request"
    );
}

fn cargo_check(project: &Path) {
    let output = Command::new("cargo")
        .current_dir(project)
        .args(["check", "--locked", "--offline"])
        .output()
        .expect("cargo check should run for the generated fixture");
    assert!(
        output.status.success(),
        "cargo check --locked --offline failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn manifest_dependencies(project: &Path) -> toml::map::Map<String, Value> {
    let manifest = fs::read_to_string(project.join("Cargo.toml"))
        .expect("fixture manifest should remain readable");
    let document: Value = toml::from_str(&manifest).expect("fixture manifest should remain valid");
    document
        .get("dependencies")
        .and_then(Value::as_table)
        .expect("fixture manifest should retain a dependency table")
        .clone()
}

fn dependency_features(dependency: &toml::map::Map<String, Value>) -> Vec<&str> {
    dependency
        .get("features")
        .and_then(Value::as_array)
        .expect("dependency should have a feature list")
        .iter()
        .map(|feature| feature.as_str().expect("feature names should be strings"))
        .collect()
}

#[test]
fn cargo_external_subcommand_invocation_forwards_the_subcommand_name() {
    let temporary = cargo_project(true);
    let project = temporary.path();
    initialize_for_add(project);

    let help = run_cargo_subcommand(project, &["--help"]);
    assert!(
        help.status.success(),
        "cargo ratcn --help failed:\n{}",
        String::from_utf8_lossy(&help.stderr)
    );
    assert!(
        String::from_utf8_lossy(&help.stdout).contains("Manage ratcn components"),
        "help output should come from cargo-ratcn"
    );

    let list = run_cargo_subcommand(project, &["add", "--list"]);
    assert_cliclack_success(
        &list,
        " cargo ratcn add ",
        "Add one with `cargo ratcn add <name>`",
        std::iter::once("Available components").chain(COMPONENTS.iter().copied()),
    );
}

#[test]
fn adding_dialog_from_a_nested_directory_preserves_the_entrypoint_and_compiles() {
    let temporary = cargo_project(true);
    let project = temporary.path();
    initialize_for_add(project);

    let components = project.join("src/components");
    fs::create_dir_all(&components).expect("components directory should exist");
    fs::write(
        components.join("mod.rs"),
        "// Existing registrations must remain.\npub mod legacy;\n",
    )
    .expect("existing component module should write");
    fs::write(components.join("legacy.rs"), "pub const LEGACY: u8 = 1;\n")
        .expect("existing component should write");
    let nested = project.join("src/nested/deeper");
    fs::create_dir_all(&nested).expect("nested working directory should exist");

    let output = run_cli(&nested, &["add", "dialog"]);
    assert_cliclack_success(
        &output,
        " cargo ratcn add ",
        "Component added!",
        [
            "Added src/components/dialog.rs (ratcn 0.0.2)",
            "Registered src/components/mod.rs",
            "Registered src/main.rs",
            "Import",
            "use crate::components::dialog;",
        ],
    );

    let copied = fs::read_to_string(components.join("dialog.rs"))
        .expect("the dialog source should be copied into the project");
    let (header, source) = copied
        .split_once("\n\n")
        .expect("the copied source should separate its provenance header");
    assert_eq!(
        header,
        "// Copied from ratcn 0.0.2: src/components/dialog.rs"
    );
    assert!(source.starts_with("use std::fmt;\n\nuse ratatui::"));
    assert!(source.contains("use ratcn::Theme;"));
    assert!(source.contains("use ratcn::geometry::{is_border, wrapped_height};"));
    assert!(source.contains("use ratcn::runtime::{"));
    assert!(!source.contains("use crate::"));
    assert!(!source.contains("#[cfg(test)]"));
    assert!(!source.contains("fn driver("));
    assert_eq!(
        fs::read_to_string(components.join("mod.rs")).expect("component module should remain"),
        "// Existing registrations must remain.\npub mod legacy;\npub mod dialog;\n"
    );
    let main = fs::read_to_string(project.join("src/main.rs"))
        .expect("entrypoint should remain readable after registration");
    assert_eq!(main, format!("mod components;\n\n{MAIN_SOURCE}"));
    assert_eq!(
        main.matches("mod components;").count(),
        1,
        "the entrypoint should receive one module declaration"
    );

    cargo_check(project);
}

#[test]
fn listing_components_is_sorted_and_does_not_modify_the_initialized_project() {
    let temporary = cargo_project(true);
    let project = temporary.path();
    initialize_for_add(project);
    let manifest_before = fs::read_to_string(project.join("Cargo.toml"))
        .expect("fixture manifest should be readable before listing");
    let main_before = fs::read_to_string(project.join("src/main.rs"))
        .expect("fixture entrypoint should be readable before listing");

    let output = run_cli(project, &["add", "--list"]);
    assert_cliclack_success(
        &output,
        " cargo ratcn add ",
        "Add one with `cargo ratcn add <name>`",
        std::iter::once("Available components").chain(COMPONENTS.iter().copied()),
    );

    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).expect("fixture manifest should remain"),
        manifest_before
    );
    assert_eq!(
        fs::read_to_string(project.join("src/main.rs")).expect("fixture entrypoint should remain"),
        main_before
    );
    assert!(
        !project.join("src/components").exists(),
        "listing choices must not create a managed module"
    );
}

#[test]
fn a_collision_in_a_multi_component_addition_leaves_every_requested_change_absent() {
    let temporary = cargo_project(true);
    let project = temporary.path();
    initialize_for_add(project);
    let components = project.join("src/components");
    fs::create_dir_all(&components).expect("components directory should exist");
    fs::write(components.join("dialog.rs"), "// user-owned dialog\n")
        .expect("colliding component should write");
    let manifest_before = fs::read_to_string(project.join("Cargo.toml"))
        .expect("fixture manifest should be readable before the rejection");
    let main_before = fs::read_to_string(project.join("src/main.rs"))
        .expect("fixture entrypoint should be readable before the rejection");

    let output = run_cli(project, &["add", "button", "dialog"]);
    assert_failure(
        &output,
        "",
        &format!(
            "error: component file already exists: {}; use --force to replace it\n",
            components.join("dialog.rs").display()
        ),
    );
    assert_eq!(
        fs::read_to_string(components.join("dialog.rs")).expect("user file should remain"),
        "// user-owned dialog\n"
    );
    assert!(
        !components.join("button.rs").exists(),
        "the noncolliding component must not be written before the collision fails"
    );
    assert!(
        !components.join("mod.rs").exists(),
        "a failed batch must not register any component modules"
    );
    assert_eq!(
        fs::read_to_string(project.join("src/main.rs")).expect("entrypoint should remain"),
        main_before
    );
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).expect("manifest should remain"),
        manifest_before
    );
}

#[test]
fn components_file_conflict_leaves_the_project_unchanged() {
    let temporary = cargo_project(true);
    let project = temporary.path();
    initialize_for_add(project);
    let components_file = project.join("src/components.rs");
    let components_source = "// user-owned module file\n";
    fs::write(&components_file, components_source).expect("conflicting module file should write");
    let main_before = fs::read_to_string(project.join("src/main.rs"))
        .expect("entrypoint should be readable before the rejection");

    let output = run_cli(project, &["add", "button"]);

    assert_failure(
        &output,
        "",
        &format!(
            "error: {} conflicts with the configured src/components/mod.rs destination\n",
            components_file.display()
        ),
    );
    assert_eq!(
        fs::read_to_string(&components_file).expect("conflicting module file should remain"),
        components_source
    );
    assert_eq!(
        fs::read_to_string(project.join("src/main.rs")).expect("entrypoint should remain"),
        main_before
    );
    assert!(
        !project.join("src/components").exists(),
        "the ambiguity must fail before creating the configured module directory"
    );
}

#[test]
fn force_replaces_an_existing_component_file() {
    let temporary = cargo_project(true);
    let project = temporary.path();
    initialize_for_add(project);
    let components = project.join("src/components");
    fs::create_dir_all(&components).expect("components directory should exist");
    let button = components.join("button.rs");
    fs::write(&button, "// user-owned button\n").expect("existing component should write");

    let output = run_cli(project, &["add", "button", "--force"]);

    assert_cliclack_success(
        &output,
        " cargo ratcn add ",
        "Component added!",
        [
            "Added src/components/button.rs (ratcn 0.0.2)",
            "Registered src/components/mod.rs",
            "Registered src/main.rs",
            "Import",
            "use crate::components::button;",
        ],
    );
    let copied = fs::read_to_string(&button).expect("forced component should be readable");
    assert!(copied.starts_with("// Copied from ratcn 0.0.2: src/components/button.rs\n"));
    assert!(
        !copied.contains("user-owned button"),
        "--force must replace the prior component content"
    );
}

#[test]
fn rejecting_registry_sources_and_uninitialized_projects_never_creates_component_files() {
    let temporary = cargo_project(true);
    let project = temporary.path();
    initialize_for_add(project);
    let manifest_before = fs::read_to_string(project.join("Cargo.toml"))
        .expect("fixture manifest should be readable before registry rejection");
    let main_before = fs::read_to_string(project.join("src/main.rs"))
        .expect("fixture entrypoint should be readable before registry rejection");

    for source in ["https://example.test/dialog", "@other/dialog"] {
        let output = run_cli(project, &["add", source]);
        assert_failure(
            &output,
            "",
            "error: third-party registries are not supported\n",
        );
    }
    assert_eq!(
        fs::read_to_string(project.join("Cargo.toml")).expect("manifest should remain"),
        manifest_before
    );
    assert_eq!(
        fs::read_to_string(project.join("src/main.rs")).expect("entrypoint should remain"),
        main_before
    );
    assert!(
        !project.join("src/components").exists(),
        "registry rejection must not create copied component files"
    );

    let uninitialized = cargo_project(false);
    let uninitialized_root = uninitialized.path();
    let manifest_before = fs::read_to_string(uninitialized_root.join("Cargo.toml"))
        .expect("uninitialized manifest should be readable");
    let main_before = fs::read_to_string(uninitialized_root.join("src/main.rs"))
        .expect("uninitialized entrypoint should be readable");

    let output = run_cli(uninitialized_root, &["add", "dialog"]);
    assert_failure(
        &output,
        "",
        "error: could not find ratcn.toml; run cargo ratcn init\n",
    );
    assert_eq!(
        fs::read_to_string(uninitialized_root.join("Cargo.toml"))
            .expect("uninitialized manifest should remain"),
        manifest_before
    );
    assert_eq!(
        fs::read_to_string(uninitialized_root.join("src/main.rs"))
            .expect("uninitialized entrypoint should remain"),
        main_before
    );
    assert!(
        !uninitialized_root.join("src/components").exists(),
        "missing configuration must not create a managed module"
    );
}

#[test]
fn adding_every_available_component_creates_a_compilable_consumer_crate() {
    let temporary = cargo_project(true);
    let project = temporary.path();
    initialize_for_add(project);

    let output = run_cli(
        project,
        &[
            "add",
            "barchart",
            "button",
            "checkbox",
            "cycle",
            "dialog",
            "list",
            "progress",
            "scroll_area",
            "select",
            "tabs",
            "toast",
            "tooltip",
        ],
    );
    let mut expected_messages = COMPONENTS
        .iter()
        .map(|component| format!("Added src/components/{component}.rs (ratcn 0.0.2)"))
        .collect::<Vec<_>>();
    expected_messages.extend([
        "Registered src/components/mod.rs".to_owned(),
        "Registered src/main.rs".to_owned(),
        "Imports".to_owned(),
    ]);
    expected_messages.extend(
        COMPONENTS
            .iter()
            .map(|component| format!("use crate::components::{component};")),
    );
    assert_cliclack_success(
        &output,
        " cargo ratcn add ",
        "Components added!",
        &expected_messages,
    );

    for component in COMPONENTS {
        assert!(
            project
                .join("src/components")
                .join(format!("{component}.rs"))
                .is_file(),
            "{component} should be copied into the consumer crate"
        );
    }
    assert_eq!(
        fs::read_to_string(project.join("src/components/mod.rs"))
            .expect("managed module should be created"),
        "// This file is managed by cargo ratcn.\n\
pub mod barchart;\n\
pub mod button;\n\
pub mod checkbox;\n\
pub mod cycle;\n\
pub mod dialog;\n\
pub mod list;\n\
pub mod progress;\n\
pub mod scroll_area;\n\
pub mod select;\n\
pub mod tabs;\n\
pub mod toast;\n\
pub mod tooltip;\n"
    );

    cargo_check(project);
}

#[test]
fn init_offline_keeps_terminal_dependencies_and_is_safe_to_rerun() {
    let temporary = cargo_project(true);
    let project = temporary.path();
    generate_lockfile(project);
    let main_before = fs::read_to_string(project.join("src/main.rs"))
        .expect("fixture entrypoint should be readable before initialization");

    let output = run_cli(project, &["init"]);
    assert_init_success(&output);

    let dependencies = manifest_dependencies(project);
    let ratcn = dependencies
        .get("ratcn")
        .and_then(Value::as_table)
        .expect("init should retain ratcn as a detailed local dependency");
    let configured_ratcn_path = ratcn
        .get("path")
        .and_then(Value::as_str)
        .expect("init must keep the fixture's local ratcn path");
    let configured_ratcn_path = Path::new(configured_ratcn_path);
    let configured_ratcn_path = if configured_ratcn_path.is_absolute() {
        configured_ratcn_path.to_path_buf()
    } else {
        project.join(configured_ratcn_path)
    };
    assert_eq!(
        configured_ratcn_path
            .canonicalize()
            .expect("configured ratcn path should resolve"),
        ratcn_path()
    );
    assert_eq!(dependency_features(ratcn), ["termina"]);

    let ratatui = dependencies
        .get("ratatui")
        .and_then(Value::as_table)
        .expect("init should retain ratatui as a detailed dependency");
    assert_eq!(
        ratatui.get("default-features").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(dependency_features(ratatui), ["layout-cache", "std"]);
    assert_eq!(
        fs::read_to_string(project.join("ratcn.toml")).expect("init should create configuration"),
        "[ratcn]\nversion = \"0.0.2\"\ncomponents = \"src/components\"\n"
    );
    assert_eq!(
        fs::read_to_string(project.join("src/components/mod.rs"))
            .expect("init should create managed module"),
        "// This file is managed by cargo ratcn.\n"
    );
    let mut component_entries = fs::read_dir(project.join("src/components"))
        .expect("managed component directory should be readable")
        .map(|entry| {
            entry
                .expect("managed component directory entry should be readable")
                .file_name()
        })
        .collect::<Vec<_>>();
    component_entries.sort();
    assert_eq!(component_entries, ["mod.rs"]);
    assert_eq!(
        fs::read_to_string(project.join("src/main.rs")).expect("entrypoint should remain"),
        main_before
    );

    let config_before = fs::read_to_string(project.join("ratcn.toml"))
        .expect("generated configuration should remain readable");
    let module_before = fs::read_to_string(project.join("src/components/mod.rs"))
        .expect("generated module should remain readable");
    let output = run_cli(project, &["init"]);
    assert_init_success(&output);
    assert_eq!(
        fs::read_to_string(project.join("ratcn.toml")).expect("configuration should remain"),
        config_before
    );
    assert_eq!(
        fs::read_to_string(project.join("src/components/mod.rs")).expect("module should remain"),
        module_before
    );
    assert_eq!(
        fs::read_to_string(project.join("src/main.rs")).expect("entrypoint should remain"),
        main_before
    );

    cargo_check(project);
}

#[test]
fn init_accepts_a_custom_crate_root_without_touching_it() {
    let temporary = cargo_project(true);
    let project = temporary.path();
    let manifest_path = project.join("Cargo.toml");
    fs::remove_file(project.join("src/main.rs")).expect("default binary root should be removed");
    let manifest = fs::read_to_string(&manifest_path)
        .expect("fixture manifest should be readable before initialization");
    fs::write(
        &manifest_path,
        format!("{manifest}\n[lib]\npath = \"src/custom.rs\"\n"),
    )
    .expect("fixture manifest should add a custom library target");
    let custom_root = "pub fn custom() {}\n";
    fs::write(project.join("src/custom.rs"), custom_root)
        .expect("custom library root should write");
    generate_lockfile(project);

    let output = run_cli(project, &["init"]);

    assert_init_success(&output);
    assert!(
        project.join("Cargo.lock").exists(),
        "existing package lockfile should remain available after initialization"
    );
    assert_eq!(
        fs::read_to_string(project.join("ratcn.toml")).expect("configuration should write"),
        "[ratcn]\nversion = \"0.0.2\"\ncomponents = \"src/components\"\n"
    );
    assert_eq!(
        fs::read_to_string(project.join("src/components/mod.rs"))
            .expect("managed module should write"),
        "// This file is managed by cargo ratcn.\n"
    );
    assert_eq!(
        fs::read_to_string(project.join("src/custom.rs")).expect("custom root should remain"),
        custom_root,
        "init must not modify custom crate roots"
    );
}

#[test]
fn starter_templates_compile_after_terminal_initialization() {
    for (name, template) in [
        ("minimal app", MINIMAL_APP_TEMPLATE),
        ("first app demo", FIRST_APP_TEMPLATE),
    ] {
        let temporary = cargo_project(true);
        let project = temporary.path();
        generate_lockfile(project);

        let output = run_cli(project, &["init"]);
        assert_init_success(&output);
        fs::write(project.join("src/main.rs"), template)
            .expect("starter template should replace the fixture entrypoint");

        cargo_check(project);
        assert!(
            fs::read_to_string(project.join("src/main.rs"))
                .expect("starter entrypoint should remain readable")
                .contains("fn main()"),
            "the {name} starter must install a complete binary entrypoint"
        );
    }
}

#[test]
fn add_with_ambiguous_standard_crate_roots_prints_the_manual_registration() {
    let temporary = cargo_project(true);
    let project = temporary.path();
    let library = "pub fn library() {}\n";
    fs::write(project.join("src/lib.rs"), library).expect("library entrypoint should write");
    initialize_for_add(project);

    let output = run_cli(project, &["add", "dialog"]);

    assert_cliclack_success(
        &output,
        " cargo ratcn add ",
        "Component added!",
        [
            "Added src/components/dialog.rs (ratcn 0.0.2)",
            "Registered src/components/mod.rs",
            "Add `mod components;` to your crate entrypoint (src/main.rs or src/lib.rs)",
            "Import",
            "use crate::components::dialog;",
        ],
    );
    assert_eq!(
        fs::read_to_string(project.join("src/main.rs")).expect("binary root should remain"),
        MAIN_SOURCE
    );
    assert_eq!(
        fs::read_to_string(project.join("src/lib.rs")).expect("library root should remain"),
        library
    );
}

#[test]
fn add_with_a_custom_crate_root_prints_manual_registration_without_touching_it() {
    let temporary = cargo_project(true);
    let project = temporary.path();
    let manifest_path = project.join("Cargo.toml");
    fs::remove_file(project.join("src/main.rs")).expect("default binary root should be removed");
    let manifest = fs::read_to_string(&manifest_path)
        .expect("fixture manifest should be readable before adding a custom target");
    fs::write(
        &manifest_path,
        format!("{manifest}\n[lib]\npath = \"src/custom.rs\"\n"),
    )
    .expect("fixture manifest should add a custom library target");
    let custom_root = "pub fn custom() {}\n";
    fs::write(project.join("src/custom.rs"), custom_root)
        .expect("custom library root should write");
    initialize_for_add(project);

    let output = run_cli(project, &["add", "dialog"]);

    assert_cliclack_success(
        &output,
        " cargo ratcn add ",
        "Component added!",
        [
            "Added src/components/dialog.rs (ratcn 0.0.2)",
            "Registered src/components/mod.rs",
            "Add `mod components;` to your crate entrypoint (src/main.rs or src/lib.rs)",
            "Import",
            "use crate::components::dialog;",
        ],
    );
    assert_eq!(
        fs::read_to_string(project.join("src/custom.rs")).expect("custom root should remain"),
        custom_root,
        "add must not claim or modify a custom crate root"
    );
}
