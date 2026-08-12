use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use zip::write::SimpleFileOptions;

use super::*;
use crate::development::archive_tool::install::archive;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-archive-cache-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("create cache fixture");
        Self { root }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn artifact_lock_identity_preserves_windows_path_case() {
    let identity = artifact_lock_identity(Path::new(r"C:\MiXeD\CacheRoot")).unwrap();

    assert!(identity.contains(r"MiXeD\CacheRoot"));
    assert!(!identity.contains(r"mixed\cacheroot"));
}

#[test]
fn verified_archive_guard_blocks_mutation_through_extraction() {
    let fixture = Fixture::new();
    let archive_path = fixture.root.join("payload.zip");
    write_archive(&archive_path);
    let destination = fixture.root.join("extracted");
    fs::create_dir(&destination).unwrap();

    let guard = open_archive_guard(&archive_path).unwrap();
    let digest = guarded_archive_digest(&guard, &archive_path).unwrap();
    assert_eq!(digest.len(), 64);
    assert!(
        OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&archive_path)
            .is_err(),
        "the live verification handle must deny writers"
    );
    assert!(
        fs::rename(&archive_path, fixture.root.join("moved.zip")).is_err(),
        "the live verification handle must deny replacement/rename"
    );

    archive::extract_file(&guard, &destination).unwrap();
    assert_eq!(
        fs::read(destination.join("payload.txt")).unwrap(),
        b"payload"
    );

    drop(guard);
    fs::write(&archive_path, b"replacement after guard release").unwrap();
    assert_eq!(
        fs::read(&archive_path).unwrap(),
        b"replacement after guard release"
    );
}

fn write_archive(path: &Path) {
    let file = fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file("payload.txt", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"payload").unwrap();
    writer.finish().unwrap();
}
