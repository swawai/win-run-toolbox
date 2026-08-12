use std::fs;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::development::rust::HOST;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    cache: PathBuf,
    source: PathBuf,
    checksum: PathBuf,
    definition: RustDefinition,
}

impl Fixture {
    fn new(content: &[u8]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "swawkit-rustup-cache-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let cache = root.join("cache");
        fs::create_dir_all(&cache).unwrap();
        let source = root.join("rustup-init.exe");
        let checksum = root.join("rustup-init.exe.sha256");
        fs::write(&source, content).unwrap();
        fs::write(&checksum, format!("{}  rustup-init.exe\n", digest(content))).unwrap();
        Self {
            root,
            cache,
            source,
            checksum,
            definition: RustDefinition::new("stable", "minimal", HOST).unwrap(),
        }
    }

    fn acquire(&self) -> Result<VerifiedRustup, RustError> {
        let cache = RustupCache::new(&self.cache, &self.definition);
        let mut transfer = |source: &OsStr,
                            destination: &Path,
                            maximum: u64,
                            report: &mut dyn FnMut(u64, Option<u64>)| {
            transfer_bounded(source, destination, maximum, report).map_err(RustError::from)
        };
        cache.acquire_with(
            self.source.as_os_str(),
            self.checksum.as_os_str(),
            &mut transfer,
            &mut |_, _| {},
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn verified_cache_is_reused_offline_and_guarded_against_mutation() {
    let fixture = Fixture::new(b"official-rustup");
    let verified = fixture.acquire().unwrap();
    let path = verified.path().to_path_buf();
    fs::remove_file(&fixture.source).unwrap();
    fs::remove_file(&fixture.checksum).unwrap();

    let repeated = fixture.acquire().unwrap();

    assert_eq!(repeated.path(), path);
    assert_eq!(repeated.sha256(), digest(b"official-rustup"));
    assert!(OpenOptions::new().write(true).open(&path).is_err());
    assert!(fs::rename(&path, path.with_extension("moved")).is_err());
    let mut clone = repeated.try_clone();
    clone.write_all(b"x").unwrap_err();
}

#[test]
fn stale_sidecar_is_refreshed_before_a_valid_download_is_rejected() {
    let fixture = Fixture::new(b"version-one");
    fixture.acquire().unwrap();
    let cache = cache_root(
        &fixture.cache,
        &fixture.definition,
        fixture.source.as_os_str(),
        fixture.checksum.as_os_str(),
    )
    .unwrap();
    fs::remove_file(cache.join("rustup-init.exe")).unwrap();
    fs::write(&fixture.source, b"version-two").unwrap();
    fs::write(
        &fixture.checksum,
        format!("{}  rustup-init.exe\n", digest(b"version-two")),
    )
    .unwrap();

    let verified = fixture.acquire().unwrap();

    assert_eq!(verified.sha256(), digest(b"version-two"));
}

#[test]
fn invalid_sidecar_and_mismatched_content_fail_closed() {
    let fixture = Fixture::new(b"rustup");
    fs::write(&fixture.checksum, b"not-a-digest\n").unwrap();
    assert_eq!(
        fixture.acquire().unwrap_err().kind(),
        RustErrorKind::DownloadFailed
    );

    fs::write(&fixture.checksum, format!("{}\n", "0".repeat(64))).unwrap();
    assert_eq!(
        fixture.acquire().unwrap_err().kind(),
        RustErrorKind::DownloadFailed
    );
}

fn digest(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}
