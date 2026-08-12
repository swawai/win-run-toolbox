use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
};

use super::{RUSTUP_CHECKSUM_URL, RUSTUP_URL, RustDefinition, RustError, RustErrorKind, error};
use crate::development::archive_tool::install::{remove_controlled_data, transfer_bounded};
use crate::development::setup::storage::{ExclusiveFileLock, ensure_directory_chain};

const MAX_CHECKSUM_BYTES: u64 = 16 * 1024;
const MAX_INSTALLER_BYTES: u64 = 100 * 1024 * 1024;

type Transfer<'a> =
    dyn FnMut(&OsStr, &Path, u64, &mut dyn FnMut(u64, Option<u64>)) -> Result<u64, RustError> + 'a;

#[derive(Debug)]
pub struct VerifiedRustup {
    file: File,
    path: PathBuf,
    sha256: String,
}

impl VerifiedRustup {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    #[allow(dead_code)]
    pub(crate) fn file(&self) -> &File {
        &self.file
    }

    #[cfg(test)]
    fn try_clone(&self) -> File {
        self.file.try_clone().unwrap()
    }
}

pub struct RustupCache<'a> {
    cache_data_root: &'a Path,
    definition: &'a RustDefinition,
}

impl<'a> RustupCache<'a> {
    pub fn new(cache_data_root: &'a Path, definition: &'a RustDefinition) -> Self {
        Self {
            cache_data_root,
            definition,
        }
    }

    pub fn acquire(
        &self,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<VerifiedRustup, RustError> {
        let mut transfer = |source: &OsStr,
                            destination: &Path,
                            maximum: u64,
                            report: &mut dyn FnMut(u64, Option<u64>)| {
            transfer_bounded(source, destination, maximum, report).map_err(RustError::from)
        };
        self.acquire_with(
            OsStr::new(RUSTUP_URL),
            OsStr::new(RUSTUP_CHECKSUM_URL),
            &mut transfer,
            progress,
        )
    }

    fn acquire_with(
        &self,
        installer_source: &OsStr,
        checksum_source: &OsStr,
        transfer: &mut Transfer<'_>,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<VerifiedRustup, RustError> {
        let cache = cache_root(
            self.cache_data_root,
            self.definition,
            installer_source,
            checksum_source,
        )?;
        let locks =
            ensure_directory_chain(self.cache_data_root, &["_locks"], "Rust artifact locks")?;
        let identity = sha256_text(&format!(
            "{}\n{}",
            installer_source.to_string_lossy(),
            checksum_source.to_string_lossy()
        ));
        let _lock = ExclusiveFileLock::acquire(
            &locks.join(format!("rustup-{identity}.lock")),
            Duration::from_secs(60),
        )?;
        let checksum_path = cache.join("rustup-init.exe.sha256");
        let installer_path = cache.join("rustup-init.exe");
        let mut expected = read_checksum(&checksum_path)?;
        if expected.is_none() {
            expected = Some(refresh_checksum(
                checksum_source,
                &checksum_path,
                transfer,
                progress,
            )?);
        }
        let mut expected = expected.expect("checksum was resolved");
        if let Some(verified) = open_verified(&installer_path, &expected)? {
            return Ok(verified);
        }
        remove_invalid(&installer_path)?;
        transfer(
            installer_source,
            &installer_path,
            MAX_INSTALLER_BYTES,
            progress,
        )?;
        if let Some(verified) = open_verified(&installer_path, &expected)? {
            return Ok(verified);
        }
        expected = refresh_checksum(checksum_source, &checksum_path, transfer, progress)?;
        if let Some(verified) = open_verified(&installer_path, &expected)? {
            return Ok(verified);
        }
        remove_invalid(&installer_path)?;
        Err(error(
            RustErrorKind::DownloadFailed,
            "SHA-256 verification failed for rustup-init.exe.",
        ))
    }
}

fn cache_root(
    cache_data_root: &Path,
    definition: &RustDefinition,
    installer_source: &OsStr,
    checksum_source: &OsStr,
) -> Result<PathBuf, RustError> {
    let identity = sha256_text(
        &[
            "swawkit.proj-dev.rustup-source.v0",
            definition.recipe_version(),
            &installer_source.to_string_lossy(),
            &checksum_source.to_string_lossy(),
        ]
        .join("\n"),
    );
    Ok(ensure_directory_chain(
        cache_data_root,
        &[
            "downloads",
            "rust",
            "rustup-init",
            definition.host(),
            &format!("{}-{}", definition.recipe_version(), &identity[..16]),
        ],
        "Rust rustup cache",
    )?)
}

fn refresh_checksum(
    source: &OsStr,
    destination: &Path,
    transfer: &mut Transfer<'_>,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<String, RustError> {
    remove_invalid(destination)?;
    transfer(source, destination, MAX_CHECKSUM_BYTES, progress)?;
    read_checksum(destination)?.ok_or_else(|| {
        let _ = remove_invalid(destination);
        error(
            RustErrorKind::DownloadFailed,
            "The official rustup-init SHA-256 sidecar is invalid.",
        )
    })
}

fn read_checksum(path: &Path) -> Result<Option<String>, RustError> {
    let file = match open_regular(path, MAX_CHECKSUM_BYTES) {
        Ok(file) => file,
        Err(failure) if failure.kind == RustErrorKind::MissingStorage => return Ok(None),
        Err(failure) => return Err(failure),
    };
    let mut text = String::new();
    BufReader::new(file)
        .take(MAX_CHECKSUM_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|cause| storage("read the rustup checksum", path, cause))?;
    let text = text.trim();
    let digest = text.get(..64).filter(|digest| {
        digest.bytes().all(|byte| {
            byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) || (b'A'..=b'F').contains(&byte)
        }) && text
            .as_bytes()
            .get(64)
            .is_none_or(|byte| byte.is_ascii_whitespace())
    });
    Ok(digest.map(str::to_ascii_lowercase))
}

fn open_verified(path: &Path, expected: &str) -> Result<Option<VerifiedRustup>, RustError> {
    let mut file = match open_regular(path, MAX_INSTALLER_BYTES) {
        Ok(file) => file,
        Err(failure) if failure.kind == RustErrorKind::MissingStorage => return Ok(None),
        Err(failure) => return Err(failure),
    };
    let length = file
        .metadata()
        .map_err(|cause| storage("inspect rustup-init", path, cause))?
        .len();
    if length == 0 {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|cause| storage("seek rustup-init", path, cause))?;
    let mut digest = Sha256::new();
    std::io::copy(&mut BufReader::new(&file), &mut digest)
        .map_err(|cause| storage("hash rustup-init", path, cause))?;
    let actual = format!("{:x}", digest.finalize());
    if actual != expected {
        return Ok(None);
    }
    Ok(Some(VerifiedRustup {
        file,
        path: path.to_path_buf(),
        sha256: actual,
    }))
}

fn open_regular(path: &Path, maximum: u64) -> Result<File, RustError> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|cause| storage("open Rust artifact", path, cause))?;
    let metadata = file
        .metadata()
        .map_err(|cause| storage("inspect Rust artifact", path, cause))?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() > maximum
    {
        return Err(error(
            RustErrorKind::UnsafeStorage,
            format!(
                "Rust artifact must be a bounded regular file: {}",
                path.display()
            ),
        ));
    }
    Ok(file)
}

fn remove_invalid(path: &Path) -> Result<(), RustError> {
    match fs::symlink_metadata(path) {
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(cause) => Err(storage("inspect invalid Rust artifact", path, cause)),
        Ok(metadata) if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
            Err(error(
                RustErrorKind::UnsafeStorage,
                format!(
                    "Rust artifact cannot be a reparse point: {}",
                    path.display()
                ),
            ))
        }
        Ok(metadata) if metadata.is_file() => fs::remove_file(path)
            .map_err(|cause| storage("remove invalid Rust artifact", path, cause)),
        Ok(metadata) if metadata.is_dir() => remove_controlled_data(path).map_err(RustError::from),
        Ok(_) => Err(error(
            RustErrorKind::UnsafeStorage,
            format!("unsupported Rust artifact entry: {}", path.display()),
        )),
    }
}

fn storage(action: &str, path: &Path, cause: std::io::Error) -> RustError {
    error(
        if cause.kind() == std::io::ErrorKind::NotFound {
            RustErrorKind::MissingStorage
        } else {
            RustErrorKind::Storage
        },
        format!("cannot {action} '{}': {cause}", path.display()),
    )
}

fn sha256_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

#[cfg(test)]
#[path = "source/tests.rs"]
mod tests;
