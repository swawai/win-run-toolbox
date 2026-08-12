use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::*;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    data_root: PathBuf,
    definition: RustDefinition,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "swawkit-rust-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let data_root = root.join("data");
        fs::create_dir_all(&data_root).unwrap();
        Self {
            root,
            data_root,
            definition: RustDefinition::new("stable", PROFILE, HOST).unwrap(),
        }
    }

    fn publish(&self) -> PathBuf {
        let install = self
            .data_root
            .join("modules/kernel/.dev/setup/export/rust/installs/stable");
        let mut records = Vec::new();
        for (index, relative) in self.definition.required_paths().into_iter().enumerate() {
            let content = if relative == "cargo\\bin\\rustup.exe" {
                b"rustup-fixture".to_vec()
            } else {
                format!("rust-file-{index}").into_bytes()
            };
            write_file(&install, &relative, &content);
            records.push(record(&relative, &content));
        }
        let extra = format!(
            "rustup\\toolchains\\{}\\lib\\rustlib\\{}\\lib\\libstd-fixture.rlib",
            self.definition.toolchain_name(),
            HOST
        );
        let extra_content = b"standard-library";
        write_file(&install, &extra, extra_content);
        records.push(record(&extra, extra_content));
        records.sort_by(|left, right| {
            left["path"]
                .as_str()
                .unwrap()
                .cmp(right["path"].as_str().unwrap())
        });
        let rustup_hash = records
            .iter()
            .find(|record| record["path"] == "cargo\\bin\\rustup.exe")
            .unwrap()["sha256"]
            .as_str()
            .unwrap();
        let metadata = json!({
            "schema": "swawkit.proj-dev.rust-install.v0",
            "name": "rust",
            "inventory": "toolchain-files-v0",
            "declaredToolchain": self.definition.toolchain(),
            "toolchainName": self.definition.toolchain_name(),
            "profile": PROFILE,
            "host": HOST,
            "components": ["rustfmt"],
            "recipeVersion": "2",
            "definitionSignature": self.definition.definition_signature(),
            "rustupInitUrl": RUSTUP_URL,
            "rustupInitSha256": rustup_hash,
            "rustupVersion": "1.29.0",
            "rustcVersion": "1.97.1",
            "rustcCommit": "a".repeat(40),
            "cargoVersion": "1.97.1",
            "rustfmtVersion": "1.8.0-stable",
            "sourceVerification": "rust-static-sha256",
            "files": records,
        });
        fs::write(
            install.join(".swawkit-dev-rust.json"),
            serde_json::to_vec_pretty(&metadata).unwrap(),
        )
        .unwrap();
        install
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn definition_matches_the_power_shell_contract() {
    let definition = RustDefinition::new(" Nightly-2026-07-28 ", " MINIMAL ", HOST).unwrap();
    assert_eq!(definition.toolchain(), "nightly-2026-07-28");
    assert_eq!(
        definition.toolchain_name(),
        "nightly-2026-07-28-x86_64-pc-windows-msvc"
    );
    assert_eq!(definition.required_components(), ["rustfmt"]);
    assert_eq!(definition.definition_signature().len(), 64);
    assert!(RustDefinition::new("1.97.1-2026-01-01", PROFILE, HOST).is_err());
    assert!(RustDefinition::new("stable", "default", HOST).is_err());
}

#[test]
fn store_validates_the_complete_toolchain_inventory_and_hashes() {
    let fixture = Fixture::new();
    let install = fixture.publish();
    let store = RustStore::new(&fixture.data_root, &fixture.definition);

    let installation = store.read_installation().unwrap();

    assert_eq!(installation.root(), install);
    assert_eq!(installation.rustc_version(), "1.97.1");
    assert_eq!(installation.cargo_version(), "1.97.1");
    let rustc = install.join(format!(
        "rustup/toolchains/{}/bin/rustc.exe",
        fixture.definition.toolchain_name()
    ));
    let mut changed = fs::read(&rustc).unwrap();
    changed[0] ^= 1;
    fs::write(rustc, changed).unwrap();
    assert_eq!(
        store.read_installation().unwrap_err().kind(),
        RustErrorKind::FileMismatch
    );
}

#[test]
fn store_rejects_unknown_metadata_and_untracked_toolchain_files() {
    let fixture = Fixture::new();
    let install = fixture.publish();
    let metadata_path = install.join(".swawkit-dev-rust.json");
    let original = fs::read(&metadata_path).unwrap();
    let mut metadata: Value = serde_json::from_slice(&original).unwrap();
    metadata["unexpected"] = json!(true);
    fs::write(&metadata_path, serde_json::to_vec(&metadata).unwrap()).unwrap();
    let store = RustStore::new(&fixture.data_root, &fixture.definition);
    assert_eq!(
        store.read_installation().unwrap_err().kind(),
        RustErrorKind::MetadataUnreadable
    );

    fs::write(&metadata_path, original).unwrap();
    let extra = install.join(format!(
        "rustup/toolchains/{}/bin/untracked.dll",
        fixture.definition.toolchain_name()
    ));
    fs::write(extra, b"untracked").unwrap();
    assert_eq!(
        store.read_installation().unwrap_err().kind(),
        RustErrorKind::InvalidInventory
    );
}

#[test]
fn installation_contributes_the_isolated_runtime_environment() {
    let fixture = Fixture::new();
    fixture.publish();
    let installation = RustStore::new(&fixture.data_root, &fixture.definition)
        .read_installation()
        .unwrap();
    let mut plan = crate::development::setup::environment::EnvironmentPlan::default();

    installation
        .add_environment(&fixture.definition, &mut plan)
        .unwrap();
    let scripts = plan.render();

    assert!(
        scripts
            .cmd()
            .contains("set \"RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc\"")
    );
    assert!(scripts.cmd().contains("set \"RUSTUP_DIST_SERVER=\""));
    assert!(
        scripts
            .cmd()
            .contains(r"\rust\installs\stable\cargo\bin;%PATH%")
    );
    assert!(
        scripts
            .ps1()
            .contains("Remove-Item -LiteralPath 'Env:RUSTUP_DIST_ROOT'")
    );
}

fn write_file(root: &Path, relative: &str, content: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn record(path: &str, content: &[u8]) -> Value {
    json!({
        "path": path,
        "kind": "file",
        "target": "",
        "length": content.len(),
        "sha256": format!("{:x}", Sha256::digest(content)),
    })
}
