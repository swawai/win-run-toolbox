use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::*;
use crate::development::archive_tool::install::InstallOutcome;
use crate::development::setup::provider::read_ready;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    data_root: PathBuf,
    cache_root: PathBuf,
    profile_revision: String,
    input_revision: String,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-archive-setup-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let data_root = root.join("data");
        let cache_root = root.join("cache");
        fs::create_dir_all(&data_root).unwrap();
        fs::create_dir_all(&cache_root).unwrap();
        let profile = b"{\"fixture\":true}\r\n";
        fs::write(data_root.join("_profile.json"), profile).unwrap();
        Self {
            root,
            data_root,
            cache_root,
            profile_revision: format!("sha256-{:x}", Sha256::digest(profile)),
            input_revision: format!("sha256-{}", "b".repeat(64)),
        }
    }

    fn context(&self) -> ArchiveSetupContext {
        ArchiveSetupContext::new(
            &self.data_root,
            &self.cache_root,
            &self.profile_revision,
            &self.input_revision,
        )
        .unwrap()
    }

    fn publish_install(&self, tool: &'static ArchiveToolContract, version: &str) -> PathBuf {
        let root = self
            .data_root
            .join("modules/kernel/.dev/setup/export")
            .join(tool.name)
            .join("installs")
            .join(version);
        fs::create_dir_all(&root).unwrap();
        let files = tool
            .required_paths
            .iter()
            .enumerate()
            .map(|(index, relative)| {
                let content = format!("{}-{version}-{index}\r\n", tool.name).into_bytes();
                fs::write(root.join(relative), &content).unwrap();
                json!({
                    "path": relative,
                    "length": content.len(),
                    "sha256": sha256(&content),
                })
            })
            .collect::<Vec<Value>>();
        let metadata = json!({
            "schema": "swawkit.proj-dev.install.v0",
            "name": tool.name,
            "version": version,
            "sourceUrl": tool.release_coordinates(version).download_url,
            "sourceSha256": "a".repeat(64),
            "sourceVerification": "unverified",
            "recipeVersion": tool.recipe_version,
            "definitionSignature": tool.definition_signature(version, ""),
            "files": files,
        });
        fs::write(
            root.join(".swawkit-dev-install.json"),
            serde_json::to_vec_pretty(&metadata).unwrap(),
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
fn archive_only_setup_is_offline_and_publishes_one_ready_environment() {
    let fixture = Fixture::new();
    let bun = fixture.publish_install(&BUN, "1.2.15");
    let pwsh = fixture.publish_install(&PWSH, "7.6.4");
    let declarations = declarations(&[
        ("SWAWKIT_PROJ_BUN_MODE", "managed"),
        ("SWAWKIT_PROJ_BUN_VERSION", "1.2.15"),
        ("SWAWKIT_PROJ_BUN_SHA256", ""),
        ("SWAWKIT_PROJ_PWSH_MODE", "managed"),
        ("SWAWKIT_PROJ_PWSH_VERSION", "7.6.4"),
        ("SWAWKIT_PROJ_PWSH_SHA256", ""),
    ]);
    let mut progress = Vec::new();

    let result = run_archive_only(
        &fixture.context(),
        &declarations,
        &mut |tool, current, total| {
            progress.push((tool.to_owned(), current, total));
        },
    )
    .unwrap();

    assert!(
        progress.is_empty(),
        "ready installs must not touch the network"
    );
    assert_eq!(
        result
            .tools()
            .iter()
            .map(|tool| (tool.name(), tool.outcome()))
            .collect::<Vec<_>>(),
        [
            ("bun", InstallOutcome::Ready),
            ("pwsh", InstallOutcome::Ready)
        ]
    );
    assert_eq!(result.tools()[0].root(), bun);
    assert_eq!(result.tools()[1].root(), pwsh);
    let ready = read_ready(&fixture.data_root, &fixture.input_revision).unwrap();
    let cmd = fs::read_to_string(
        fixture
            .data_root
            .join("modules/kernel/.dev/setup/export/env.cmd"),
    )
    .unwrap();
    assert!(cmd.contains(ready.token()));
    assert!(
        cmd.find(r"\bun\installs\1.2.15").unwrap() < cmd.find(r"\pwsh\installs\7.6.4").unwrap(),
        "{cmd}"
    );
    let ps1 = fs::read(
        fixture
            .data_root
            .join("modules/kernel/.dev/setup/export/env.ps1"),
    )
    .unwrap();
    assert_eq!(&ps1[..3], &[0xef, 0xbb, 0xbf]);

    let repeated = run_archive_only(&fixture.context(), &declarations, &mut |_, _, _| {}).unwrap();
    assert!(
        repeated
            .tools()
            .iter()
            .all(|tool| tool.outcome() == InstallOutcome::Ready)
    );
}

#[test]
fn unsupported_enabled_domain_leaves_the_provider_unavailable() {
    let fixture = Fixture::new();
    let declarations = declarations(&[
        ("SWAWKIT_PROJ_MSVC_MODE", "managed"),
        ("SWAWKIT_PROJ_MSVC_CHANNEL", "17"),
    ]);

    let error = run_archive_only(&fixture.context(), &declarations, &mut |_, _, _| {}).unwrap_err();

    assert!(error.contains("native archive setup"), "{error}");
    assert!(read_ready(&fixture.data_root, &fixture.input_revision).is_err());
    let state: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .data_root
                .join("modules/kernel/.dev/setup/_state.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(state["status"], "unavailable");
    assert!(
        !fixture
            .data_root
            .join("modules/kernel/.dev/setup/export/env.cmd")
            .exists()
    );
}

fn declarations(values: &[(&'static str, &'static str)]) -> DeclarationSnapshot {
    let values = values.iter().copied().collect::<BTreeMap<_, _>>();
    crate::development::setup::declaration::snapshot(|name| {
        values.get(name).map(|value| (*value).to_owned())
    })
}

fn sha256(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}
