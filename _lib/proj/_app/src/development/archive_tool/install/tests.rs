use std::fs;
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
use zip::write::SimpleFileOptions;

use super::recipe::Recipe;
use super::*;
use crate::development::archive_tool::{ArchiveToolRequest, ArchiveToolStore};
use crate::development::{BUN, PWSH};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

pub(super) struct Fixture {
    root: PathBuf,
    pub(super) data_root: PathBuf,
    pub(super) cache_root: PathBuf,
    pub(super) archive: PathBuf,
}

impl Fixture {
    pub(super) fn new(tool: &ArchiveToolContract, version: &str, executable: &[u8]) -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-archive-install-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let data_root = root.join("data");
        let cache_root = root.join("cache");
        fs::create_dir_all(&data_root).unwrap();
        fs::create_dir_all(&cache_root).unwrap();
        let archive = root.join(tool.archive_name(version));
        let file = fs::File::create(&archive).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let path = if tool.archive_subdir.is_empty() {
            tool.executable.to_owned()
        } else {
            format!("{}/{}", tool.archive_subdir, tool.executable)
        };
        writer
            .start_file(path, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(executable).unwrap();
        writer.finish().unwrap();
        Self {
            root,
            data_root,
            cache_root,
            archive,
        }
    }

    pub(super) fn digest(&self) -> String {
        format!("{:x}", Sha256::digest(fs::read(&self.archive).unwrap()))
    }

    pub(super) fn publish_latest_selection(
        &self,
        tool: &ArchiveToolContract,
        version: &str,
        digest: &str,
        verification: SourceVerification,
    ) {
        let root = self
            .data_root
            .join("modules/kernel/.dev/setup/export")
            .join(tool.name);
        fs::create_dir_all(&root).unwrap();
        let selection = serde_json::json!({
            "schema": tool.selection_schema,
            "selector": "latest",
            "version": version,
            "sourceSha256": digest,
            "sourceVerification": verification,
        });
        fs::write(
            root.join(".swawkit-dev-selection.json"),
            serde_json::to_vec_pretty(&selection).unwrap(),
        )
        .unwrap();
    }

    fn resolved(
        &self,
        tool: &'static ArchiveToolContract,
        version: &str,
        project_sha256: &str,
    ) -> ResolvedDefinition {
        let request = ArchiveToolRequest::new(tool, version, project_sha256).unwrap();
        ArchiveToolStore::new(&self.data_root, tool)
            .resolve(&request)
            .unwrap()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(super) struct FixtureRecipe;

impl Recipe for FixtureRecipe {
    fn prepare(
        &self,
        tool: &ArchiveToolContract,
        staged_root: &Path,
    ) -> Result<(), ArchiveToolError> {
        if tool.name == BUN.name {
            fs::write(
                staged_root.join("bunx.cmd"),
                b"@echo off\r\n\"%~dp0bun.exe\" x %*\r\n",
            )
            .unwrap();
        }
        Ok(())
    }

    fn validate(
        &self,
        tool: &ArchiveToolContract,
        _resolved: &ResolvedDefinition,
        staged_root: &Path,
    ) -> Result<(), ArchiveToolError> {
        let content = fs::read(staged_root.join(tool.executable)).unwrap();
        if content == b"fixture" {
            Ok(())
        } else {
            Err(ArchiveToolError::new(
                ArchiveToolErrorKind::ProbeFailed,
                "fixture version probe rejected the staged executable",
            ))
        }
    }
}

pub(super) struct FailingRecipe;

impl Recipe for FailingRecipe {
    fn prepare(
        &self,
        _tool: &ArchiveToolContract,
        _staged_root: &Path,
    ) -> Result<(), ArchiveToolError> {
        Err(ArchiveToolError::new(
            ArchiveToolErrorKind::ProbeFailed,
            "injected staging failure",
        ))
    }

    fn validate(
        &self,
        _tool: &ArchiveToolContract,
        _resolved: &ResolvedDefinition,
        _staged_root: &Path,
    ) -> Result<(), ArchiveToolError> {
        unreachable!("the injected prepare failure stops before validation")
    }
}

fn install_parent(fixture: &Fixture, tool: &ArchiveToolContract) -> PathBuf {
    fixture
        .data_root
        .join("modules/kernel/.dev/setup/export")
        .join(tool.name)
        .join("installs")
}

fn transaction_residues(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            name.contains(".work-") || name.contains(".partial-")
        })
        .collect()
}

struct AnyPayloadRecipe;

impl Recipe for AnyPayloadRecipe {
    fn prepare(
        &self,
        tool: &ArchiveToolContract,
        staged_root: &Path,
    ) -> Result<(), ArchiveToolError> {
        FixtureRecipe.prepare(tool, staged_root)
    }

    fn validate(
        &self,
        _tool: &ArchiveToolContract,
        _resolved: &ResolvedDefinition,
        _staged_root: &Path,
    ) -> Result<(), ArchiveToolError> {
        Ok(())
    }
}

#[test]
fn bun_installation_is_published_and_reused_without_resolving_a_source() {
    let fixture = Fixture::new(&BUN, "1.2.15", b"fixture");
    let digest = fixture.digest();
    let resolved = fixture.resolved(&BUN, "1.2.15", &digest);
    let source = ArchiveSource::new(
        &resolved,
        fixture.archive.to_string_lossy(),
        Some(&digest),
        SourceVerification::Project,
    )
    .unwrap();
    let request = InstallRequest::new(
        &fixture.data_root,
        &fixture.cache_root,
        &BUN,
        resolved.clone(),
    )
    .unwrap();

    let installed =
        ensure_installed_with(request, |_| Ok(source), &FixtureRecipe, &mut |_, _| {}).unwrap();

    assert_eq!(installed.outcome(), InstallOutcome::Installed);
    assert_eq!(installed.trust().level().as_str(), "pinned");
    assert_eq!(
        fs::read(installed.installation().root().join("bun.exe")).unwrap(),
        b"fixture"
    );
    assert_eq!(
        fs::read(installed.installation().root().join("bunx.cmd")).unwrap(),
        b"@echo off\r\n\"%~dp0bun.exe\" x %*\r\n"
    );

    fs::remove_file(&fixture.archive).unwrap();
    let request =
        InstallRequest::new(&fixture.data_root, &fixture.cache_root, &BUN, resolved).unwrap();
    let ready = ensure_installed_with(
        request,
        |_| panic!("a ready exact installation must remain fully offline"),
        &FixtureRecipe,
        &mut |_, _| {},
    )
    .unwrap();
    assert_eq!(ready.outcome(), InstallOutcome::Ready);
}

#[test]
fn unverified_power_shell_records_the_actual_archive_digest() {
    let fixture = Fixture::new(&PWSH, "7.6.4", b"fixture");
    let actual = fixture.digest();
    let resolved = fixture.resolved(&PWSH, "7.6.4", "");
    let source = ArchiveSource::new(
        &resolved,
        fixture.archive.to_string_lossy(),
        None,
        SourceVerification::Unverified,
    )
    .unwrap();
    let request =
        InstallRequest::new(&fixture.data_root, &fixture.cache_root, &PWSH, resolved).unwrap();

    let installed =
        ensure_installed_with(request, |_| Ok(source), &FixtureRecipe, &mut |_, _| {}).unwrap();

    assert_eq!(installed.installation().metadata().source_sha256(), actual);
    assert_eq!(
        installed.installation().metadata().source_verification(),
        SourceVerification::Unverified
    );
}

#[test]
fn a_source_that_conflicts_with_the_resolution_is_rejected() {
    let fixture = Fixture::new(&BUN, "1.2.15", b"fixture");
    let digest = fixture.digest();
    let resolved = fixture.resolved(&BUN, "1.2.15", &digest);
    let source = ArchiveSource::new(
        &resolved,
        fixture.archive.to_string_lossy(),
        Some(&digest),
        SourceVerification::Github,
    )
    .unwrap();
    let request =
        InstallRequest::new(&fixture.data_root, &fixture.cache_root, &BUN, resolved).unwrap();

    let error = match ensure_installed_with(request, |_| Ok(source), &FixtureRecipe, &mut |_, _| {})
    {
        Ok(_) => panic!("conflicting source verification must fail"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), ArchiveToolErrorKind::InvalidInstallRequest);
}

#[test]
fn a_release_source_cannot_be_replayed_for_another_definition() {
    let fixture = Fixture::new(&BUN, "1.2.15", b"fixture");
    let original = fixture.resolved(&BUN, "1.2.15", "");
    let replayed = fixture.resolved(&BUN, "1.2.16", "");
    let source = ArchiveSource::new(
        &original,
        fixture.archive.to_string_lossy(),
        None,
        SourceVerification::Unverified,
    )
    .unwrap();
    let request =
        InstallRequest::new(&fixture.data_root, &fixture.cache_root, &BUN, replayed).unwrap();

    let error = ensure_installed_with(request, |_| Ok(source), &FixtureRecipe, &mut |_, _| {})
        .err()
        .expect("a source capability belongs to one exact definition");

    assert_eq!(error.kind(), ArchiveToolErrorKind::InvalidInstallRequest);
    assert!(!install_parent(&fixture, &BUN).join("1.2.16").exists());
}

#[test]
fn latest_unverified_digest_reinstalls_missing_content_without_becoming_trusted() {
    let fixture = Fixture::new(&BUN, "1.2.15", b"fixture");
    let digest = fixture.digest();
    fixture.publish_latest_selection(&BUN, "1.2.15", &digest, SourceVerification::Unverified);
    let resolved = fixture.resolved(&BUN, "latest", "");
    let source = ArchiveSource::new(
        &resolved,
        fixture.archive.to_string_lossy(),
        Some(&digest),
        SourceVerification::Unverified,
    )
    .unwrap();
    let request =
        InstallRequest::new(&fixture.data_root, &fixture.cache_root, &BUN, resolved).unwrap();

    let installed =
        ensure_installed_with(request, |_| Ok(source), &FixtureRecipe, &mut |_, _| {}).unwrap();

    assert_eq!(installed.outcome(), InstallOutcome::Installed);
    assert_eq!(installed.trust().level().as_str(), "unpinned");
    assert_eq!(installed.installation().metadata().source_sha256(), digest);
    assert_eq!(
        installed.installation().metadata().source_verification(),
        SourceVerification::Unverified
    );
}

#[test]
fn a_digest_verified_legacy_power_shell_cache_is_imported_offline() {
    let fixture = Fixture::new(&BUN, "1.2.15", b"fixture");
    let digest = fixture.digest();
    let resolved = fixture.resolved(&BUN, "1.2.15", &digest);
    let canonical_identity = [
        BUN.source_identity(resolved.version()),
        BUN.archive_subdir.to_owned(),
        resolved.project_sha256().to_owned(),
    ]
    .join("\n");
    let key = format!("{:x}", Sha256::digest(canonical_identity.as_bytes()));
    let legacy_root = fixture
        .cache_root
        .join("downloads")
        .join(BUN.name)
        .join(format!("{}-{}", resolved.version(), &key[..16]));
    fs::create_dir_all(&legacy_root).unwrap();
    fs::copy(
        &fixture.archive,
        legacy_root.join(BUN.archive_name(resolved.version())),
    )
    .unwrap();
    let source_path = fixture.archive.to_string_lossy().into_owned();
    let source = ArchiveSource::new(
        &resolved,
        &source_path,
        Some(&digest),
        SourceVerification::Project,
    )
    .unwrap();
    fs::remove_file(&fixture.archive).unwrap();
    let request =
        InstallRequest::new(&fixture.data_root, &fixture.cache_root, &BUN, resolved).unwrap();

    let installed =
        ensure_installed_with(request, |_| Ok(source), &FixtureRecipe, &mut |_, _| {}).unwrap();

    assert_eq!(installed.outcome(), InstallOutcome::Installed);
    assert_eq!(
        fs::read(installed.installation().executable()).unwrap(),
        b"fixture"
    );
}

#[test]
fn unverified_sources_do_not_reuse_legacy_or_different_url_caches() {
    let fixture = Fixture::new(&BUN, "1.2.15", b"legacy");
    let resolved = fixture.resolved(&BUN, "1.2.15", "");
    let canonical_identity = [
        BUN.source_identity(resolved.version()),
        BUN.archive_subdir.to_owned(),
        resolved.project_sha256().to_owned(),
    ]
    .join("\n");
    let key = format!("{:x}", Sha256::digest(canonical_identity.as_bytes()));
    let legacy_root = fixture
        .cache_root
        .join("downloads")
        .join(BUN.name)
        .join(format!("{}-{}", resolved.version(), &key[..16]));
    fs::create_dir_all(&legacy_root).unwrap();
    fs::copy(
        &fixture.archive,
        legacy_root.join(BUN.archive_name(resolved.version())),
    )
    .unwrap();

    let source_b = fixture.root.join("source-b.zip");
    write_archive(&source_b, &BUN, b"source-b");
    let request = InstallRequest::new(
        &fixture.data_root,
        &fixture.cache_root,
        &BUN,
        resolved.clone(),
    )
    .unwrap();
    let source = ArchiveSource::new(
        &resolved,
        source_b.to_string_lossy(),
        None,
        SourceVerification::Unverified,
    )
    .unwrap();
    let first =
        ensure_installed_with(request, |_| Ok(source), &AnyPayloadRecipe, &mut |_, _| {}).unwrap();
    assert_eq!(
        fs::read(first.installation().executable()).unwrap(),
        b"source-b"
    );
    let target = first.installation().root().to_path_buf();
    drop(first);
    fs::remove_dir_all(target).unwrap();

    let source_c = fixture.root.join("source-c.zip");
    write_archive(&source_c, &BUN, b"source-c");
    let source = ArchiveSource::new(
        &resolved,
        source_c.to_string_lossy(),
        None,
        SourceVerification::Unverified,
    )
    .unwrap();
    let request =
        InstallRequest::new(&fixture.data_root, &fixture.cache_root, &BUN, resolved).unwrap();
    let second =
        ensure_installed_with(request, |_| Ok(source), &AnyPayloadRecipe, &mut |_, _| {}).unwrap();
    assert_eq!(
        fs::read(second.installation().executable()).unwrap(),
        b"source-c"
    );
}

fn write_archive(path: &Path, tool: &ArchiveToolContract, executable: &[u8]) {
    let file = fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let entry = if tool.archive_subdir.is_empty() {
        tool.executable.to_owned()
    } else {
        format!("{}/{}", tool.archive_subdir, tool.executable)
    };
    writer
        .start_file(entry, SimpleFileOptions::default())
        .unwrap();
    writer.write_all(executable).unwrap();
    writer.finish().unwrap();
}

#[test]
fn one_tool_uses_one_installation_lock_across_versions() {
    let root = std::env::temp_dir().join(format!(
        "swawkit-archive-install-lock-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    let bun_1215 = installation_lock_path(&root, &BUN);
    let bun_future = installation_lock_path(&root, &BUN);
    let pwsh = installation_lock_path(&root, &PWSH);

    assert_eq!(bun_1215, bun_future);
    assert_ne!(bun_1215, pwsh);
    let first = ExclusiveFileLock::acquire(&bun_1215, 1, std::time::Duration::ZERO).unwrap();
    assert!(
        ExclusiveFileLock::acquire(&bun_future, 1, std::time::Duration::ZERO).is_err(),
        "another version must not enter recovery while this tool is being installed"
    );
    drop(first);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_failed_stage_leaves_no_work_or_partial_directory() {
    let fixture = Fixture::new(&PWSH, "7.6.4", b"fixture");
    let digest = fixture.digest();
    let resolved = fixture.resolved(&PWSH, "7.6.4", &digest);
    let source = ArchiveSource::new(
        &resolved,
        fixture.archive.to_string_lossy(),
        Some(&digest),
        SourceVerification::Project,
    )
    .unwrap();
    let request =
        InstallRequest::new(&fixture.data_root, &fixture.cache_root, &PWSH, resolved).unwrap();

    let error = match ensure_installed_with(request, |_| Ok(source), &FailingRecipe, &mut |_, _| {})
    {
        Ok(_) => panic!("the injected staging failure must fail installation"),
        Err(error) => error,
    };

    assert_eq!(error.kind(), ArchiveToolErrorKind::ProbeFailed);
    assert!(error.to_string().contains("injected staging failure"));
    assert!(transaction_residues(&install_parent(&fixture, &PWSH)).is_empty());
}

#[test]
fn a_cleanup_failure_is_attached_without_hiding_the_primary_error() {
    let fixture = Fixture::new(&PWSH, "7.6.4", b"fixture");
    let residue = fixture.root.join("locked-residue");
    fs::create_dir(&residue).unwrap();
    let locked = residue.join("locked.txt");
    fs::write(&locked, b"locked").unwrap();
    let _guard = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&locked)
        .unwrap();
    let primary = ArchiveToolError::new(ArchiveToolErrorKind::ProbeFailed, "primary failure");
    let mut warnings = Vec::new();

    let error = cleanup_result::<()>(Err(primary), &[residue], &mut warnings).unwrap_err();

    assert_eq!(error.kind(), ArchiveToolErrorKind::ProbeFailed);
    assert!(error.to_string().contains("primary failure"));
    assert!(error.to_string().contains("Cleanup warnings:"));
    assert_eq!(warnings.len(), 1);
}

#[test]
fn an_ordinary_file_target_is_removed_and_reinstalled() {
    let fixture = Fixture::new(&PWSH, "7.6.4", b"fixture");
    let digest = fixture.digest();
    let resolved = fixture.resolved(&PWSH, "7.6.4", &digest);
    let parent = install_parent(&fixture, &PWSH);
    fs::create_dir_all(&parent).unwrap();
    let target = parent.join(resolved.version());
    fs::write(&target, b"interrupted non-directory target").unwrap();
    let source = ArchiveSource::new(
        &resolved,
        fixture.archive.to_string_lossy(),
        Some(&digest),
        SourceVerification::Project,
    )
    .unwrap();
    let request =
        InstallRequest::new(&fixture.data_root, &fixture.cache_root, &PWSH, resolved).unwrap();

    let installed =
        ensure_installed_with(request, |_| Ok(source), &FixtureRecipe, &mut |_, _| {}).unwrap();

    assert_eq!(installed.outcome(), InstallOutcome::Installed);
    assert!(target.is_dir());
    assert_eq!(
        fs::read(installed.installation().executable()).unwrap(),
        b"fixture"
    );
}

#[test]
fn an_ordinary_file_backup_is_ignored_while_a_valid_backup_is_restored() {
    let fixture = Fixture::new(&PWSH, "7.6.4", b"fixture");
    let digest = fixture.digest();
    let resolved = fixture.resolved(&PWSH, "7.6.4", &digest);
    let source = ArchiveSource::new(
        &resolved,
        fixture.archive.to_string_lossy(),
        Some(&digest),
        SourceVerification::Project,
    )
    .unwrap();
    let request = InstallRequest::new(
        &fixture.data_root,
        &fixture.cache_root,
        &PWSH,
        resolved.clone(),
    )
    .unwrap();
    let installed =
        ensure_installed_with(request, |_| Ok(source), &FixtureRecipe, &mut |_, _| {}).unwrap();
    let target = installed.installation().root().to_path_buf();
    drop(installed);
    let valid_backup = target.with_file_name(format!(
        "{}.backup-20260801T0101010000000Z-11111111111111111111111111111111",
        target.file_name().unwrap().to_string_lossy()
    ));
    let ordinary_backup = target.with_file_name(format!(
        "{}.backup-ordinary-file",
        target.file_name().unwrap().to_string_lossy()
    ));
    fs::rename(&target, &valid_backup).unwrap();
    fs::write(&ordinary_backup, b"not an installation directory").unwrap();
    let request =
        InstallRequest::new(&fixture.data_root, &fixture.cache_root, &PWSH, resolved).unwrap();

    let recovered = ensure_installed_with(
        request,
        |_| panic!("a valid backup must recover without resolving a source"),
        &FixtureRecipe,
        &mut |_, _| {},
    )
    .unwrap();

    assert_eq!(recovered.outcome(), InstallOutcome::Recovered);
    assert_eq!(
        fs::read(recovered.installation().executable()).unwrap(),
        b"fixture"
    );
    assert!(!ordinary_backup.exists());
}
