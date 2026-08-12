use super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::development::msvc::{InstalledFile, MsvcMetadata, required_paths};
use sha2::{Digest, Sha256};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct TemporaryRoot(PathBuf);

impl Drop for TemporaryRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn install_context_requires_absolute_roots() {
    assert!(MsvcInstallContext::new(Path::new("data"), Path::new("cache")).is_err());
}

#[test]
fn a_ready_installation_is_returned_without_resolving_microsoft_sources() {
    let root = std::env::temp_dir().join(format!(
        "swawkit-msvc-ready-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let data_root = root.join("data");
    let cache_root = root.join("cache");
    fs::create_dir_all(&data_root).unwrap();
    fs::create_dir_all(&cache_root).unwrap();
    let definition = MsvcDefinition::new("17").unwrap();
    let target = data_root.join("modules/kernel/.dev/setup/export/msvc/installs/17");
    fs::create_dir_all(&target).unwrap();
    let tool = "14.44.35228";
    let sdk = "10.0.26100.0";
    let mut files = Vec::new();
    for (index, relative) in required_paths(tool, sdk).into_iter().enumerate() {
        let content = format!("ready-{index}").into_bytes();
        let path = target.join(&relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &content).unwrap();
        files.push(InstalledFile {
            path: relative,
            length: content.len() as u64,
            sha256: format!("{:x}", Sha256::digest(&content)),
        });
    }
    let metadata = MsvcMetadata {
        schema: super::super::INSTALL_SCHEMA.to_owned(),
        name: "msvc".to_owned(),
        channel: "17".to_owned(),
        channel_url: definition.channel_url(),
        recipe_version: super::super::RECIPE_VERSION.to_owned(),
        definition_signature: definition.definition_signature(),
        manifest_url: "https://download.visualstudio.microsoft.com/fixture.vsman".to_owned(),
        manifest_sha256: "a".repeat(64),
        tool_package_version: "14.44.17.14".to_owned(),
        tool_version: tool.to_owned(),
        sdk_package: "Win11SDK_fixture".to_owned(),
        sdk_version: sdk.to_owned(),
        source_verification: "microsoft-manifest".to_owned(),
        files,
    };
    fs::write(
        target.join(".swawkit-dev-msvc.json"),
        serde_json::to_vec_pretty(&metadata).unwrap(),
    )
    .unwrap();

    let result = ensure_installed(
        MsvcInstallContext::new(&data_root, &cache_root).unwrap(),
        &definition,
        &mut |_, _, _| panic!("a ready installation must stay offline"),
    )
    .unwrap();

    assert_eq!(result.outcome(), MsvcInstallOutcome::Ready);
    assert!(result.warnings().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
#[ignore = "uses the real Microsoft package cache and runs eight MSI administrative installs"]
fn native_installer_accepts_the_real_microsoft_package_set() {
    let cache_root = std::env::var_os("SWAWKIT_MSVC_E2E_CACHE_ROOT")
        .map(PathBuf::from)
        .expect("SWAWKIT_MSVC_E2E_CACHE_ROOT must identify a prepared cache data root");
    let root = TemporaryRoot(std::env::temp_dir().join(format!(
        "swawkit-msvc-native-e2e-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    let data_root = root.0.join("data");
    fs::create_dir_all(&data_root).unwrap();
    let definition = MsvcDefinition::new("17").unwrap();

    let result = ensure_installed(
        MsvcInstallContext::new(&data_root, &cache_root).unwrap(),
        &definition,
        &mut |_, _, _| {},
    )
    .unwrap();

    assert_eq!(result.outcome(), MsvcInstallOutcome::Installed);
    assert!(result.installation().root().join("setup_x64.bat").is_file());
}
