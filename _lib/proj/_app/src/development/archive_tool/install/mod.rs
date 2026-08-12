use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

use crate::development::ArchiveToolContract;
use sha2::{Digest, Sha256};

use super::filesystem::ensure_directory_chain;
use super::{
    ArchiveToolError, ArchiveToolErrorKind, ArchiveToolStore, Installation, ResolvedDefinition,
    ResolvedVerification, SourceVerification, Trust,
};
use cache::ArtifactLease;
use recipe::{NativeRecipe, Recipe};
use transaction::{ExclusiveFileLock, RecoveryOutcome, WorkKind};

mod archive;
mod cache;
mod recipe;
mod stage;
mod transaction;
mod transfer;

pub struct InstallRequest<'a> {
    data_root: &'a Path,
    cache_data_root: &'a Path,
    tool: &'static ArchiveToolContract,
    resolved: ResolvedDefinition,
}

impl<'a> InstallRequest<'a> {
    pub fn new(
        data_root: &'a Path,
        cache_data_root: &'a Path,
        tool: &'static ArchiveToolContract,
        resolved: ResolvedDefinition,
    ) -> Result<Self, ArchiveToolError> {
        if resolved.tool_name() != tool.name || !tool.accepts_exact_version(resolved.version()) {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::InvalidInstallRequest,
                "archive tool installation request does not match its contract",
            ));
        }
        if !data_root.is_absolute() || !cache_data_root.is_absolute() {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::InvalidInstallRequest,
                "archive tool installation roots must be absolute",
            ));
        }
        Ok(Self {
            data_root,
            cache_data_root,
            tool,
            resolved,
        })
    }

    pub fn data_root(&self) -> &Path {
        self.data_root
    }

    pub fn cache_data_root(&self) -> &Path {
        self.cache_data_root
    }

    pub fn tool(&self) -> &'static ArchiveToolContract {
        self.tool
    }

    pub fn resolved(&self) -> &ResolvedDefinition {
        &self.resolved
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveSource {
    tool_name: String,
    version: String,
    url: String,
    expected_sha256: Option<String>,
    verification: SourceVerification,
}

impl ArchiveSource {
    fn new(
        resolved: &ResolvedDefinition,
        url: impl Into<String>,
        expected_sha256: Option<&str>,
        verification: SourceVerification,
    ) -> Result<Self, ArchiveToolError> {
        let url = url.into();
        if url.is_empty() || url.trim() != url {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::InvalidInstallRequest,
                "archive source URL must be non-empty and trimmed",
            ));
        }
        let expected_sha256 = expected_sha256.map(|value| value.to_ascii_lowercase());
        if expected_sha256
            .as_deref()
            .is_some_and(|value| !is_lower_sha256(value))
        {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::InvalidInstallRequest,
                "archive source SHA-256 must contain exactly 64 lowercase hexadecimal digits",
            ));
        }
        if matches!(
            verification,
            SourceVerification::Github | SourceVerification::Project
        ) && expected_sha256.is_none()
        {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::InvalidInstallRequest,
                "a verified archive source must declare its SHA-256",
            ));
        }
        Ok(Self {
            tool_name: resolved.tool_name().to_owned(),
            version: resolved.version().to_owned(),
            url,
            expected_sha256,
            verification,
        })
    }

    pub(super) fn from_release(
        resolved: &ResolvedDefinition,
        url: impl Into<String>,
        expected_sha256: Option<&str>,
        verification: SourceVerification,
    ) -> Result<Self, ArchiveToolError> {
        Self::new(resolved, url, expected_sha256, verification)
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn expected_sha256(&self) -> Option<&str> {
        self.expected_sha256.as_deref()
    }

    pub fn verification(&self) -> SourceVerification {
        self.verification
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallOutcome {
    Ready,
    Recovered,
    Installed,
}

pub struct InstallResult {
    outcome: InstallOutcome,
    installation: Installation,
    trust: Trust,
    warnings: Vec<String>,
}

impl InstallResult {
    pub fn outcome(&self) -> InstallOutcome {
        self.outcome
    }

    pub fn installation(&self) -> &Installation {
        &self.installation
    }

    pub fn trust(&self) -> &Trust {
        &self.trust
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

pub fn ensure_installed<F>(
    request: InstallRequest<'_>,
    resolve_source: F,
) -> Result<InstallResult, ArchiveToolError>
where
    F: FnOnce(&ResolvedDefinition) -> Result<ArchiveSource, ArchiveToolError>,
{
    ensure_installed_with(request, resolve_source, &NativeRecipe, &mut |_, _| {})
}

#[doc(hidden)]
pub fn ensure_installed_observed<F>(
    request: InstallRequest<'_>,
    resolve_source: F,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<InstallResult, ArchiveToolError>
where
    F: FnOnce(&ResolvedDefinition) -> Result<ArchiveSource, ArchiveToolError>,
{
    ensure_installed_with(request, resolve_source, &NativeRecipe, progress)
}

fn ensure_installed_with<F>(
    request: InstallRequest<'_>,
    resolve_source: F,
    recipe: &dyn Recipe,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<InstallResult, ArchiveToolError>
where
    F: FnOnce(&ResolvedDefinition) -> Result<ArchiveSource, ArchiveToolError>,
{
    let locks = ensure_directory_chain(
        request.data_root,
        &["modules", "kernel", ".dev", "setup", "locks"],
        "archive tool installation lock",
    )?;
    // Recovery owns every interrupted work directory under this tool's
    // installation parent, not only the requested version.  The lock must
    // therefore protect the same tool-wide domain.
    let lock_path = installation_lock_path(&locks, request.tool);
    let _target_lock = ExclusiveFileLock::acquire(&lock_path, 3_000, Duration::from_millis(200))?;
    let installs = ensure_directory_chain(
        request.data_root,
        &[
            "modules",
            "kernel",
            ".dev",
            "setup",
            "export",
            request.tool.name,
            "installs",
        ],
        "archive tool installation parent",
    )?;
    let target = installs.join(request.resolved.version());
    let store = ArchiveToolStore::new(request.data_root, request.tool);
    let mut warnings = Vec::new();
    let mut validate = |root: &Path| archive_candidate(&store, &request.resolved, root);
    let recovery = transaction::recover(&target, &mut validate)?;
    warnings.extend(recovery.warnings);
    match recovery.outcome {
        RecoveryOutcome::Ready(installation) => {
            return finish(
                &store,
                &request.resolved,
                InstallOutcome::Ready,
                installation,
                warnings,
            );
        }
        RecoveryOutcome::Recovered(installation) => {
            return finish(
                &store,
                &request.resolved,
                InstallOutcome::Recovered,
                installation,
                warnings,
            );
        }
        RecoveryOutcome::Missing => {}
    }

    let source = resolve_source(&request.resolved)?;
    validate_source(&request.resolved, &source)?;
    let mut last_stage_error = None;
    for attempt in 0..2 {
        let lease = ArtifactLease::acquire(
            request.cache_data_root,
            request.tool,
            &request.resolved,
            &source,
        )?;
        let verified = lease.ensure(&source, progress)?;
        let work = transaction::work_path(&target, WorkKind::Work)?;
        let staged = transaction::work_path(&target, WorkKind::Partial)?;
        let stage_result = (|| {
            stage::create_fresh_directory(&work, "installation work directory")?;
            stage::create_fresh_directory(&staged, "staged installation directory")?;
            stage::payload(
                request.tool,
                &request.resolved,
                &source,
                &verified.file,
                &verified.sha256,
                &work,
                &staged,
                recipe,
            )
        })();
        let stage_result = cleanup_result(stage_result, &[work.clone()], &mut warnings);
        match stage_result {
            Ok(()) => {
                drop(verified);
                drop(lease);
                let (installation, publish_warnings) =
                    transaction::publish(&staged, &target, &mut validate)?;
                warnings.extend(publish_warnings);
                return finish(
                    &store,
                    &request.resolved,
                    InstallOutcome::Installed,
                    installation,
                    warnings,
                );
            }
            Err(error) if attempt == 0 => {
                let error = cleanup_error(error, &[staged], &mut warnings);
                last_stage_error = Some(error);
                drop(verified);
                lease.clear()?;
                warnings.push(format!(
                    "{} staging failed; the artifact cache was reset and installation retried once.",
                    request.tool.display_name
                ));
            }
            Err(error) => {
                return Err(cleanup_error(error, &[staged], &mut warnings));
            }
        }
    }
    Err(last_stage_error.unwrap_or_else(|| {
        ArchiveToolError::new(
            ArchiveToolErrorKind::InstallationFailed,
            format!(
                "installing {} exhausted the clean retry attempt",
                request.tool.display_name
            ),
        )
    }))
}

fn cleanup_result<T>(
    result: Result<T, ArchiveToolError>,
    paths: &[std::path::PathBuf],
    warnings: &mut Vec<String>,
) -> Result<T, ArchiveToolError> {
    let cleanup = transaction::remove_residues(paths);
    warnings.extend(cleanup.iter().cloned());
    match result {
        Ok(value) => Ok(value),
        Err(error) => Err(transaction::with_cleanup_warnings(error, &cleanup)),
    }
}

fn cleanup_error(
    error: ArchiveToolError,
    paths: &[std::path::PathBuf],
    warnings: &mut Vec<String>,
) -> ArchiveToolError {
    let cleanup = transaction::remove_residues(paths);
    warnings.extend(cleanup.iter().cloned());
    transaction::with_cleanup_warnings(error, &cleanup)
}

fn archive_candidate(
    store: &ArchiveToolStore<'_>,
    resolved: &ResolvedDefinition,
    root: &Path,
) -> Result<Option<Installation>, ArchiveToolError> {
    use std::fs;
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::Storage,
                format!(
                    "cannot inspect installation candidate '{}': {error}",
                    root.display()
                ),
            ));
        }
    };
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!(
                "installation candidate cannot be a reparse point: {}",
                root.display()
            ),
        ));
    }
    if metadata.is_file() {
        return Ok(None);
    }
    if !metadata.is_dir() {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!(
                "installation candidate must be a regular file or directory: {}",
                root.display()
            ),
        ));
    }
    let installation = match store.read_installation_at(resolved, root) {
        Ok(installation) => installation,
        Err(error) if invalid_archive_candidate(error.kind()) => return Ok(None),
        Err(error) => return Err(error),
    };
    match store.verify_hashes(&installation) {
        Ok(()) => Ok(Some(installation)),
        Err(error) if invalid_archive_candidate(error.kind()) => Ok(None),
        Err(error) => Err(error),
    }
}

fn invalid_archive_candidate(kind: ArchiveToolErrorKind) -> bool {
    matches!(
        kind,
        ArchiveToolErrorKind::InstallationUnavailable
            | ArchiveToolErrorKind::MissingStorage
            | ArchiveToolErrorKind::MetadataUnreadable
            | ArchiveToolErrorKind::MetadataStale
            | ArchiveToolErrorKind::DuplicateFileRecords
            | ArchiveToolErrorKind::MissingFileRecord
            | ArchiveToolErrorKind::InvalidFileRecord
            | ArchiveToolErrorKind::InstalledFileInvalid
    )
}

fn installation_lock_path(locks: &Path, tool: &ArchiveToolContract) -> std::path::PathBuf {
    locks.join(format!(
        "archive-install-{}.lock",
        format!("{:x}", Sha256::digest(tool.name.as_bytes()))
    ))
}

#[doc(hidden)]
pub fn transfer_archive(
    source: &OsStr,
    destination: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<u64, ArchiveToolError> {
    transfer::transfer(source, destination, progress)
}

pub(crate) fn remove_controlled_data(path: &Path) -> Result<(), ArchiveToolError> {
    transaction::remove_controlled(path)
}

pub(crate) fn cleanup_installation_data(paths: &[std::path::PathBuf]) -> Vec<String> {
    transaction::remove_residues(paths)
}

pub(crate) fn extract_vsix_contents(
    archive: &std::fs::File,
    destination: &Path,
) -> Result<(), ArchiveToolError> {
    archive::extract_contents_file(archive, destination)
}

pub(crate) struct InstallationTransaction<T, F>
where
    F: FnMut(&Path) -> Result<Option<T>, ArchiveToolError>,
{
    _lock: ExclusiveFileLock,
    target: std::path::PathBuf,
    validate: F,
}

impl<T, F> InstallationTransaction<T, F>
where
    F: FnMut(&Path) -> Result<Option<T>, ArchiveToolError>,
{
    pub(crate) fn open(
        data_root: &Path,
        tool_name: &str,
        target: std::path::PathBuf,
        validate: F,
    ) -> Result<Self, ArchiveToolError> {
        let locks = ensure_directory_chain(
            data_root,
            &["modules", "kernel", ".dev", "setup", "locks"],
            "installation lock",
        )?;
        let identity = format!("{:x}", Sha256::digest(tool_name.as_bytes()));
        let lock = ExclusiveFileLock::acquire(
            &locks.join(format!("install-{identity}.lock")),
            3_000,
            Duration::from_millis(200),
        )?;
        Ok(Self {
            _lock: lock,
            target,
            validate,
        })
    }

    pub(crate) fn recover(&mut self) -> Result<(Option<T>, bool, Vec<String>), ArchiveToolError> {
        let report = transaction::recover(&self.target, &mut self.validate)?;
        match report.outcome {
            RecoveryOutcome::Ready(value) => Ok((Some(value), false, report.warnings)),
            RecoveryOutcome::Recovered(value) => Ok((Some(value), true, report.warnings)),
            RecoveryOutcome::Missing => Ok((None, false, report.warnings)),
        }
    }

    pub(crate) fn work_path(&self) -> Result<std::path::PathBuf, ArchiveToolError> {
        transaction::work_path(&self.target, WorkKind::Work)
    }

    pub(crate) fn staged_path(&self) -> Result<std::path::PathBuf, ArchiveToolError> {
        transaction::work_path(&self.target, WorkKind::Partial)
    }

    pub(crate) fn publish(&mut self, staged: &Path) -> Result<(T, Vec<String>), ArchiveToolError> {
        transaction::publish(staged, &self.target, &mut self.validate)
    }
}

#[doc(hidden)]
pub fn test_archive(path: &Path) -> Result<(), ArchiveToolError> {
    archive::test(path)
}

#[doc(hidden)]
pub fn extract_archive(path: &Path, destination: &Path) -> Result<(), ArchiveToolError> {
    archive::extract(path, destination)
}

fn validate_source(
    resolved: &ResolvedDefinition,
    source: &ArchiveSource,
) -> Result<(), ArchiveToolError> {
    let matches_resolution = source.tool_name == resolved.tool_name()
        && source.version == resolved.version()
        && match resolved.verification() {
            ResolvedVerification::Published(expected) => {
                source.verification == expected
                    && resolved.source_sha256() == source.expected_sha256.as_deref()
            }
            ResolvedVerification::Unresolved => matches!(
                source.verification,
                SourceVerification::Github | SourceVerification::Unverified
            ),
        };
    if !matches_resolution
        || (!resolved.project_sha256().is_empty()
            && (source.verification != SourceVerification::Project
                || source.expected_sha256() != Some(resolved.project_sha256())))
    {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::InvalidInstallRequest,
            "archive source does not match the resolved tool definition",
        ));
    }
    Ok(())
}

fn finish(
    store: &ArchiveToolStore<'_>,
    resolved: &ResolvedDefinition,
    outcome: InstallOutcome,
    installation: Installation,
    warnings: Vec<String>,
) -> Result<InstallResult, ArchiveToolError> {
    let trust = store.trust(resolved, Some(&installation))?;
    store.publish_latest_selection(resolved, &installation)?;
    Ok(InstallResult {
        outcome,
        installation,
        trust,
        warnings,
    })
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
