use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::development::msvc::source::tests::{channel, manifest};

const MANIFEST_URL: &str = "https://download.visualstudio.microsoft.com/fixture/VisualStudio.vsman";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    definition: MsvcDefinition,
    channel: Vec<u8>,
    manifest: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-msvc-resolver-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).expect("create MSVC resolver fixture");
        Self {
            root,
            definition: MsvcDefinition::new("17").unwrap(),
            channel: serde_json::to_vec(&channel()).unwrap(),
            manifest: serde_json::to_vec(&manifest()).unwrap(),
        }
    }

    fn resolver(&self) -> MsvcResolver<'_> {
        MsvcResolver::new(&self.root, &self.definition)
    }

    fn documents(&self) -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([
            (self.definition.channel_url(), self.channel.clone()),
            (MANIFEST_URL.to_owned(), self.manifest.clone()),
        ])
    }

    fn manifest_path(&self) -> PathBuf {
        self.root
            .join("downloads/msvc/17/manifests/VisualStudio.aaaaaaaaaaaaaaaa.vsman")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct TransferFixture {
    documents: BTreeMap<String, Vec<u8>>,
    calls: Vec<String>,
}

impl TransferFixture {
    fn new(documents: BTreeMap<String, Vec<u8>>) -> Self {
        Self {
            documents,
            calls: Vec::new(),
        }
    }

    fn transfer(
        &mut self,
        source: &OsStr,
        destination: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<u64, MsvcError> {
        let source = source.to_string_lossy().into_owned();
        self.calls.push(source.clone());
        let content = self.documents.get(&source).ok_or_else(|| {
            error(
                MsvcErrorKind::InvalidSource,
                format!("fixture source is offline: {source}"),
            )
        })?;
        fs::write(destination, content).map_err(MsvcError::from)?;
        progress(content.len() as u64, Some(content.len() as u64));
        Ok(content.len() as u64)
    }
}

fn resolve(fixture: &Fixture, transfer: &mut TransferFixture) -> Result<MsvcRecipe, MsvcError> {
    let mut client =
        |source: &OsStr, destination: &Path, progress: &mut dyn FnMut(u64, Option<u64>)| {
            transfer.transfer(source, destination, progress)
        };
    fixture
        .resolver()
        .resolve_with(&mut client, &mut |_, _, _| {})
}

#[test]
fn source_documents_are_cached_with_the_actual_manifest_digest() {
    let fixture = Fixture::new();
    let mut transfer = TransferFixture::new(fixture.documents());

    let recipe = resolve(&fixture, &mut transfer).unwrap();

    assert_eq!(transfer.calls.len(), 2);
    assert_eq!(
        recipe.manifest_sha256(),
        format!("{:x}", Sha256::digest(&fixture.manifest))
    );
    assert_eq!(fs::read(fixture.manifest_path()).unwrap(), fixture.manifest);
    assert_eq!(
        fs::read_to_string(
            sibling_with_suffix(&fixture.manifest_path(), ".actual.sha256").unwrap()
        )
        .unwrap()
        .trim(),
        recipe.manifest_sha256()
    );
}

#[test]
fn a_valid_cache_keeps_setup_available_when_the_channel_is_offline() {
    let fixture = Fixture::new();
    resolve(&fixture, &mut TransferFixture::new(fixture.documents())).unwrap();
    let mut offline = TransferFixture::new(BTreeMap::new());

    let recipe = resolve(&fixture, &mut offline).unwrap();

    assert_eq!(recipe.tool_package_version(), "14.44.17.14");
    assert_eq!(offline.calls, [fixture.definition.channel_url()]);
}

#[test]
fn a_manifest_with_a_stale_digest_is_downloaded_again() {
    let fixture = Fixture::new();
    resolve(&fixture, &mut TransferFixture::new(fixture.documents())).unwrap();
    fs::write(fixture.manifest_path(), b"{\"packages\":[]}").unwrap();
    let mut transfer = TransferFixture::new(fixture.documents());

    resolve(&fixture, &mut transfer).unwrap();

    assert_eq!(transfer.calls.len(), 2);
    assert_eq!(fs::read(fixture.manifest_path()).unwrap(), fixture.manifest);
}

#[test]
fn invalid_cache_does_not_hide_a_source_failure() {
    let fixture = Fixture::new();
    let channel_path = fixture
        .root
        .join("downloads/msvc/17/manifests/channel.json");
    fs::create_dir_all(channel_path.parent().unwrap()).unwrap();
    fs::write(&channel_path, b"not-json").unwrap();
    let mut offline = TransferFixture::new(BTreeMap::new());

    let failure = resolve(&fixture, &mut offline).unwrap_err();

    assert_eq!(failure.kind(), MsvcErrorKind::InvalidSource);
    assert!(failure.to_string().contains("no valid cache"));
}
