use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
const CONTENT: &[u8] = b"verified MSVC fixture payload";

struct Fixture {
    root: PathBuf,
    definition: MsvcDefinition,
    payload: MsvcPayload,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-msvc-payload-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self {
            root,
            definition: MsvcDefinition::new("17").unwrap(),
            payload: MsvcPayload::fixture("fixture.vsix", CONTENT),
        }
    }

    fn cache(&self) -> MsvcPayloadCache<'_> {
        MsvcPayloadCache::new(&self.root, &self.definition)
    }

    fn path(&self) -> PathBuf {
        self.root
            .join("downloads/msvc/17/payloads")
            .join(self.payload.sha256())
            .join(self.payload.leaf_name())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn acquire_with_content(
    fixture: &Fixture,
    content: &[u8],
    calls: &mut usize,
) -> Result<VerifiedMsvcPayload, MsvcError> {
    let mut transfer =
        |_: &OsStr, destination: &Path, progress: &mut dyn FnMut(u64, Option<u64>)| {
            *calls += 1;
            fs::write(destination, content).map_err(MsvcError::from)?;
            progress(content.len() as u64, Some(content.len() as u64));
            Ok(content.len() as u64)
        };
    fixture
        .cache()
        .acquire_with(&fixture.payload, &mut transfer, &mut |_, _| {})
}

#[test]
fn first_use_downloads_and_subsequent_use_is_offline() {
    let fixture = Fixture::new();
    let mut calls = 0;

    let verified = acquire_with_content(&fixture, CONTENT, &mut calls).unwrap();
    assert_eq!(verified.path(), fixture.path());
    assert_eq!(verified.length(), CONTENT.len() as u64);
    drop(verified);
    let verified = acquire_with_content(&fixture, b"offline", &mut calls).unwrap();

    assert_eq!(calls, 1);
    assert_eq!(fs::read(verified.path()).unwrap(), CONTENT);
}

#[test]
fn a_corrupt_cache_entry_is_replaced_and_reverified() {
    let fixture = Fixture::new();
    let mut calls = 0;
    drop(acquire_with_content(&fixture, CONTENT, &mut calls).unwrap());
    fs::write(fixture.path(), b"corrupt").unwrap();

    drop(acquire_with_content(&fixture, CONTENT, &mut calls).unwrap());

    assert_eq!(calls, 2);
    assert_eq!(fs::read(fixture.path()).unwrap(), CONTENT);
}

#[test]
fn an_ordinary_directory_in_the_payload_slot_is_repaired() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.path()).unwrap();
    fs::write(fixture.path().join("residue"), b"interrupted").unwrap();
    let mut calls = 0;

    drop(acquire_with_content(&fixture, CONTENT, &mut calls).unwrap());

    assert_eq!(calls, 1);
    assert_eq!(fs::read(fixture.path()).unwrap(), CONTENT);
}

#[test]
fn an_unverified_download_is_removed() {
    let fixture = Fixture::new();
    let mut calls = 0;

    let failure = acquire_with_content(&fixture, b"wrong payload", &mut calls)
        .err()
        .expect("reject unverified download");

    assert_eq!(failure.kind(), MsvcErrorKind::DownloadFailed);
    assert_eq!(calls, 1);
    assert!(!fixture.path().exists());
}

#[test]
fn the_verified_handle_prevents_mutation_until_the_consumer_finishes() {
    let fixture = Fixture::new();
    let mut calls = 0;
    let verified = acquire_with_content(&fixture, CONTENT, &mut calls).unwrap();

    assert!(fs::write(fixture.path(), b"changed").is_err());
    let mut clone = verified.try_clone().unwrap();
    clone.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    clone.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, CONTENT);
    drop(clone);
    drop(verified);

    fs::write(fixture.path(), b"changed").unwrap();
}

#[test]
fn a_reparse_payload_fails_closed() {
    use std::os::windows::fs::symlink_file;

    let fixture = Fixture::new();
    fs::create_dir_all(fixture.path().parent().unwrap()).unwrap();
    let outside = fixture.root.join("outside.bin");
    fs::write(&outside, CONTENT).unwrap();
    if symlink_file(&outside, fixture.path()).is_err() {
        return;
    }
    let mut calls = 0;

    let failure = acquire_with_content(&fixture, CONTENT, &mut calls)
        .err()
        .expect("reject reparse payload");

    assert_eq!(failure.kind(), MsvcErrorKind::UnsafeStorage);
    assert_eq!(calls, 0);
    assert_eq!(fs::read(outside).unwrap(), CONTENT);
}
