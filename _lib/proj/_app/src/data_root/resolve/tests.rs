use super::*;
use crate::data_root::claim::ClaimKind;
use crate::data_root::lock::DataRootLock;
use crate::data_root::record::{publish_entry_record, read_entry_record};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    swawkit_home: PathBuf,
    project_root: PathBuf,
    legacy_data_directory: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-data-resolve-{}-{sequence}",
            std::process::id()
        ));
        let swawkit_home = root.join("home");
        let project_root = root.join("project");
        let legacy_data_directory = project_root.join("data");
        fs::create_dir_all(&swawkit_home).expect("create SWAWKIT_HOME");
        fs::create_dir_all(&project_root).expect("create project root");
        Self {
            root,
            swawkit_home,
            project_root,
            legacy_data_directory,
        }
    }

    fn entry(&self, name: &str) -> PathBuf {
        self.project_root.join(format!("{name}.cmd"))
    }

    fn write_entry(&self, name: &str, content: &str) -> PathBuf {
        let path = self.entry(name);
        fs::write(&path, content).expect("write entry");
        path
    }

    fn data_root(&self, name: &str) -> PathBuf {
        self.swawkit_home.join("data").join(format!("proj.{name}"))
    }

    fn request<'a>(&'a self, entry_file: &'a Path) -> ResolveDataRootRequest<'a> {
        ResolveDataRootRequest {
            swawkit_home: &self.swawkit_home,
            entry_file,
            inherited_data_root: None,
            legacy_data_directory: Some(&self.legacy_data_directory),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn approve(_claim: &DataRootClaim) -> Result<bool, ClaimApprovalError> {
    Ok(true)
}

#[test]
fn inspection_is_read_only_for_fresh_and_unbound_entries() {
    let fixture = Fixture::new();
    let fresh_entry = fixture.write_entry("fresh", "entry");
    let fresh = inspect_data_root(fixture.request(&fresh_entry)).expect("inspect fresh Entry");
    assert!(fresh.claim.is_none());
    assert!(!fixture.swawkit_home.join("data").exists());

    let unbound_entry = fixture.write_entry("unbound", "entry");
    let data_root = fixture.data_root("unbound");
    fs::create_dir_all(&data_root).expect("create unbound DataRoot");
    fs::write(data_root.join("_entry.json"), "invalid").expect("write invalid record");
    let before = fs::read(data_root.join("_entry.json")).expect("read invalid record");

    let unbound = inspect_data_root(fixture.request(&unbound_entry)).expect("inspect claim");
    assert!(matches!(
        unbound.claim,
        Some(DataRootClaim {
            kind: ClaimKind::Current,
            ..
        })
    ));
    assert_eq!(
        fs::read(data_root.join("_entry.json")).expect("reread invalid record"),
        before
    );
    assert!(!fixture.swawkit_home.join("data/_proj-entry.lock").exists());
}

#[test]
fn explicit_claim_applies_only_the_inspected_plan() {
    let fixture = Fixture::new();
    let entry = fixture.write_entry("explicit", "entry");
    let data_root = fixture.data_root("explicit");
    fs::create_dir_all(&data_root).expect("create unbound DataRoot");
    let inspection = inspect_data_root(fixture.request(&entry)).expect("inspect claim");
    let claim = inspection.claim.expect("required claim");

    let resolved = claim_data_root(fixture.request(&entry), &claim).expect("apply claim");
    assert_eq!(resolved.path(), data_root);
    assert!(read_entry_record(&data_root).valid_record().is_some());

    let stale = claim_data_root(fixture.request(&entry), &claim).expect("idempotent claim");
    assert_eq!(stale.path(), data_root);
}

#[test]
fn explicit_expected_claim_rejects_a_changed_incumbent_record() {
    let fixture = Fixture::new();
    let entry = fixture.write_entry("explicit-conflict", "target entry");
    let incumbent_a = fixture.write_entry("incumbent-a", "first incumbent");
    let incumbent_c = fixture.write_entry("incumbent-c", "second incumbent");
    let data_root = fixture.data_root("explicit-conflict");
    fs::create_dir_all(&data_root).expect("create occupied DataRoot");
    publish_incumbent(&data_root, "explicit-conflict", &incumbent_a);

    let expected = inspect_data_root(fixture.request(&entry))
        .expect("inspect first incumbent")
        .claim
        .expect("claim first incumbent");
    publish_incumbent(&data_root, "explicit-conflict", &incumbent_c);
    let current = inspect_data_root(fixture.request(&entry))
        .expect("inspect second incumbent")
        .claim
        .expect("claim second incumbent");
    assert_ne!(expected.revision(), current.revision());

    let error = claim_data_root(fixture.request(&entry), &expected).unwrap_err();
    assert!(error.is_state_changed());
    assert_record_matches(&data_root, &incumbent_c);
}

#[test]
fn approver_expected_claim_rejects_a_changed_incumbent_record() {
    let fixture = Fixture::new();
    let entry = fixture.write_entry("callback-conflict", "target entry");
    let incumbent_a = fixture.write_entry("callback-a", "first incumbent");
    let incumbent_c = fixture.write_entry("callback-c", "second incumbent");
    let data_root = fixture.data_root("callback-conflict");
    fs::create_dir_all(&data_root).expect("create occupied DataRoot");
    publish_incumbent(&data_root, "callback-conflict", &incumbent_a);
    let mut approver = |_claim: &DataRootClaim| {
        publish_incumbent(&data_root, "callback-conflict", &incumbent_c);
        Ok(true)
    };

    let error = resolve_data_root(fixture.request(&entry), &mut approver).unwrap_err();
    assert!(error.is_state_changed());
    assert_record_matches(&data_root, &incumbent_c);
}

#[test]
fn completed_explicit_legacy_claim_cleans_legacy_residue() {
    let fixture = Fixture::new();
    let entry = fixture.write_entry("legacy-new", "entry");
    let identity = EntryIdentity::read(&entry).expect("entry identity");
    let legacy_directory = fixture.legacy_data_directory.clone();
    let legacy_root = legacy_directory.join("proj.legacy-old");
    fs::create_dir_all(&legacy_root).expect("create legacy DataRoot");
    publish_entry_record(&legacy_root, "legacy-old", &entry, &identity)
        .expect("publish legacy record");
    fs::write(legacy_directory.join("_proj-entry.lock"), "").expect("write legacy lock");
    let expected = inspect_data_root(fixture.request(&entry))
        .expect("inspect legacy claim")
        .claim
        .expect("legacy claim");
    assert_eq!(expected.kind, ClaimKind::MigrateLegacy);

    let target = fixture.data_root("legacy-new");
    fs::create_dir_all(target.parent().expect("target parent")).expect("create current data root");
    fs::rename(&legacy_root, &target).expect("simulate completed legacy move");
    publish_entry_record(&target, "legacy-new", &entry, &identity)
        .expect("complete target record");

    let resolved = claim_data_root(fixture.request(&entry), &expected)
        .expect("accept completed legacy claim");
    assert_eq!(resolved.path(), target);
    assert!(!legacy_directory.exists());
}

#[test]
fn creates_and_then_directly_reuses_a_bound_data_root() {
    let fixture = Fixture::new();
    let entry = fixture.write_entry("alpha", "first");
    let mut unexpected_claim =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));

    let first =
        resolve_data_root(fixture.request(&entry), &mut unexpected_claim).expect("create DataRoot");
    assert_eq!(first.path(), fixture.data_root("alpha"));
    assert!(first.path().join("_entry.json").is_file());
    assert!(fixture.swawkit_home.join("data/_proj-entry.lock").is_file());

    let second =
        resolve_data_root(fixture.request(&entry), &mut unexpected_claim).expect("direct DataRoot");
    assert_eq!(second.path(), first.path());
}

#[test]
fn claim_current_replaces_an_invalid_record_only_after_approval() {
    let fixture = Fixture::new();
    let entry = fixture.write_entry("beta", "entry");
    let data_root = fixture.data_root("beta");
    fs::create_dir_all(&data_root).expect("create unbound DataRoot");
    fs::write(data_root.join("_entry.json"), "invalid").expect("write invalid record");
    let mut saw_claim = false;
    let mut approver = |claim: &DataRootClaim| {
        saw_claim = claim.kind == ClaimKind::Current
            && claim.data_root == data_root
            && claim.source_data_root.is_none();
        Ok(true)
    };

    let resolved =
        resolve_data_root(fixture.request(&entry), &mut approver).expect("claim current DataRoot");
    assert!(saw_claim);
    assert_eq!(resolved.path(), data_root);
    assert!(read_entry_record(&data_root).valid_record().is_some());
    assert!(
        fs::read_dir(&data_root)
            .expect("read DataRoot")
            .all(|entry| !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .contains("._entry"))
    );
}

#[test]
fn denial_leaves_an_unbound_data_root_unchanged() {
    let fixture = Fixture::new();
    let entry = fixture.write_entry("gamma", "entry");
    let data_root = fixture.data_root("gamma");
    fs::create_dir_all(&data_root).expect("create unbound DataRoot");
    let mut deny = |_claim: &DataRootClaim| Ok(false);

    let error = resolve_data_root(fixture.request(&entry), &mut deny).unwrap_err();
    assert!(error.is_approval_denied());
    assert!(!data_root.join("_entry.json").exists());
}

#[test]
fn accepts_when_another_process_completes_the_same_claim_during_confirmation() {
    let fixture = Fixture::new();
    let entry = fixture.write_entry("completed", "entry");
    let data_root = fixture.data_root("completed");
    fs::create_dir_all(&data_root).expect("create unbound DataRoot");
    let data_directory = fixture.swawkit_home.join("data");
    let mut approver = |claim: &DataRootClaim| {
        let lock = DataRootLock::acquire_for_test(&data_directory, 1, Duration::ZERO)
            .expect("simulate the completing process");
        let identity =
            EntryIdentity::from_parts(&claim.volume_id, &claim.file_id).expect("claim identity");
        publish_entry_record(
            &claim.data_root,
            &claim.entry_name,
            &claim.entry_file,
            &identity,
        )
        .expect("complete binding");
        drop(lock);
        Ok(true)
    };

    let resolved =
        resolve_data_root(fixture.request(&entry), &mut approver).expect("accept completed claim");
    assert_eq!(resolved.path(), data_root);
    assert!(read_entry_record(&data_root).valid_record().is_some());
}

#[test]
fn rename_follows_file_identity_and_preserves_opaque_module_data() {
    let fixture = Fixture::new();
    let old_entry = fixture.write_entry("old-name", "entry");
    let mut approver = approve;
    let old =
        resolve_data_root(fixture.request(&old_entry), &mut approver).expect("create old binding");
    let export_root = old.path().join("modules/kernel/.dev/setup/export");
    let old_data_root = old.path().to_path_buf();
    fs::create_dir_all(&export_root).expect("create opaque module export");
    fs::write(export_root.join("sentinel.bin"), b"opaque module data")
        .expect("write opaque module data");
    drop(old);

    let new_entry = fixture.entry("new-name");
    fs::rename(&old_entry, &new_entry).expect("rename entry");
    let mut saw_rename = false;
    let mut rename_approver = |claim: &DataRootClaim| {
        saw_rename = claim.kind == ClaimKind::Rename
            && claim.source_data_root.as_deref() == Some(old_data_root.as_path());
        Ok(true)
    };
    let renamed = resolve_data_root(fixture.request(&new_entry), &mut rename_approver)
        .expect("claim renamed DataRoot");

    assert!(saw_rename);
    assert_eq!(renamed.path(), fixture.data_root("new-name"));
    assert!(!old_data_root.exists());
    assert!(renamed.warnings().is_empty());
    assert_eq!(
        fs::read(
            renamed
                .path()
                .join("modules/kernel/.dev/setup/export/sentinel.bin")
        )
        .expect("read preserved module data"),
        b"opaque module data"
    );
}

#[test]
fn confirmation_releases_the_data_root_lock_but_keeps_the_entry_pinned() {
    let fixture = Fixture::new();
    let entry = fixture.write_entry("epsilon", "original");
    let mut approver = approve;
    let original =
        resolve_data_root(fixture.request(&entry), &mut approver).expect("create original binding");
    let original_data_root = original.path().to_path_buf();
    drop(original);
    replace_entry(&entry, "first replacement");

    let data_directory = fixture.swawkit_home.join("data");
    let mut checking_approver = |_claim: &DataRootClaim| {
        let lock = DataRootLock::acquire_for_test(&data_directory, 1, Duration::ZERO)
            .expect("confirmation must not hold the DataRoot lock");
        drop(lock);
        assert!(
            fs::write(&entry, "second replacement").is_err(),
            "confirmation must keep the planned Entry identity pinned"
        );
        Ok(true)
    };
    let resolved = resolve_data_root(fixture.request(&entry), &mut checking_approver)
        .expect("claim the pinned replacement Entry");
    assert_eq!(resolved.path(), original_data_root);
    assert_record_matches(resolved.path(), &entry);
}

#[test]
fn migrates_a_matching_legacy_root_without_claim_and_cleans_its_directory() {
    let fixture = Fixture::new();
    let entry = fixture.write_entry("legacy", "entry");
    let identity = EntryIdentity::read(&entry).expect("entry identity");
    let legacy_directory = fixture.project_root.join("data");
    let legacy_root = legacy_directory.join("proj.legacy");
    fs::create_dir_all(&legacy_root).expect("create legacy root");
    publish_entry_record(&legacy_root, "legacy", &entry, &identity).expect("publish legacy record");
    fs::write(legacy_directory.join("_proj-entry.lock"), "").expect("write stale lock file");
    let mut unexpected_claim =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("legacy migration should not claim"));

    let resolved = resolve_data_root(fixture.request(&entry), &mut unexpected_claim)
        .expect("migrate legacy root");
    assert_eq!(resolved.path(), fixture.data_root("legacy"));
    assert!(!legacy_root.exists());
    assert!(!legacy_directory.exists());
}

fn replace_entry(path: &Path, content: &str) {
    let replacement = path.with_extension(format!(
        "{}.replacement",
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&replacement, content).expect("write replacement");
    fs::remove_file(path).expect("remove previous entry");
    fs::rename(replacement, path).expect("publish replacement entry");
}

fn publish_incumbent(data_root: &Path, entry_name: &str, entry_file: &Path) {
    let identity = EntryIdentity::read(entry_file).expect("incumbent identity");
    publish_entry_record(data_root, entry_name, entry_file, &identity)
        .expect("publish incumbent record");
}

fn assert_record_matches(data_root: &Path, entry_file: &Path) {
    let expected = EntryIdentity::read(entry_file).expect("expected incumbent identity");
    let state = read_entry_record(data_root);
    let record = state.valid_record().expect("incumbent record remains valid");
    assert!(record.matches_identity(&expected));
}
