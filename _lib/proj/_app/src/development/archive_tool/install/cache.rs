use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Seek, SeekFrom};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};

use crate::development::ArchiveToolContract;

use super::super::filesystem::{ensure_directory_chain, is_reparse, regular_file_digest};
use super::super::{ArchiveToolError, ArchiveToolErrorKind, ResolvedDefinition};
use super::archive::test_file as test_archive_file;
use super::transaction::{ExclusiveFileLock, remove_controlled};
use super::{ArchiveSource, test_archive, transfer_archive};

const MAX_ARCHIVE_BYTES: u64 = 12 * 1024 * 1024 * 1024;
const LOCK_ATTEMPTS: usize = 3_000;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(200);

pub(super) struct ArtifactLease {
    _lock: ExclusiveFileLock,
    cache_root: PathBuf,
    archive_path: PathBuf,
}

pub(super) struct VerifiedArchive {
    pub(super) file: File,
    pub(super) sha256: String,
}

impl ArtifactLease {
    pub(super) fn acquire(
        cache_data_root: &Path,
        tool: &ArchiveToolContract,
        resolved: &ResolvedDefinition,
        source: &ArchiveSource,
    ) -> Result<Self, ArchiveToolError> {
        let downloads = ensure_directory_chain(
            cache_data_root,
            &["downloads", tool.name],
            "archive tool cache",
        )?;
        let canonical_identity = [
            tool.source_identity(resolved.version()),
            tool.archive_subdir.to_owned(),
            resolved.project_sha256().to_owned(),
        ]
        .join("\n");
        let legacy_key = digest_text(&canonical_identity);
        let legacy_root = downloads.join(format!("{}-{}", resolved.version(), &legacy_key[..16]));
        let source_identity = [
            canonical_identity,
            source.url().to_owned(),
            source.verification().as_str().to_owned(),
            source.expected_sha256().unwrap_or("").to_owned(),
        ]
        .join("\n");
        let source_key = digest_text(&source_identity);
        let cache_root = downloads.join(format!("{}-{}", resolved.version(), &source_key[..16]));
        let lock_root = ensure_directory_chain(cache_data_root, &["_locks"], "artifact lock")?;
        let lock_path = artifact_lock_path(&lock_root, &cache_root)?;
        let lock = ExclusiveFileLock::acquire(&lock_path, LOCK_ATTEMPTS, LOCK_RETRY_DELAY)?;
        ensure_cache_directory(&cache_root)?;
        let archive_name = tool.archive_name(resolved.version());
        clean_orphaned_downloads(&cache_root, &archive_name)?;
        let archive_path = cache_root.join(&archive_name);
        reject_archive_directory(&archive_path)?;
        if !cached_archive_exists(&archive_path)? {
            import_legacy_archive(
                &lock_root,
                &legacy_root,
                &cache_root,
                &archive_name,
                source.expected_sha256(),
                &archive_path,
            )?;
        }
        Ok(Self {
            _lock: lock,
            cache_root,
            archive_path,
        })
    }

    pub(super) fn ensure(
        &self,
        source: &ArchiveSource,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<VerifiedArchive, ArchiveToolError> {
        if cached_archive_exists(&self.archive_path)? && !self.valid(source.expected_sha256())? {
            remove_controlled(&self.archive_path)?;
        }
        if !cached_archive_exists(&self.archive_path)? {
            transfer_archive(OsStr::new(source.url()), &self.archive_path, progress)?;
        }
        // This handle is the trust boundary: it is opened before hashing and
        // denies writers/deleters until staging has completely extracted it.
        let file = open_archive_guard(&self.archive_path)?;
        let actual = guarded_archive_digest(&file, &self.archive_path)?;
        if source
            .expected_sha256()
            .is_some_and(|expected| expected != actual)
        {
            drop(file);
            remove_controlled(&self.archive_path)?;
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::DownloadFailed,
                format!(
                    "SHA-256 verification failed for: {}",
                    self.archive_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
            ));
        }
        if let Err(error) = test_archive_file(&file) {
            drop(file);
            remove_controlled(&self.archive_path)?;
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::ArchiveInvalid,
                format!(
                    "downloaded archive is not a valid ZIP file '{}': {error}",
                    self.archive_path.display()
                ),
            ));
        }
        Ok(VerifiedArchive {
            file,
            sha256: actual,
        })
    }

    pub(super) fn clear(&self) -> Result<(), ArchiveToolError> {
        remove_controlled(&self.cache_root)?;
        ensure_cache_directory(&self.cache_root)
    }

    fn valid(&self, expected_sha256: Option<&str>) -> Result<bool, ArchiveToolError> {
        match test_archive(&self.archive_path) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    ArchiveToolErrorKind::ArchiveInvalid | ArchiveToolErrorKind::MissingStorage
                ) =>
            {
                return Ok(false);
            }
            Err(error) => return Err(error),
        }
        if let Some(expected) = expected_sha256 {
            let actual = match regular_file_digest(
                &self.archive_path,
                "archive tool cache file",
                MAX_ARCHIVE_BYTES,
            ) {
                Ok((_, actual)) => actual,
                Err(error) if error.kind() == ArchiveToolErrorKind::MissingStorage => {
                    return Ok(false);
                }
                Err(error) => return Err(error),
            };
            return Ok(actual == expected);
        }
        Ok(true)
    }
}

fn import_legacy_archive(
    lock_root: &Path,
    legacy_root: &Path,
    cache_root: &Path,
    archive_name: &str,
    expected_sha256: Option<&str>,
    destination: &Path,
) -> Result<(), ArchiveToolError> {
    let Some(expected_sha256) = expected_sha256 else {
        return Ok(());
    };
    if same_path(legacy_root, cache_root) {
        return Ok(());
    }
    let legacy_lock_path = artifact_lock_path(lock_root, legacy_root)?;
    let _legacy_lock =
        ExclusiveFileLock::acquire(&legacy_lock_path, LOCK_ATTEMPTS, LOCK_RETRY_DELAY)?;
    let Some(legacy_archive) = legacy_archive(legacy_root, archive_name)? else {
        return Ok(());
    };
    if !archive_matches(&legacy_archive, expected_sha256)? {
        return Ok(());
    }
    transfer_archive(legacy_archive.as_os_str(), destination, &mut |_, _| {})?;
    if !archive_matches(destination, expected_sha256)? {
        remove_controlled(destination)?;
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::DownloadFailed,
            "the migrated legacy artifact did not preserve its expected SHA-256",
        ));
    }
    Ok(())
}

fn legacy_archive(
    cache_root: &Path,
    archive_name: &str,
) -> Result<Option<PathBuf>, ArchiveToolError> {
    match fs::symlink_metadata(cache_root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(cache_error("inspect", cache_root, error)),
        Ok(metadata) if metadata.is_dir() && !is_reparse(&metadata) => {}
        Ok(metadata) if is_reparse(&metadata) => {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::UnsafeStorage,
                format!(
                    "legacy archive tool cache cannot be a reparse point: {}",
                    cache_root.display()
                ),
            ));
        }
        Ok(_) => return Ok(None),
    }
    let archive = cache_root.join(archive_name);
    cached_archive_exists(&archive).map(|exists| exists.then_some(archive))
}

fn archive_matches(path: &Path, expected_sha256: &str) -> Result<bool, ArchiveToolError> {
    let actual = match regular_file_digest(path, "archive tool cache file", MAX_ARCHIVE_BYTES) {
        Ok((_, actual)) => actual,
        Err(error) if error.kind() == ArchiveToolErrorKind::MissingStorage => return Ok(false),
        Err(error) => return Err(error),
    };
    if actual != expected_sha256 {
        return Ok(false);
    }
    match test_archive(path) {
        Ok(()) => Ok(true),
        Err(error)
            if matches!(
                error.kind(),
                ArchiveToolErrorKind::ArchiveInvalid | ArchiveToolErrorKind::MissingStorage
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

fn cached_archive_exists(path: &Path) -> Result<bool, ArchiveToolError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(cache_error("inspect", path, error)),
        Ok(metadata) if metadata.is_file() && !is_reparse(&metadata) => Ok(true),
        Ok(metadata) if is_reparse(&metadata) => Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!(
                "cached archive cannot be a reparse point: {}",
                path.display()
            ),
        )),
        Ok(_) => Ok(false),
    }
}

fn artifact_lock_path(lock_root: &Path, cache_root: &Path) -> Result<PathBuf, ArchiveToolError> {
    Ok(lock_root.join(format!(
        "{}.lock",
        digest_text(&artifact_lock_identity(cache_root)?)
    )))
}

fn artifact_lock_identity(cache_root: &Path) -> Result<String, ArchiveToolError> {
    std::path::absolute(cache_root)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| {
            ArchiveToolError::new(
                ArchiveToolErrorKind::InvalidInstallRequest,
                format!(
                    "cannot resolve archive tool cache '{}': {error}",
                    cache_root.display()
                ),
            )
        })
}

fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn ensure_cache_directory(path: &Path) -> Result<(), ArchiveToolError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !is_reparse(&metadata) => Ok(()),
        Ok(metadata) if metadata.is_file() && !is_reparse(&metadata) => {
            fs::remove_file(path).map_err(|error| cache_error("remove", path, error))?;
            fs::create_dir(path).map_err(|error| cache_error("create", path, error))
        }
        Ok(_) => Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!(
                "archive tool cache must be a regular directory: {}",
                path.display()
            ),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|error| cache_error("create", path, error))
        }
        Err(error) => Err(cache_error("inspect", path, error)),
    }
}

fn clean_orphaned_downloads(root: &Path, archive_name: &str) -> Result<(), ArchiveToolError> {
    let prefix = format!(".{archive_name}.");
    for entry in fs::read_dir(root).map_err(|error| cache_error("inspect", root, error))? {
        let entry = entry.map_err(|error| cache_error("inspect", root, error))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(&prefix) || !name.ends_with(".tmp") {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| cache_error("inspect", &entry.path(), error))?;
        if !metadata.is_file() || is_reparse(&metadata) {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::UnsafeStorage,
                format!(
                    "download temporary file must be a regular file: {}",
                    entry.path().display()
                ),
            ));
        }
        fs::remove_file(entry.path())
            .map_err(|error| cache_error("remove", &entry.path(), error))?;
    }
    Ok(())
}

fn reject_archive_directory(path: &Path) -> Result<(), ArchiveToolError> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if is_reparse(&metadata) {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!(
                "cached archive cannot be a reparse point: {}",
                path.display()
            ),
        ));
    }
    if metadata.is_dir() {
        remove_controlled(path)?;
    }
    Ok(())
}

fn digest_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn open_archive_guard(path: &Path) -> Result<File, ArchiveToolError> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| cache_error("open", path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| cache_error("inspect", path, error))?;
    if !metadata.is_file() || is_reparse(&metadata) || metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!(
                "cached archive must be a bounded regular file: {}",
                path.display()
            ),
        ));
    }
    Ok(file)
}

fn guarded_archive_digest(file: &File, path: &Path) -> Result<String, ArchiveToolError> {
    let length = file
        .metadata()
        .map_err(|error| cache_error("inspect", path, error))?
        .len();
    if length == 0 {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::ArchiveInvalid,
            format!("cached archive cannot be empty: {}", path.display()),
        ));
    }
    let mut reader = file
        .try_clone()
        .map_err(|error| cache_error("clone", path, error))?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| cache_error("seek", path, error))?;
    let mut digest = Sha256::new();
    let copied = std::io::copy(&mut BufReader::new(reader), &mut digest)
        .map_err(|error| cache_error("hash", path, error))?;
    if copied != length {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::Storage,
            format!(
                "cached archive changed while it was being hashed: {}",
                path.display()
            ),
        ));
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn cache_error(action: &str, path: &Path, error: std::io::Error) -> ArchiveToolError {
    ArchiveToolError::new(
        ArchiveToolErrorKind::Storage,
        format!(
            "cannot {action} archive tool cache '{}': {error}",
            path.display()
        ),
    )
}

#[cfg(test)]
mod tests;
