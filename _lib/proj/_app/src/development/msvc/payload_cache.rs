use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Seek, SeekFrom};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
};

use crate::development::archive_tool::install::{remove_controlled_data, transfer_archive};
use crate::development::setup::storage::{ExclusiveFileLock, ensure_directory_chain};

use super::{MsvcDefinition, MsvcError, MsvcErrorKind, MsvcPayload, error};

const LOCK_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_PAYLOAD_BYTES: u64 = 12 * 1024 * 1024 * 1024;

type PayloadTransfer<'a> =
    dyn FnMut(&OsStr, &Path, &mut dyn FnMut(u64, Option<u64>)) -> Result<u64, MsvcError> + 'a;

pub struct VerifiedMsvcPayload {
    file: File,
    path: PathBuf,
    length: u64,
}

impl VerifiedMsvcPayload {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn try_clone(&self) -> Result<File, MsvcError> {
        self.file
            .try_clone()
            .map_err(|cause| storage("clone the verified MSVC payload", &self.path, cause))
    }
}

pub struct MsvcPayloadCache<'a> {
    cache_data_root: &'a Path,
    definition: &'a MsvcDefinition,
}

impl<'a> MsvcPayloadCache<'a> {
    pub fn new(cache_data_root: &'a Path, definition: &'a MsvcDefinition) -> Self {
        Self {
            cache_data_root,
            definition,
        }
    }

    pub fn acquire(
        &self,
        payload: &MsvcPayload,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<VerifiedMsvcPayload, MsvcError> {
        let mut transfer =
            |source: &OsStr, destination: &Path, report: &mut dyn FnMut(u64, Option<u64>)| {
                transfer_archive(source, destination, report).map_err(MsvcError::from)
            };
        self.acquire_with(payload, &mut transfer, progress)
    }

    fn acquire_with(
        &self,
        payload: &MsvcPayload,
        transfer: &mut PayloadTransfer<'_>,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<VerifiedMsvcPayload, MsvcError> {
        let cache_root = ensure_directory_chain(
            self.cache_data_root,
            &["downloads", "msvc", self.definition.channel(), "payloads"],
            "MSVC payload cache",
        )?;
        let locks =
            ensure_directory_chain(self.cache_data_root, &["_locks"], "MSVC artifact locks")?;
        let _lock = ExclusiveFileLock::acquire(
            &locks.join(format!("msvc-{}.lock", payload.sha256())),
            LOCK_TIMEOUT,
        )?;
        let payload_root = ensure_payload_root(&cache_root, payload.sha256())?;
        let path = payload_root.join(payload.leaf_name());

        if let Some(verified) = open_verified(&path, payload)? {
            return Ok(verified);
        }
        remove_invalid_entry(&path)?;
        if let Err(failure) = transfer(OsStr::new(payload.url()), &path, progress) {
            return Err(match remove_invalid_entry(&path) {
                Ok(()) => failure,
                Err(cleanup) => error(failure.kind, format!("{failure} Cleanup failed: {cleanup}")),
            });
        }
        match open_verified(&path, payload)? {
            Some(verified) => Ok(verified),
            None => {
                remove_invalid_entry(&path)?;
                Err(error(
                    MsvcErrorKind::DownloadFailed,
                    format!(
                        "Microsoft payload verification failed: {}",
                        payload.leaf_name()
                    ),
                ))
            }
        }
    }
}

fn ensure_payload_root(cache_root: &Path, sha256: &str) -> Result<PathBuf, MsvcError> {
    let path = cache_root.join(sha256);
    match fs::symlink_metadata(&path) {
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {}
        Err(cause) => return Err(storage("inspect the MSVC payload cache", &path, cause)),
        Ok(metadata) if metadata.is_dir() && !is_reparse(&metadata) => return Ok(path),
        Ok(metadata) if metadata.is_file() && !is_reparse(&metadata) => {
            fs::remove_file(&path)
                .map_err(|cause| storage("repair the MSVC payload cache", &path, cause))?;
        }
        Ok(_) => return Err(unsafe_storage("MSVC payload cache", &path)),
    }
    ensure_directory_chain(cache_root, &[sha256], "MSVC payload cache").map_err(MsvcError::from)
}

fn open_verified(
    path: &Path,
    payload: &MsvcPayload,
) -> Result<Option<VerifiedMsvcPayload>, MsvcError> {
    match fs::symlink_metadata(path) {
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(cause) => return Err(storage("inspect the MSVC payload", path, cause)),
        Ok(metadata) if (metadata.is_file() || metadata.is_dir()) && !is_reparse(&metadata) => {}
        Ok(_) => return Err(unsafe_storage("MSVC payload", path)),
    }
    if fs::symlink_metadata(path)
        .map_err(|cause| storage("inspect the MSVC payload", path, cause))?
        .is_dir()
    {
        return Ok(None);
    }
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|cause| storage("open the MSVC payload", path, cause))?;
    let metadata = file
        .metadata()
        .map_err(|cause| storage("inspect the MSVC payload", path, cause))?;
    let length = metadata.len();
    if !metadata.is_file() || is_reparse(&metadata) || length == 0 || length > MAX_PAYLOAD_BYTES {
        return Ok(None);
    }
    let mut reader = file
        .try_clone()
        .map_err(|cause| storage("clone the MSVC payload", path, cause))?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|cause| storage("seek the MSVC payload", path, cause))?;
    let mut digest = Sha256::new();
    let copied = std::io::copy(&mut BufReader::new(reader), &mut digest)
        .map_err(|cause| storage("hash the MSVC payload", path, cause))?;
    if copied != length || format!("{:x}", digest.finalize()) != payload.sha256() {
        return Ok(None);
    }
    Ok(Some(VerifiedMsvcPayload {
        file,
        path: path.to_path_buf(),
        length,
    }))
}

fn remove_invalid_entry(path: &Path) -> Result<(), MsvcError> {
    match fs::symlink_metadata(path) {
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(cause) => Err(storage("inspect the invalid MSVC payload", path, cause)),
        Ok(metadata) if is_reparse(&metadata) => Err(unsafe_storage("MSVC payload", path)),
        Ok(metadata) if metadata.is_file() => fs::remove_file(path)
            .map_err(|cause| storage("remove the invalid MSVC payload", path, cause)),
        Ok(metadata) if metadata.is_dir() => remove_controlled_data(path).map_err(MsvcError::from),
        Ok(_) => Err(unsafe_storage("MSVC payload", path)),
    }
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn unsafe_storage(subject: &str, path: &Path) -> MsvcError {
    error(
        MsvcErrorKind::UnsafeStorage,
        format!(
            "{subject} must be a regular filesystem entry: {}",
            path.display()
        ),
    )
}

fn storage(action: &str, path: &Path, cause: std::io::Error) -> MsvcError {
    error(
        MsvcErrorKind::Storage,
        format!("cannot {action} '{}': {cause}", path.display()),
    )
}

#[cfg(test)]
mod tests;
