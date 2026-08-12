use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;
use sha2::{Digest, Sha256};

use super::CommandExecutor;
use super::{CommandExecutionContext, CommandProcessMode, ProcessEnvironment, resolve_entry_bun};
use crate::catalog::CatalogSnapshot;
use crate::development::BUN;
use crate::profile::EntryProfileRecord;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    context: CommandExecutionContext,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("workspace root")
            .join("data/proj_cache/tests/entry-bun")
            .join(format!("{}-{sequence}", std::process::id()));
        let data_root = root.join("data");
        for directory in [
            data_root.clone(),
            root.join("_lib/proj"),
            root.join("project/.swaw"),
        ] {
            fs::create_dir_all(directory).expect("create fixture directory");
        }
        fs::write(root.join("swawkit-proj-toolchain.exe"), "fixture")
            .expect("write Toolchain fixture");
        let mut profile = EntryProfileRecord::default();
        profile.development.pwsh.mode = "disabled".to_owned();
        profile.development.msvc.mode = "disabled".to_owned();
        profile.development.rust.mode = "disabled".to_owned();
        let context = CommandExecutionContext {
            swawkit_home: root.clone(),
            kernel_root: root.join("_lib/proj"),
            target_project_root: root.join("project"),
            action_root: root.join("project/.swaw"),
            data_root,
            entry_name: "fixture".to_owned(),
            entry_file: root.join("fixture.exe"),
            invocation_directory: root.join("project"),
            toolchain_executable: root.join("swawkit-proj-toolchain.exe"),
            environment_input_revision: profile.environment_input_revision(),
            profile_revision: format!("sha256-{}", "0".repeat(64)),
            profile,
            process_mode: CommandProcessMode::InheritConsole,
        };
        Self { root, context }
    }

    fn publish_exact(&self, state_revision: &str, version: &str) -> PathBuf {
        let provider = self.context.data_root.join("modules/kernel/.dev/setup");
        let install = provider.join("export/bun/installs").join(version);
        fs::create_dir_all(&install).expect("create Bun installation");
        let executable = install.join("bun.exe");
        let bun = b"fixture";
        let bunx = b"@echo off\r\nfixture\r\n";
        fs::write(&executable, bun).expect("write Bun executable");
        fs::write(install.join("bunx.cmd"), bunx).expect("write Bunx shim");
        write_json(
            &install.join(".swawkit-dev-install.json"),
            &json!({
                "schema": "swawkit.proj-dev.install.v0",
                "name": "bun",
                "version": version,
                "sourceUrl": "https://example.invalid/bun.zip",
                "sourceSha256": "a".repeat(64),
                "sourceVerification": "unverified",
                "recipeVersion": BUN.recipe_version,
                "definitionSignature": BUN.definition_signature(
                    version,
                    &self.context.profile.development.bun.sha256.to_ascii_lowercase(),
                ),
                "files": [
                    {
                        "path": "bun.exe",
                        "length": bun.len(),
                        "sha256": sha256(bun)
                    },
                    {
                        "path": "bunx.cmd",
                        "length": bunx.len(),
                        "sha256": sha256(bunx)
                    }
                ]
            }),
        );
        write_json(
            &provider.join("_state.json"),
            &json!({
                "schema": "swawkit.command-provider-state/v1",
                "status": "ready",
                "inputRevision": state_revision,
                "token": "d".repeat(32),
                "producerContract": "swawkit.proj.dev-setup/v2"
            }),
        );
        executable
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn exact_entry_bun_resolves_from_the_current_provider_export() {
    let fixture = Fixture::new();
    let expected = fixture.publish_exact(
        &fixture.context.environment_input_revision,
        &fixture.context.profile.development.bun.version,
    );

    assert_eq!(resolve_entry_bun(&fixture.context).unwrap(), expected);
}

#[test]
fn stale_provider_state_is_rejected_with_one_repair_command() {
    let fixture = Fixture::new();
    fixture.publish_exact(
        &format!("sha256-{}", "e".repeat(64)),
        &fixture.context.profile.development.bun.version,
    );

    let error = resolve_entry_bun(&fixture.context).unwrap_err().to_string();
    assert!(error.contains("not ready for the current Entry Profile"));
    assert!(error.contains("fixture .dev.setup"));
}

#[test]
fn unsafe_bun_version_segment_is_rejected() {
    let mut fixture = Fixture::new();
    fixture.context.profile.development.bun.version = "../1.2.3".to_owned();

    let error = resolve_entry_bun(&fixture.context).unwrap_err().to_string();
    assert!(error.contains("not a supported Bun version"), "{error}");
}

#[test]
fn same_length_bun_tampering_is_rejected() {
    let fixture = Fixture::new();
    let executable = fixture.publish_exact(
        &fixture.context.environment_input_revision,
        &fixture.context.profile.development.bun.version,
    );
    fs::write(&executable, b"changed").expect("tamper Bun executable");

    let error = resolve_entry_bun(&fixture.context).unwrap_err().to_string();
    assert!(error.contains("SHA-256"), "{error}");
    assert!(error.contains("fixture .dev.setup"), "{error}");
}

#[test]
fn missing_bunx_contract_member_is_rejected() {
    let fixture = Fixture::new();
    let executable = fixture.publish_exact(
        &fixture.context.environment_input_revision,
        &fixture.context.profile.development.bun.version,
    );
    fs::remove_file(executable.with_file_name("bunx.cmd")).expect("remove Bunx shim");

    let error = resolve_entry_bun(&fixture.context).unwrap_err().to_string();
    assert!(error.contains("Entry Bun installation is invalid"));
}

#[test]
fn stale_bun_recipe_is_rejected() {
    let fixture = Fixture::new();
    let executable = fixture.publish_exact(
        &fixture.context.environment_input_revision,
        &fixture.context.profile.development.bun.version,
    );
    let metadata_path = executable.with_file_name(".swawkit-dev-install.json");
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(&metadata_path).unwrap()).unwrap();
    metadata["recipeVersion"] = json!("stale-recipe");
    write_json(&metadata_path, &metadata);

    let error = resolve_entry_bun(&fixture.context).unwrap_err().to_string();
    assert!(error.contains("metadata is stale"));
}

#[test]
fn latest_entry_bun_uses_the_published_selection() {
    let mut fixture = Fixture::new();
    fixture.context.profile.development.bun.version = "latest".to_owned();
    fixture.context.environment_input_revision =
        fixture.context.profile.environment_input_revision();
    let expected = fixture.publish_exact(&fixture.context.environment_input_revision, "1.3.14");
    let selection = fixture
        .context
        .data_root
        .join("modules/kernel/.dev/setup/export/bun/.swawkit-dev-selection.json");
    write_json(
        &selection,
        &json!({
            "schema": "swawkit.proj-dev.bun-selection.v0",
            "selector": "latest",
            "version": "1.3.14",
            "sourceSha256": "a".repeat(64),
            "sourceVerification": "unverified"
        }),
    );

    assert_eq!(resolve_entry_bun(&fixture.context).unwrap(), expected);
}

#[test]
fn bun_is_resolved_after_guards_can_change_the_published_installation() {
    let fixture = Fixture::new();
    let executable = fixture.publish_exact(
        &fixture.context.environment_input_revision,
        &fixture.context.profile.development.bun.version,
    );
    let action = fixture.context.action_root.join("task");
    fs::create_dir_all(&action).expect("create Action command");
    fs::write(action.join("run.ts"), "console.log('must not run')").expect("write Action command");
    let guard = fixture.context.kernel_root.join("_global");
    fs::create_dir_all(&guard).expect("create global guard");
    let escaped = executable.to_string_lossy().replace('\'', "''");
    fs::write(
        guard.join("run.ps1"),
        format!(
            "[IO.File]::WriteAllBytes('{escaped}', [Text.Encoding]::UTF8.GetBytes('changed'))\n"
        ),
    )
    .expect("write mutating guard");
    let catalog = CatalogSnapshot::discover_roots(
        &fixture.context.kernel_root,
        &fixture.context.action_root,
        "fixture",
    )
    .expect("discover fixture Catalog");

    let error = CommandExecutor::new(&fixture.context, &catalog)
        .execute(&[OsString::from("task")])
        .unwrap_err()
        .to_string();

    assert!(error.contains("SHA-256"), "{error}");
    assert!(error.contains("fixture .dev.setup"), "{error}");
}

#[test]
fn action_environment_contains_only_validated_enabled_domains() {
    let fixture = Fixture::new();
    let expected = fixture.publish_exact(
        &fixture.context.environment_input_revision,
        &fixture.context.profile.development.bun.version,
    );

    let resolved = super::resolve_entry_development(&fixture.context).unwrap();

    assert_eq!(resolved.bun_executable, expected);
    assert_eq!(resolved.environment.paths(), [expected.parent().unwrap()]);
    assert!(resolved.environment.variables().is_empty());
}

#[test]
fn applying_action_environment_removes_disabled_domain_variables() {
    let fixture = Fixture::new();
    let expected = fixture.publish_exact(
        &fixture.context.environment_input_revision,
        &fixture.context.profile.development.bun.version,
    );
    let resolved = super::resolve_entry_development(&fixture.context).unwrap();
    let mut environment = ProcessEnvironment::default();
    environment
        .apply_development_environment(
            &resolved.environment,
            &fixture
                .context
                .data_root
                .join("modules/kernel/.dev/setup/export"),
        )
        .unwrap();

    assert_eq!(environment.value("CARGO_HOME"), Some(None));
    let path = environment
        .value("PATH")
        .and_then(|value| value)
        .unwrap()
        .to_string_lossy();
    let first = std::env::split_paths(path.as_ref()).next().unwrap();
    assert_eq!(first, expected.parent().unwrap());
}

fn write_json(path: &Path, value: &serde_json::Value) {
    fs::create_dir_all(path.parent().expect("JSON parent")).expect("create JSON parent");
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).expect("write JSON fixture");
}

fn sha256(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
