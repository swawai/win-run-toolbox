use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::*;
use crate::development::{BUN, PWSH};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    data_root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("workspace root")
            .join("data/proj_cache/tests/archive-tool")
            .join(format!("{}-{sequence}", std::process::id()));
        let data_root = root.join("data");
        fs::create_dir_all(&data_root).expect("create fixture DataRoot");
        Self { root, data_root }
    }

    fn store(&self) -> ArchiveToolStore<'_> {
        ArchiveToolStore::new(&self.data_root, &BUN)
    }

    fn publish_selection(&self, extra: Option<(&str, Value)>) {
        let mut value = json!({
            "schema": BUN.selection_schema,
            "selector": "latest",
            "version": "1.3.14",
            "sourceSha256": "a".repeat(64),
            "sourceVerification": "unverified"
        });
        if let Some((name, field)) = extra {
            value
                .as_object_mut()
                .unwrap()
                .insert(name.to_owned(), field);
        }
        write_json(
            &self.tool_root().join(".swawkit-dev-selection.json"),
            &value,
        );
    }

    fn publish_install(&self, resolved: &ResolvedDefinition) -> PathBuf {
        let root = self.tool_root().join("installs").join(resolved.version());
        fs::create_dir_all(&root).expect("create installation");
        let bun = b"fixture-bun";
        let bunx = b"@echo off\r\nfixture\r\n";
        fs::write(root.join("bun.exe"), bun).expect("write Bun executable");
        fs::write(root.join("bunx.cmd"), bunx).expect("write Bunx shim");
        let verification = match resolved.verification() {
            ResolvedVerification::Published(value) => value.as_str(),
            ResolvedVerification::Unresolved => "unverified",
        };
        let source_sha256 = resolved
            .source_sha256()
            .map(str::to_owned)
            .unwrap_or_else(|| "a".repeat(64));
        write_json(
            &root.join(".swawkit-dev-install.json"),
            &json!({
                "schema": INSTALL_SCHEMA,
                "name": BUN.name,
                "version": resolved.version(),
                "sourceUrl": "https://example.invalid/bun.zip",
                "sourceSha256": source_sha256,
                "sourceVerification": verification,
                "recipeVersion": BUN.recipe_version,
                "definitionSignature": BUN.definition_signature(
                    resolved.version(),
                    resolved.project_sha256(),
                ),
                "files": [
                    { "path": "bun.exe", "length": bun.len(), "sha256": sha256(bun) },
                    { "path": "bunx.cmd", "length": bunx.len(), "sha256": sha256(bunx) }
                ]
            }),
        );
        root
    }

    fn tool_root(&self) -> PathBuf {
        self.data_root.join("modules/kernel/.dev/setup/export/bun")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn exact_resolution_does_not_require_a_published_export() {
    let fixture = Fixture::new();
    let request = ArchiveToolRequest::new(&BUN, "1.2.3", "").unwrap();
    let resolved = fixture.store().resolve(&request).unwrap().unwrap();

    assert_eq!(resolved.version(), "1.2.3");
    assert_eq!(resolved.verification(), ResolvedVerification::Unresolved);
}

#[test]
fn latest_missing_is_none_but_invalid_or_extended_selection_is_an_error() {
    let fixture = Fixture::new();
    let request = ArchiveToolRequest::new(&BUN, "latest", "").unwrap();
    assert!(fixture.store().resolve(&request).unwrap().is_none());

    fixture.publish_selection(Some(("unexpected", json!(true))));
    let error = fixture.store().resolve(&request).unwrap_err();
    assert_eq!(error.kind(), ArchiveToolErrorKind::SelectionUnreadable);

    fixture.publish_selection(None);
    let path = fixture.tool_root().join(".swawkit-dev-selection.json");
    let mut selection: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    selection["sourceVerification"] = json!("project");
    write_json(&path, &selection);
    let error = fixture.store().resolve(&request).unwrap_err();
    assert_eq!(error.kind(), ArchiveToolErrorKind::SelectionInvalid);
}

#[test]
fn installation_length_and_hash_are_separate_shared_checks() {
    let fixture = Fixture::new();
    let request = ArchiveToolRequest::new(&BUN, "1.2.3", "").unwrap();
    let resolved = fixture.store().resolve(&request).unwrap().unwrap();
    let root = fixture.publish_install(&resolved);

    let installation = fixture.store().read_installation(&resolved).unwrap();
    fixture.store().verify_hashes(&installation).unwrap();
    fs::write(root.join("bun.exe"), b"tampered-bu").expect("same-length tamper");
    fixture.store().read_installation(&resolved).unwrap();
    let error = fixture.store().verify_hashes(&installation).unwrap_err();
    assert_eq!(error.kind(), ArchiveToolErrorKind::InstalledFileInvalid);
}

#[test]
fn metadata_requires_a_trimmed_source_url_and_nonempty_files() {
    let fixture = Fixture::new();
    let request = ArchiveToolRequest::new(&BUN, "1.2.3", "").unwrap();
    let resolved = fixture.store().resolve(&request).unwrap().unwrap();
    let root = fixture.publish_install(&resolved);
    let metadata_path = root.join(".swawkit-dev-install.json");
    let baseline: Value = serde_json::from_slice(&fs::read(&metadata_path).unwrap()).unwrap();

    let mut unknown_metadata = baseline.clone();
    unknown_metadata["unexpected"] = json!(true);
    write_json(&metadata_path, &unknown_metadata);
    assert_eq!(
        fixture
            .store()
            .read_installation(&resolved)
            .unwrap_err()
            .kind(),
        ArchiveToolErrorKind::MetadataUnreadable
    );

    let mut unknown_file_field = baseline.clone();
    unknown_file_field["files"][0]["unexpected"] = json!(true);
    write_json(&metadata_path, &unknown_file_field);
    assert_eq!(
        fixture
            .store()
            .read_installation(&resolved)
            .unwrap_err()
            .kind(),
        ArchiveToolErrorKind::MetadataUnreadable
    );

    let mut missing_url = baseline.clone();
    missing_url.as_object_mut().unwrap().remove("sourceUrl");
    write_json(&metadata_path, &missing_url);
    assert_eq!(
        fixture
            .store()
            .read_installation(&resolved)
            .unwrap_err()
            .kind(),
        ArchiveToolErrorKind::MetadataUnreadable
    );

    let mut invalid_url = baseline.clone();
    invalid_url["sourceUrl"] = json!(" https://example.invalid/bun.zip");
    write_json(&metadata_path, &invalid_url);
    assert_eq!(
        fixture
            .store()
            .read_installation(&resolved)
            .unwrap_err()
            .kind(),
        ArchiveToolErrorKind::MetadataStale
    );

    let mut empty_file = baseline;
    empty_file["files"][0]["length"] = json!(0);
    write_json(&metadata_path, &empty_file);
    assert_eq!(
        fixture
            .store()
            .read_installation(&resolved)
            .unwrap_err()
            .kind(),
        ArchiveToolErrorKind::InvalidFileRecord
    );
}

#[test]
fn source_verification_must_match_the_resolved_definition() {
    let pinned_fixture = Fixture::new();
    let pinned = resolve(&pinned_fixture.store(), "1.2.3", &"a".repeat(64));
    let pinned_root = pinned_fixture.publish_install(&pinned);
    pinned_fixture.store().read_installation(&pinned).unwrap();
    let pinned_metadata = pinned_root.join(".swawkit-dev-install.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&pinned_metadata).unwrap()).unwrap();
    value["sourceVerification"] = json!("github");
    write_json(&pinned_metadata, &value);
    assert_eq!(
        pinned_fixture
            .store()
            .read_installation(&pinned)
            .unwrap_err()
            .kind(),
        ArchiveToolErrorKind::MetadataStale
    );

    let unpinned_fixture = Fixture::new();
    let unpinned = resolve(&unpinned_fixture.store(), "1.2.3", "");
    let unpinned_root = unpinned_fixture.publish_install(&unpinned);
    let unpinned_metadata = unpinned_root.join(".swawkit-dev-install.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&unpinned_metadata).unwrap()).unwrap();
    value["sourceVerification"] = json!("github");
    write_json(&unpinned_metadata, &value);
    unpinned_fixture
        .store()
        .read_installation(&unpinned)
        .unwrap();
    value["sourceVerification"] = json!("project");
    write_json(&unpinned_metadata, &value);
    assert_eq!(
        unpinned_fixture
            .store()
            .read_installation(&unpinned)
            .unwrap_err()
            .kind(),
        ArchiveToolErrorKind::MetadataStale
    );
}

#[test]
fn pwsh_exact_versions_and_signature_match_the_published_contract() {
    assert_eq!(
        ArchiveToolRequest::new(&PWSH, "preview", "")
            .unwrap_err()
            .kind(),
        ArchiveToolErrorKind::InvalidVersion
    );
    let request = ArchiveToolRequest::new(&PWSH, "7.6.4", "").unwrap();
    let root = Fixture::new();
    let resolved = ArchiveToolStore::new(&root.data_root, &PWSH)
        .resolve(&request)
        .unwrap()
        .unwrap();

    assert_eq!(resolved.version(), "7.6.4");
    assert_eq!(
        PWSH.definition_signature(resolved.version(), resolved.project_sha256()),
        "cd874984efa7930073b8b1a6c988e139def6b084ea05d30ee0e6f70b641dfb68"
    );
}

#[test]
fn trust_is_derived_from_the_typed_resolution_state() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let pinned = resolve(&store, "1.2.3", &"a".repeat(64));
    assert_eq!(
        store.trust(&pinned, None).unwrap().level(),
        TrustLevel::Pinned
    );

    fixture.publish_selection(None);
    let unverified = resolve(&store, "latest", "");
    let trust = store.trust(&unverified, None).unwrap();
    assert_eq!(trust.level(), TrustLevel::Unpinned);
    assert_eq!(trust.message(), "no comparable release SHA-256");

    let unresolved = resolve(&store, "1.2.3", "");
    assert_eq!(
        store.trust(&unresolved, None).unwrap().message(),
        "awaiting GitHub Release resolution"
    );

    let mut selection: Value = serde_json::from_slice(
        &fs::read(fixture.tool_root().join(".swawkit-dev-selection.json")).unwrap(),
    )
    .unwrap();
    selection["sourceVerification"] = json!("github");
    write_json(
        &fixture.tool_root().join(".swawkit-dev-selection.json"),
        &selection,
    );
    let github = resolve(&store, "latest", "");
    assert_eq!(
        store.trust(&github, None).unwrap().level(),
        TrustLevel::Upstream
    );
}

fn resolve(
    store: &ArchiveToolStore<'_>,
    version: &str,
    project_sha256: &str,
) -> ResolvedDefinition {
    let request = ArchiveToolRequest::new(&BUN, version, project_sha256).unwrap();
    store.resolve(&request).unwrap().unwrap()
}

fn write_json(path: &Path, value: &Value) {
    fs::create_dir_all(path.parent().unwrap()).expect("create JSON parent");
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).expect("write JSON");
}

fn sha256(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
