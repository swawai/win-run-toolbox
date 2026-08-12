use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::*;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    data_root: PathBuf,
    definition: MsvcDefinition,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "swawkit-msvc-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        let data_root = root.join("data");
        fs::create_dir_all(&data_root).unwrap();
        Self {
            root,
            data_root,
            definition: MsvcDefinition::new("17").unwrap(),
        }
    }

    fn publish(&self) -> PathBuf {
        let tool_version = "14.44.35228";
        let sdk_version = "10.0.26100.0";
        let root = self
            .data_root
            .join("modules/kernel/.dev/setup/export/msvc/installs/17");
        fs::create_dir_all(&root).unwrap();
        let files = required_paths(tool_version, sdk_version)
            .into_iter()
            .enumerate()
            .map(|(index, relative)| {
                let content = format!("fixture-{index}").into_bytes();
                let path = root.join(&relative);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, &content).unwrap();
                json!({
                    "path": relative,
                    "length": content.len(),
                    "sha256": format!("{:x}", Sha256::digest(&content)),
                })
            })
            .collect::<Vec<Value>>();
        fs::write(
            root.join(".swawkit-dev-msvc.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": INSTALL_SCHEMA,
                "name": "msvc",
                "channel": "17",
                "channelUrl": self.definition.channel_url(),
                "recipeVersion": RECIPE_VERSION,
                "definitionSignature": self.definition.definition_signature(),
                "manifestUrl": "https://download.visualstudio.microsoft.com/fixture.vsman",
                "manifestSha256": "a".repeat(64),
                "toolPackageVersion": "14.44.17.14",
                "toolVersion": tool_version,
                "sdkPackage": "Win11SDK_10",
                "sdkVersion": sdk_version,
                "sourceVerification": "microsoft-manifest",
                "files": files,
            }))
            .unwrap(),
        )
        .unwrap();
        root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn definition_and_metadata_match_the_power_shell_contract() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.definition.channel_url(),
        "https://aka.ms/vs/17/release/channel"
    );
    assert_eq!(
        fixture.definition.definition_signature(),
        "4597e8b291e1b50b7bec65f02b99df7dac9ef2e495d1931c6e0ea9f34b31d5a8"
    );
    let root = fixture.publish();
    let store = MsvcStore::new(&fixture.data_root, &fixture.definition);
    let installation = store.read_installation().unwrap();
    assert_eq!(installation.root(), root);
    assert_eq!(installation.tool_version(), "14.44.35228");
    assert_eq!(installation.sdk_version(), "10.0.26100.0");
}

#[test]
fn environment_layout_matches_the_power_shell_contract() {
    let fixture = Fixture::new();
    fixture.publish();
    let installation = MsvcStore::new(&fixture.data_root, &fixture.definition)
        .read_installation()
        .unwrap();
    let mut plan = crate::development::setup::environment::EnvironmentPlan::default();

    installation.add_environment(&mut plan).unwrap();
    let scripts = plan.render();

    assert!(scripts.cmd().contains("set \"VSCMD_ARG_HOST_ARCH=x64\""));
    assert!(
        scripts
            .cmd()
            .contains("set \"WindowsSDKVersion=10.0.26100.0\\\"")
    );
    assert!(
        scripts
            .cmd()
            .contains("VC\\Tools\\MSVC\\14.44.35228\\include")
    );
    assert!(
        scripts
            .cmd()
            .contains("Windows Kits\\10\\Lib\\10.0.26100.0\\um\\x64")
    );
    assert!(scripts.cmd().contains("bin\\Hostx64\\x64;"));
}

#[test]
fn metadata_shape_and_every_required_hash_fail_closed() {
    let fixture = Fixture::new();
    let root = fixture.publish();
    let store = MsvcStore::new(&fixture.data_root, &fixture.definition);
    let metadata_path = root.join(".swawkit-dev-msvc.json");
    let baseline = fs::read(&metadata_path).unwrap();
    let mut extended: Value = serde_json::from_slice(&baseline).unwrap();
    extended["unexpected"] = json!(true);
    fs::write(
        &metadata_path,
        serde_json::to_vec_pretty(&extended).unwrap(),
    )
    .unwrap();
    assert_eq!(
        store.read_installation().unwrap_err().kind(),
        MsvcErrorKind::MetadataUnreadable
    );
    fs::write(&metadata_path, baseline).unwrap();
    let target = root.join(required_paths("14.44.35228", "10.0.26100.0")[1].as_str());
    let bytes = fs::read(&target).unwrap();
    fs::write(&target, vec![b'x'; bytes.len()]).unwrap();
    assert_eq!(
        store.read_installation().unwrap_err().kind(),
        MsvcErrorKind::FileMismatch
    );
}

#[test]
fn metadata_must_belong_to_the_requested_definition() {
    let fixture = Fixture::new();
    let root = fixture.publish();
    let other = MsvcDefinition::new("18").unwrap();
    let other_root = fixture
        .data_root
        .join("modules/kernel/.dev/setup/export/msvc/installs/18");
    fs::create_dir_all(other_root.parent().unwrap()).unwrap();
    fs::rename(root, &other_root).unwrap();

    assert_eq!(
        MsvcStore::new(&fixture.data_root, &other)
            .read_installation()
            .unwrap_err()
            .kind(),
        MsvcErrorKind::MetadataStale
    );
}

#[test]
fn non_numeric_channels_are_rejected() {
    assert!(MsvcDefinition::new("preview").is_err());
    assert_eq!(MsvcDefinition::new(" 17 ").unwrap().channel(), "17");
}
