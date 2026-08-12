use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::*;
use crate::development::archive_tool::install::InstallOutcome;
use crate::development::msvc::MsvcInstallOutcome;
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

    fn context(&self) -> NativeSetupContext {
        NativeSetupContext::new(
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

    fn publish_msvc(&self) -> PathBuf {
        let tool = "14.44.35228";
        let sdk = "10.0.26100.0";
        let root = self
            .data_root
            .join("modules/kernel/.dev/setup/export/msvc/installs/17");
        fs::create_dir_all(&root).unwrap();
        let mut files = Vec::new();
        for (index, relative) in msvc_required_paths(tool, sdk).into_iter().enumerate() {
            let content = format!("msvc-ready-{index}").into_bytes();
            let path = root.join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &content).unwrap();
            files.push(json!({
                "path": relative,
                "length": content.len(),
                "sha256": sha256(&content),
            }));
        }
        let metadata = json!({
            "schema": "swawkit.proj-dev.msvc-install.v0",
            "name": "msvc",
            "channel": "17",
            "channelUrl": "https://aka.ms/vs/17/release/channel",
            "recipeVersion": "1",
            "definitionSignature": "4597e8b291e1b50b7bec65f02b99df7dac9ef2e495d1931c6e0ea9f34b31d5a8",
            "manifestUrl": "https://download.visualstudio.microsoft.com/fixture.vsman",
            "manifestSha256": "a".repeat(64),
            "toolPackageVersion": "14.44.17.14",
            "toolVersion": tool,
            "sdkPackage": "Win11SDK_fixture",
            "sdkVersion": sdk,
            "sourceVerification": "microsoft-manifest",
            "files": files,
        });
        fs::write(
            root.join(".swawkit-dev-msvc.json"),
            serde_json::to_vec_pretty(&metadata).unwrap(),
        )
        .unwrap();
        root
    }

    fn publish_rust(&self) -> PathBuf {
        let definition = crate::development::rust::RustDefinition::new(
            "stable",
            "minimal",
            "x86_64-pc-windows-msvc",
        )
        .unwrap();
        let root = self
            .data_root
            .join("modules/kernel/.dev/setup/export/rust/installs/stable");
        fs::create_dir_all(&root).unwrap();
        let mut files = Vec::new();
        for (index, relative) in definition.required_paths().into_iter().enumerate() {
            let content = if relative == "cargo\\bin\\rustup.exe" {
                b"rustup-native-fixture".to_vec()
            } else {
                format!("rust-native-{index}").into_bytes()
            };
            let path = root.join(&relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, &content).unwrap();
            files.push(json!({
                "path": relative,
                "kind": "file",
                "target": "",
                "length": content.len(),
                "sha256": sha256(&content),
            }));
        }
        let extra = format!(
            "rustup\\toolchains\\{}\\lib\\rustlib\\x86_64-pc-windows-msvc\\lib\\libstd.rlib",
            definition.toolchain_name()
        );
        let extra_path = root.join(&extra);
        fs::create_dir_all(extra_path.parent().unwrap()).unwrap();
        fs::write(extra_path, b"std").unwrap();
        files.push(json!({
            "path": extra,
            "kind": "file",
            "target": "",
            "length": 3,
            "sha256": sha256(b"std"),
        }));
        files.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
        let metadata = json!({
            "schema": "swawkit.proj-dev.rust-install.v0",
            "name": "rust",
            "inventory": "toolchain-files-v0",
            "declaredToolchain": "stable",
            "toolchainName": definition.toolchain_name(),
            "profile": "minimal",
            "host": "x86_64-pc-windows-msvc",
            "components": ["rustfmt"],
            "recipeVersion": "2",
            "definitionSignature": definition.definition_signature(),
            "rustupInitUrl": crate::development::rust::RUSTUP_URL,
            "rustupInitSha256": sha256(b"rustup-native-fixture"),
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
        root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn native_setup_is_offline_and_publishes_one_ready_environment() {
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

    let result = run_native(
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
            .archive_tools()
            .iter()
            .map(|tool| (tool.name(), tool.outcome()))
            .collect::<Vec<_>>(),
        [
            ("bun", InstallOutcome::Ready),
            ("pwsh", InstallOutcome::Ready)
        ]
    );
    assert_eq!(result.archive_tools()[0].root(), bun);
    assert_eq!(result.archive_tools()[1].root(), pwsh);
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

    let repeated = run_native(&fixture.context(), &declarations, &mut |_, _, _| {}).unwrap();
    assert!(
        repeated
            .archive_tools()
            .iter()
            .all(|tool| tool.outcome() == InstallOutcome::Ready)
    );
}

#[test]
fn invalid_rust_declaration_leaves_the_provider_unavailable() {
    let fixture = Fixture::new();
    let declarations = declarations(&[
        ("SWAWKIT_PROJ_RUST_MODE", "rustup"),
        ("SWAWKIT_PROJ_RUST_TOOLCHAIN", "invalid/toolchain"),
        ("SWAWKIT_PROJ_RUST_PROFILE", "minimal"),
        ("SWAWKIT_PROJ_RUST_HOST", "x86_64-pc-windows-msvc"),
    ]);

    let error = run_native(&fixture.context(), &declarations, &mut |_, _, _| {}).unwrap_err();

    assert!(error.contains("RUST_TOOLCHAIN"), "{error}");
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

#[test]
fn ready_rust_joins_the_shared_environment_and_provider_transaction() {
    let fixture = Fixture::new();
    let root = fixture.publish_rust();
    let declarations = declarations(&[
        ("SWAWKIT_PROJ_RUST_MODE", "rustup"),
        ("SWAWKIT_PROJ_RUST_TOOLCHAIN", "stable"),
        ("SWAWKIT_PROJ_RUST_PROFILE", "minimal"),
        ("SWAWKIT_PROJ_RUST_HOST", "x86_64-pc-windows-msvc"),
    ]);

    let result = run_native(&fixture.context(), &declarations, &mut |_, _, _| {
        panic!("ready Rust must remain offline")
    })
    .unwrap();

    let rust = result.rust().unwrap();
    assert_eq!(rust.toolchain(), "stable");
    assert_eq!(rust.root(), root);
    assert_eq!(
        rust.outcome(),
        crate::development::rust::RustInstallOutcome::Ready
    );
    let cmd = fs::read_to_string(
        fixture
            .data_root
            .join("modules/kernel/.dev/setup/export/env.cmd"),
    )
    .unwrap();
    assert!(cmd.contains("set \"RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc\""));
    assert!(cmd.contains(r"\rust\installs\stable\cargo\bin"));
    assert!(read_ready(&fixture.data_root, &fixture.input_revision).is_ok());
}

#[test]
fn every_enabled_definition_is_preflighted_before_any_tool_is_touched() {
    let fixture = Fixture::new();
    fixture.publish_install(&BUN, "1.2.15");
    let declarations = declarations(&[
        ("SWAWKIT_PROJ_BUN_MODE", "managed"),
        ("SWAWKIT_PROJ_BUN_VERSION", "1.2.15"),
        ("SWAWKIT_PROJ_BUN_SHA256", ""),
        ("SWAWKIT_PROJ_MSVC_MODE", "managed"),
        ("SWAWKIT_PROJ_MSVC_CHANNEL", "invalid"),
    ]);
    let env = fixture
        .data_root
        .join("modules/kernel/.dev/setup/export/env.cmd");

    let error = run_native(&fixture.context(), &declarations, &mut |_, _, _| {})
        .expect_err("invalid MSVC must fail the shared declaration preflight");

    assert!(error.contains("numeric VS channel"), "{error}");
    assert!(!env.exists());
    assert!(read_ready(&fixture.data_root, &fixture.input_revision).is_err());
}

#[test]
fn ready_msvc_joins_the_same_provider_and_environment_transaction() {
    let fixture = Fixture::new();
    fixture.publish_install(&BUN, "1.2.15");
    fixture.publish_install(&PWSH, "7.6.4");
    let root = fixture.publish_msvc();
    let declarations = declarations(&[
        ("SWAWKIT_PROJ_BUN_MODE", "managed"),
        ("SWAWKIT_PROJ_BUN_VERSION", "1.2.15"),
        ("SWAWKIT_PROJ_BUN_SHA256", ""),
        ("SWAWKIT_PROJ_PWSH_MODE", "managed"),
        ("SWAWKIT_PROJ_PWSH_VERSION", "7.6.4"),
        ("SWAWKIT_PROJ_PWSH_SHA256", ""),
        ("SWAWKIT_PROJ_MSVC_MODE", "managed"),
        ("SWAWKIT_PROJ_MSVC_CHANNEL", "17"),
    ]);
    let mut progress = Vec::new();

    let result = run_native(
        &fixture.context(),
        &declarations,
        &mut |tool, current, total| progress.push((tool.to_owned(), current, total)),
    )
    .unwrap();

    assert!(progress.is_empty(), "ready MSVC must remain offline");
    assert_eq!(result.archive_tools().len(), 2);
    let msvc = result.msvc().unwrap();
    assert_eq!(msvc.channel(), "17");
    assert_eq!(msvc.tool_version(), "14.44.35228");
    assert_eq!(msvc.sdk_version(), "10.0.26100.0");
    assert_eq!(msvc.root(), root);
    assert_eq!(msvc.outcome(), MsvcInstallOutcome::Ready);
    let ready = read_ready(&fixture.data_root, &fixture.input_revision).unwrap();
    let cmd = fs::read_to_string(
        fixture
            .data_root
            .join("modules/kernel/.dev/setup/export/env.cmd"),
    )
    .unwrap();
    assert!(cmd.contains(ready.token()));
    assert!(cmd.contains("set \"VSCMD_ARG_HOST_ARCH=x64\""));
    assert!(cmd.contains(r"VC\Tools\MSVC\14.44.35228\bin\Hostx64\x64"));
    assert!(cmd.contains(r"Windows Kits\10\bin\10.0.26100.0\x64"));
    let bun = cmd.find(r"\bun\installs\1.2.15").unwrap();
    let pwsh = cmd.find(r"\pwsh\installs\7.6.4").unwrap();
    let msvc = cmd
        .find(r"VC\Tools\MSVC\14.44.35228\bin\Hostx64\x64")
        .unwrap();
    assert!(bun < pwsh && pwsh < msvc, "{cmd}");
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

fn msvc_required_paths(tool: &str, sdk: &str) -> Vec<String> {
    [
        "setup_x64.bat".to_owned(),
        format!(r"VC\Tools\MSVC\{tool}\bin\Hostx64\x64\cl.exe"),
        format!(r"VC\Tools\MSVC\{tool}\bin\Hostx64\x64\link.exe"),
        format!(r"VC\Tools\MSVC\{tool}\bin\Hostx64\x64\lib.exe"),
        format!(r"VC\Tools\MSVC\{tool}\bin\Hostx64\x64\msdia140.dll"),
        format!(r"VC\Tools\MSVC\{tool}\include\yvals_core.h"),
        format!(r"Windows Kits\10\bin\{sdk}\x64\rc.exe"),
        format!(r"Windows Kits\10\Include\{sdk}\ucrt\stdio.h"),
        format!(r"Windows Kits\10\Include\{sdk}\um\windows.h"),
        format!(r"Windows Kits\10\Lib\{sdk}\ucrt\x64\ucrt.lib"),
        format!(r"Windows Kits\10\Lib\{sdk}\um\x64\kernel32.lib"),
    ]
    .into_iter()
    .collect()
}
