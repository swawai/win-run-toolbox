use super::*;
use crate::data_root::record::{publish_entry_record, read_entry_record};
use std::fs::{self, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicU64, Ordering};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    swawkit_home: PathBuf,
    entry_file: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-data-root-lease-{}-{sequence}",
            std::process::id()
        ));
        let swawkit_home = root.join("home");
        let entry_file = root.join("project/lease-entry.exe");
        fs::create_dir_all(&swawkit_home).expect("create Swaw Kit home");
        fs::create_dir_all(entry_file.parent().expect("entry parent"))
            .expect("create entry parent");
        fs::write(&entry_file, "entry").expect("write entry");
        Self {
            root,
            swawkit_home,
            entry_file,
        }
    }

    fn request(&self) -> ResolveDataRootRequest<'_> {
        ResolveDataRootRequest {
            swawkit_home: &self.swawkit_home,
            entry_file: &self.entry_file,
        }
    }

    fn resolve(&self) -> ResolvedDataRoot {
        let mut approve = |_claim: &DataRootClaim| Ok(true);
        resolve_data_root(self.request(), &mut approve).expect("resolve DataRoot")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn entry_pin_failure_happens_before_a_new_data_root_is_created() {
    let fixture = Fixture::new();
    let target = fixture.swawkit_home.join("data/proj.lease-entry");
    let conflicting_writer = OpenOptions::new()
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&fixture.entry_file)
        .expect("open a writer that permits transient identity reads");
    let mut unexpected_claim =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("new DataRoot must not claim"));

    assert!(
        resolve_data_root(fixture.request(), &mut unexpected_claim).is_err(),
        "the incompatible writer must prevent the Entry lease"
    );
    assert!(
        !target.exists(),
        "Entry pinning must finish before DataRoot execution has side effects"
    );

    drop(conflicting_writer);
    let resolved = fixture.resolve();
    assert_eq!(resolved.path(), target);
}

#[test]
fn lease_pins_the_entry_file_until_the_last_resolved_clone_is_dropped() {
    let fixture = Fixture::new();
    let resolved = fixture.resolve();
    let clone = resolved.clone();
    let moved_entry = fixture.root.join("project/moved-entry.exe");
    let concurrent = fixture.resolve();

    assert_eq!(
        fs::read(&fixture.entry_file).expect("read pinned entry"),
        b"entry"
    );
    assert_eq!(concurrent.path(), resolved.path());
    drop(concurrent);
    assert!(fs::write(&fixture.entry_file, "changed").is_err());
    assert!(fs::rename(&fixture.entry_file, &moved_entry).is_err());
    drop(resolved);
    assert!(fs::rename(&fixture.entry_file, &moved_entry).is_err());

    drop(clone);
    fs::write(&fixture.entry_file, "changed").expect("modify released entry");
    fs::rename(&fixture.entry_file, moved_entry).expect("rename released entry");
}

#[test]
fn lease_allows_data_root_contents_but_pins_the_directory_itself() {
    let fixture = Fixture::new();
    let resolved = fixture.resolve();
    let moved_data_root = fixture.root.join("moved-data-root");

    fs::write(resolved.path().join("module-state.bin"), b"state")
        .expect("write inside pinned DataRoot");
    assert!(fs::rename(resolved.path(), &moved_data_root).is_err());

    let data_root = resolved.path().to_path_buf();
    drop(resolved);
    fs::rename(&data_root, moved_data_root).expect("rename released DataRoot");
}

#[test]
fn lease_prevents_the_entry_record_from_being_rebound() {
    let fixture = Fixture::new();
    let resolved = fixture.resolve();
    let other_entry = fixture.root.join("project/other.exe");
    fs::write(&other_entry, "other entry").expect("write other entry");
    let other_identity = EntryIdentity::read(&other_entry).expect("other identity");

    assert!(
        publish_entry_record(
            resolved.path(),
            "lease-entry",
            &other_entry,
            &other_identity,
        )
        .is_err()
    );
    let current = read_entry_record(resolved.path());
    assert!(
        current
            .valid_record()
            .is_some_and(|record| !record.matches_identity(&other_identity))
    );

    let data_root = resolved.path().to_path_buf();
    drop(resolved);
    publish_entry_record(&data_root, "lease-entry", &other_entry, &other_identity)
        .expect("rebind released record");
    assert!(
        read_entry_record(&data_root)
            .valid_record()
            .is_some_and(|record| record.matches_identity(&other_identity))
    );
}
