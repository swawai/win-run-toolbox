use super::*;
use crate::data_root::record::publish_entry_record;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    swawkit_home: PathBuf,
    entry_file: PathBuf,
    data_root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-data-root-directory-identity-{}-{sequence}",
            std::process::id()
        ));
        let swawkit_home = root.join("home");
        let project_root = root.join("project");
        let entry_file = project_root.join("directory-conflict.cmd");
        let data_root = swawkit_home.join("data/proj.directory-conflict");
        fs::create_dir_all(&project_root).expect("create project root");
        fs::create_dir_all(&data_root).expect("create DataRoot");
        fs::write(&entry_file, "entry").expect("write entry");
        Self {
            root,
            swawkit_home,
            entry_file,
            data_root,
        }
    }

    fn request(&self) -> ResolveDataRootRequest<'_> {
        ResolveDataRootRequest {
            swawkit_home: &self.swawkit_home,
            entry_file: &self.entry_file,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn explicit_claim_rejects_a_replaced_directory_with_identical_record_bytes() {
    let fixture = Fixture::new();
    let record = b"same invalid identity record";
    let record_path = fixture.data_root.join("_entry.json");
    fs::write(&record_path, record).expect("write original record");

    let expected = inspect_data_root(fixture.request())
        .expect("inspect original DataRoot")
        .claim
        .expect("claim original DataRoot");
    let original_identity =
        EntryIdentity::read_directory(&fixture.data_root).expect("original directory identity");

    let displaced = fixture.root.join("displaced-data-root");
    fs::rename(&fixture.data_root, &displaced).expect("displace original DataRoot");
    fs::create_dir(&fixture.data_root).expect("create replacement DataRoot");
    fs::write(&record_path, record).expect("write identical replacement record");
    let replacement_identity =
        EntryIdentity::read_directory(&fixture.data_root).expect("replacement directory identity");
    assert_ne!(original_identity, replacement_identity);

    let current = inspect_data_root(fixture.request())
        .expect("inspect replacement DataRoot")
        .claim
        .expect("claim replacement DataRoot");
    assert_ne!(expected.revision(), current.revision());

    let error = claim_data_root(fixture.request(), &expected).unwrap_err();
    assert!(error.is_state_changed());
    assert_eq!(
        fs::read(record_path).expect("read replacement record"),
        record
    );
}

#[test]
fn explicit_claim_rejects_a_replacement_that_publishes_a_matching_record() {
    let fixture = Fixture::new();
    fs::write(fixture.data_root.join("_entry.json"), "unbound").expect("write original record");
    let expected = inspect_data_root(fixture.request())
        .expect("inspect original DataRoot")
        .claim
        .expect("claim original DataRoot");

    let displaced = fixture.root.join("displaced-matching-data-root");
    fs::rename(&fixture.data_root, &displaced).expect("displace original DataRoot");
    fs::create_dir(&fixture.data_root).expect("create replacement DataRoot");
    let entry_identity = EntryIdentity::read(&fixture.entry_file).expect("entry identity");
    publish_entry_record(
        &fixture.data_root,
        "directory-conflict",
        &fixture.entry_file,
        &entry_identity,
    )
    .expect("publish matching replacement record");
    assert!(
        inspect_data_root(fixture.request())
            .expect("inspect direct replacement")
            .claim
            .is_none()
    );

    let error = claim_data_root(fixture.request(), &expected).unwrap_err();
    assert!(error.is_state_changed());
}

#[test]
fn explicit_claim_accepts_a_matching_record_published_in_the_same_directory() {
    let fixture = Fixture::new();
    fs::write(fixture.data_root.join("_entry.json"), "unbound").expect("write original record");
    let expected = inspect_data_root(fixture.request())
        .expect("inspect original DataRoot")
        .claim
        .expect("claim original DataRoot");
    let original_identity =
        EntryIdentity::read_directory(&fixture.data_root).expect("original directory identity");

    let entry_identity = EntryIdentity::read(&fixture.entry_file).expect("entry identity");
    publish_entry_record(
        &fixture.data_root,
        "directory-conflict",
        &fixture.entry_file,
        &entry_identity,
    )
    .expect("publish matching record");

    let resolved = claim_data_root(fixture.request(), &expected).expect("accept direct completion");
    assert_eq!(resolved.path(), fixture.data_root);
    assert_eq!(
        EntryIdentity::read_directory(resolved.path()).expect("completed directory identity"),
        original_identity
    );
}

#[test]
fn rename_claim_accepts_the_same_directory_moved_and_completed_elsewhere() {
    let fixture = Fixture::new();
    let entry_identity = EntryIdentity::read(&fixture.entry_file).expect("entry identity");
    publish_entry_record(
        &fixture.data_root,
        "directory-conflict",
        &fixture.entry_file,
        &entry_identity,
    )
    .expect("publish original binding");
    let renamed_entry = fixture.root.join("project/directory-renamed.cmd");
    fs::rename(&fixture.entry_file, &renamed_entry).expect("rename entry");
    let request = ResolveDataRootRequest {
        swawkit_home: &fixture.swawkit_home,
        entry_file: &renamed_entry,
    };
    let expected = inspect_data_root(request)
        .expect("inspect rename")
        .claim
        .expect("rename claim");

    let renamed_root = fixture.swawkit_home.join("data/proj.directory-renamed");
    let original_identity =
        EntryIdentity::read_directory(&fixture.data_root).expect("original directory identity");
    fs::rename(&fixture.data_root, &renamed_root).expect("complete DataRoot move");
    publish_entry_record(
        &renamed_root,
        "directory-renamed",
        &renamed_entry,
        &entry_identity,
    )
    .expect("complete renamed binding");

    let resolved = claim_data_root(request, &expected).expect("accept completed rename");
    assert_eq!(resolved.path(), renamed_root);
    assert_eq!(
        EntryIdentity::read_directory(resolved.path()).expect("renamed directory identity"),
        original_identity
    );
}
