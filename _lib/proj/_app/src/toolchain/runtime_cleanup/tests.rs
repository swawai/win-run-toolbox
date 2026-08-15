use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    store: RuntimeReleaseStore,
    selected: String,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-runtime-cleanup-{}-{sequence}",
            std::process::id()
        ));
        let releases = root.join("_lib/proj/_bin/releases");
        fs::create_dir_all(&releases).expect("create releases root");
        let selected = publish_release(&releases, b"selected");
        fs::write(root.join("_lib/proj/_bin/current"), format!("{selected}\n"))
            .expect("write selector");
        let store = RuntimeReleaseStore::open(&root).expect("open store");
        Self {
            root,
            store,
            selected,
        }
    }

    fn release(&self, seed: &[u8]) -> String {
        publish_release(self.store.releases_root(), seed)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn preview_separates_selected_in_use_removable_and_invalid() {
    let fixture = Fixture::new();
    let in_use_id = fixture.release(b"in-use");
    let removable_id = fixture.release(b"removable");
    let invalid_id = "f".repeat(64);
    fs::create_dir(fixture.store.releases_root().join(&invalid_id)).expect("invalid release");
    let in_use = BTreeMap::from([(in_use_id.clone(), vec![23, 42])]);

    let plan = build_plan(&fixture.store, &fixture.selected, &in_use).expect("build plan");
    assert_eq!(state(&plan, &fixture.selected), &PlanState::Selected);
    assert_eq!(state(&plan, &in_use_id), &PlanState::InUse(vec![23, 42]));
    assert_eq!(state(&plan, &removable_id), &PlanState::Removable);
    assert!(matches!(state(&plan, &invalid_id), PlanState::Retained(_)));
}

#[test]
fn apply_removes_only_valid_removable_release() {
    let fixture = Fixture::new();
    let removable_id = fixture.release(b"removable");
    let removable_root = fixture.store.releases_root().join(&removable_id);
    let plan = build_plan(&fixture.store, &fixture.selected, &BTreeMap::new()).expect("plan");

    execute_plan(&fixture.store, &plan, true).expect("apply cleanup");

    assert!(
        fixture
            .store
            .releases_root()
            .join(&fixture.selected)
            .is_dir()
    );
    assert!(!removable_root.exists());
}

#[test]
fn corrupted_release_is_retained() {
    let fixture = Fixture::new();
    let id = fixture.release(b"corrupt");
    let root = fixture.store.releases_root().join(&id);
    fs::write(root.join("swawkit-proj.exe"), b"changed").expect("corrupt artifact");

    let plan = build_plan(&fixture.store, &fixture.selected, &BTreeMap::new()).expect("plan");
    assert!(matches!(state(&plan, &id), PlanState::Retained(_)));
    execute_plan(&fixture.store, &plan, true).expect("apply cleanup");
    assert!(root.exists());
}

#[test]
fn invalid_selected_release_stops_the_entire_cleanup() {
    let fixture = Fixture::new();
    fs::write(
        fixture
            .store
            .releases_root()
            .join(&fixture.selected)
            .join("swawkit-proj.exe"),
        b"changed",
    )
    .expect("corrupt selected artifact");

    let error = build_plan(&fixture.store, &fixture.selected, &BTreeMap::new())
        .expect_err("invalid selected Release must stop cleanup");
    assert!(error.contains("cleanup stopped"), "{error}");
}

#[test]
fn reparse_artifact_is_retained_without_following_its_target() {
    let fixture = Fixture::new();
    let id = fixture.release(b"reparse");
    let root = fixture.store.releases_root().join(&id);
    let artifact = root.join("swawkit-proj.exe");
    let external = fixture.root.join("external.exe");
    fs::write(&external, b"external").expect("write external file");
    fs::remove_file(&artifact).expect("remove artifact");
    if std::os::windows::fs::symlink_file(&external, &artifact).is_err() {
        return;
    }

    let plan = build_plan(&fixture.store, &fixture.selected, &BTreeMap::new()).expect("plan");
    assert!(matches!(state(&plan, &id), PlanState::Retained(_)));
    execute_plan(&fixture.store, &plan, true).expect("apply cleanup");
    assert_eq!(
        fs::read(&external).expect("read external file"),
        b"external"
    );
    assert!(root.exists());
}

fn state<'a>(plan: &'a [PlanItem], release_id: &str) -> &'a PlanState {
    &plan
        .iter()
        .find(|item| item.name == release_id)
        .expect("plan item")
        .state
}

fn publish_release(releases: &Path, seed: &[u8]) -> String {
    let artifacts = [
        ("swawkit-proj.exe", [seed, b"-core"].concat()),
        ("swawkit-proj-host.exe", [seed, b"-host"].concat()),
        ("swawkit-proj-toolchain.exe", [seed, b"-toolchain"].concat()),
    ];
    let records = artifacts
        .iter()
        .map(|(name, bytes)| {
            (
                *name,
                bytes.len() as u64,
                format!("{:x}", Sha256::digest(bytes)),
            )
        })
        .collect::<Vec<_>>();
    let mut identity = vec![swawkit_proj::runtime_release::RUNTIME_RELEASE_SCHEMA.to_owned()];
    for (name, length, sha256) in &records {
        identity.extend([name.to_string(), length.to_string(), sha256.clone()]);
    }
    let release_id = format!("{:x}", Sha256::digest(identity.join("\n").as_bytes()));
    let root = releases.join(&release_id);
    fs::create_dir(&root).expect("create release");
    for (name, bytes) in &artifacts {
        fs::write(root.join(name), bytes).expect("write artifact");
    }
    let manifest = json!({
        "schema": swawkit_proj::runtime_release::RUNTIME_RELEASE_SCHEMA,
        "releaseId": release_id,
        "artifacts": records.iter().map(|(name, length, sha256)| json!({
            "name": name,
            "length": length,
            "sha256": sha256,
        })).collect::<Vec<_>>(),
    });
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&manifest).expect("manifest JSON"),
    )
    .expect("write manifest");
    release_id
}
