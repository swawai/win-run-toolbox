use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::json;
use sha2::{Digest, Sha256};

use super::*;
use crate::development::rust::{HOST, PROFILE, RUSTUP_URL};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    definition: RustDefinition,
}

struct TemporaryRoot(PathBuf);

impl Drop for TemporaryRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "swawkit-rust-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let data = root.join("data");
        let cache = root.join("cache");
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&cache).unwrap();
        Self {
            root,
            data,
            cache,
            definition: RustDefinition::new("stable", PROFILE, HOST).unwrap(),
        }
    }

    fn publish(&self, root: &Path) {
        let mut files = Vec::new();
        for (index, relative) in self.definition.required_paths().into_iter().enumerate() {
            let content = if relative == "cargo\\bin\\rustup.exe" {
                b"rustup-fixture".to_vec()
            } else {
                format!("rust-{index}").into_bytes()
            };
            let path = root.join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &content).unwrap();
            files.push(json!({
                "path": relative,
                "kind": "file",
                "target": "",
                "length": content.len(),
                "sha256": digest(&content),
            }));
        }
        let extra = format!(
            "rustup\\toolchains\\{}\\lib\\rustlib\\{}\\lib\\libstd.rlib",
            self.definition.toolchain_name(),
            HOST
        );
        let extra_path = root.join(&extra);
        fs::create_dir_all(extra_path.parent().unwrap()).unwrap();
        fs::write(&extra_path, b"std").unwrap();
        files.push(json!({
            "path": extra,
            "kind": "file",
            "target": "",
            "length": 3,
            "sha256": digest(b"std"),
        }));
        files.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
        let rustup_hash = digest(b"rustup-fixture");
        let metadata = json!({
            "schema": "swawkit.proj-dev.rust-install.v0",
            "name": "rust",
            "inventory": "toolchain-files-v0",
            "declaredToolchain": "stable",
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
            "rustfmtVersion": "1.9.0-stable",
            "sourceVerification": "rust-static-sha256",
            "files": files,
        });
        fs::write(
            root.join(".swawkit-dev-rust.json"),
            serde_json::to_vec_pretty(&metadata).unwrap(),
        )
        .unwrap();
    }

    fn target(&self) -> PathBuf {
        self.data
            .join("modules/kernel/.dev/setup/export/rust/installs/stable")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn ready_installation_is_offline() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.target()).unwrap();
    fixture.publish(&fixture.target());

    let result = ensure_installed(
        RustInstallContext::new(&fixture.data, &fixture.cache).unwrap(),
        &fixture.definition,
        &mut |_, _| panic!("ready Rust must not download"),
    )
    .unwrap();

    assert_eq!(result.outcome(), RustInstallOutcome::Ready);
    assert_eq!(result.installation().root(), fixture.target());
}

#[test]
fn valid_backup_is_recovered_without_source_access() {
    let fixture = Fixture::new();
    let target = fixture.target();
    let parent = target.parent().unwrap();
    fs::create_dir_all(parent).unwrap();
    let backup = parent.join("stable.backup-20260812T0102030000000Z-fixture");
    fs::create_dir(&backup).unwrap();
    fixture.publish(&backup);

    let result = ensure_installed(
        RustInstallContext::new(&fixture.data, &fixture.cache).unwrap(),
        &fixture.definition,
        &mut |_, _| panic!("recovered Rust must not download"),
    )
    .unwrap();

    assert_eq!(result.outcome(), RustInstallOutcome::Recovered);
    assert_eq!(result.installation().root(), fixture.target());
    assert!(!backup.exists());
}

#[test]
fn invalid_target_is_removed_by_offline_recovery() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.target()).unwrap();
    fs::write(fixture.target().join("corrupt.txt"), b"corrupt").unwrap();
    let store = RustStore::new(&fixture.data, &fixture.definition);
    let mut transaction =
        InstallationTransaction::open(&fixture.data, "rust", fixture.target(), |root| {
            candidate(&store, root)
        })
        .unwrap();

    let (installation, _, _) = transaction.recover().unwrap();

    assert!(installation.is_none());
    assert!(!fixture.target().exists());
}

#[test]
fn recovery_removes_only_owned_rustup_proxy_links() {
    let fixture = Fixture::new();
    let target = fixture.target();
    let bin = target.join("cargo/bin");
    fs::create_dir_all(&bin).unwrap();
    fs::write(bin.join("rustup.exe"), b"rustup").unwrap();
    if let Err(cause) =
        std::os::windows::fs::symlink_file("rustup.exe", bin.join("future-rustup-proxy.exe"))
    {
        if cause.kind() == std::io::ErrorKind::PermissionDenied {
            return;
        }
        panic!("create rustup proxy: {cause}");
    }
    let store = RustStore::new(&fixture.data, &fixture.definition);
    let mut transaction = InstallationTransaction::open_with_removal(
        &fixture.data,
        "rust",
        target.clone(),
        |root| candidate(&store, root),
        prepare_rust_removal,
    )
    .unwrap();

    let (installation, _, _) = transaction.recover().unwrap();

    assert!(installation.is_none());
    assert!(!target.exists());
}

#[test]
fn recovery_refuses_unowned_links_without_partial_deletion() {
    let fixture = Fixture::new();
    let target = fixture.target();
    let external = fixture.root.join("external.txt");
    fs::create_dir_all(&target).unwrap();
    fs::write(&external, b"external").unwrap();
    let ordinary = target.join("ordinary.txt");
    fs::write(&ordinary, b"ordinary").unwrap();
    if let Err(cause) = std::os::windows::fs::symlink_file(&external, target.join("unknown.exe")) {
        if cause.kind() == std::io::ErrorKind::PermissionDenied {
            return;
        }
        panic!("create unowned link: {cause}");
    }
    let store = RustStore::new(&fixture.data, &fixture.definition);
    let mut transaction = InstallationTransaction::open_with_removal(
        &fixture.data,
        "rust",
        target,
        |root| candidate(&store, root),
        prepare_rust_removal,
    )
    .unwrap();

    let failure = transaction.recover().err().expect("unsafe link must fail");

    assert_eq!(failure.kind(), ArchiveToolErrorKind::UnsafeStorage);
    assert!(ordinary.is_file());
    assert_eq!(fs::read(external).unwrap(), b"external");
}

#[test]
#[ignore = "downloads the declared Rust toolchain and runs rustup-init"]
fn native_installer_accepts_the_real_rustup_distribution() {
    let cache = std::env::var_os("SWAWKIT_RUST_E2E_CACHE_ROOT")
        .map(PathBuf::from)
        .expect("SWAWKIT_RUST_E2E_CACHE_ROOT must identify a cache data root");
    let root = TemporaryRoot(std::env::temp_dir().join(format!(
        "swawkit-rust-native-e2e-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    let data = root.0.join("data");
    fs::create_dir_all(&data).unwrap();
    let definition = RustDefinition::new("1.97.1", PROFILE, HOST).unwrap();

    let result = ensure_installed(
        RustInstallContext::new(&data, &cache).unwrap(),
        &definition,
        &mut |_, _| {},
    )
    .unwrap();

    assert_eq!(result.outcome(), RustInstallOutcome::Installed);
    assert_eq!(result.installation().rustc_version(), "1.97.1");
    assert!(
        result
            .installation()
            .root()
            .join("cargo/bin/rustup.exe")
            .is_file()
    );
}

fn digest(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}
