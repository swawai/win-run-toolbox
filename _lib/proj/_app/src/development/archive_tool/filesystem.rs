use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

use super::{ArchiveToolError, ArchiveToolErrorKind};

pub(super) const MAX_SELECTION_BYTES: u64 = 16 * 1024;
pub(crate) const MAX_METADATA_BYTES: u64 = 1024 * 1024;

pub(super) fn optional_directory_chain(
    root: &Path,
    components: &[&str],
    subject: &str,
) -> Result<Option<PathBuf>, ArchiveToolError> {
    regular_directory(root, subject)?;
    let mut path = root.to_path_buf();
    for component in components {
        validate_segment(component, subject)?;
        path.push(component);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(storage(subject, &path, "inspect", error)),
            Ok(metadata) if metadata.is_dir() && !is_reparse(&metadata) => {}
            Ok(_) => {
                return Err(ArchiveToolError::new(
                    ArchiveToolErrorKind::UnsafeStorage,
                    format!("{subject} must be a regular directory: {}", path.display()),
                ));
            }
        }
    }
    Ok(Some(path))
}

pub(crate) fn directory_chain(
    root: &Path,
    components: &[&str],
    subject: &str,
) -> Result<PathBuf, ArchiveToolError> {
    optional_directory_chain(root, components, subject)?.ok_or_else(|| {
        ArchiveToolError::new(
            ArchiveToolErrorKind::MissingStorage,
            format!("{subject} does not exist below '{}'", root.display()),
        )
    })
}

pub(super) fn ensure_directory_chain(
    root: &Path,
    components: &[&str],
    subject: &str,
) -> Result<PathBuf, ArchiveToolError> {
    regular_directory(root, subject)?;
    let mut path = root.to_path_buf();
    for component in components {
        validate_segment(component, subject)?;
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !is_reparse(&metadata) => {}
            Ok(_) => {
                return Err(ArchiveToolError::new(
                    ArchiveToolErrorKind::UnsafeStorage,
                    format!("{subject} must be a regular directory: {}", path.display()),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_or_validate_directory(&path, subject)?;
            }
            Err(error) => return Err(storage(subject, &path, "inspect", error)),
        }
    }
    Ok(path)
}

fn create_or_validate_directory(path: &Path, subject: &str) -> Result<(), ArchiveToolError> {
    match fs::create_dir(path) {
        Ok(()) => regular_directory(path, subject),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // Another process can create this component between the preceding
            // metadata check and create_dir.  Revalidate the winner instead of
            // turning a safe first-use race into a setup failure.
            regular_directory(path, subject)
        }
        Err(error) => Err(storage(subject, path, "create", error)),
    }
}

pub(super) fn optional_regular_file(path: &Path, subject: &str) -> Result<bool, ArchiveToolError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(storage(subject, path, "inspect", error)),
        Ok(metadata) if metadata.is_file() && !is_reparse(&metadata) => Ok(true),
        Ok(_) => Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!("{subject} must be a regular file: {}", path.display()),
        )),
    }
}

pub(crate) fn child_file(
    root: &Path,
    relative: &str,
    subject: &str,
) -> Result<PathBuf, ArchiveToolError> {
    regular_directory(root, subject)?;
    let components: Vec<_> = Path::new(relative).components().collect();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!("unsafe {subject} relative path '{relative}'"),
        ));
    }
    let mut path = root.to_path_buf();
    for component in &components[..components.len() - 1] {
        let Component::Normal(name) = component else {
            unreachable!()
        };
        path.push(name);
        regular_directory(&path, subject)?;
    }
    let Component::Normal(name) = components[components.len() - 1] else {
        unreachable!()
    };
    path.push(name);
    Ok(path)
}

pub(crate) fn read_json<T: DeserializeOwned>(
    path: &Path,
    subject: &str,
    max_bytes: u64,
) -> Result<T, ArchiveToolError> {
    let mut file = open_regular_at_most(path, subject, max_bytes)?;
    let capacity = file
        .metadata()
        .map(|metadata| metadata.len() as usize)
        .unwrap_or(0);
    let mut content = Vec::with_capacity(capacity);
    file.read_to_end(&mut content)
        .map_err(|error| storage(subject, path, "read", error))?;
    serde_json::from_slice(&content).map_err(|error| {
        ArchiveToolError::new(
            ArchiveToolErrorKind::InvalidDocument,
            format!("cannot parse {subject} '{}': {error}", path.display()),
        )
    })
}

pub(crate) fn verify_regular_file(
    path: &Path,
    subject: &str,
    expected_length: u64,
    expected_sha256: &str,
) -> Result<(), ArchiveToolError> {
    let file = open_regular_at_most(path, subject, expected_length)?;
    let metadata = file
        .metadata()
        .map_err(|error| storage(subject, path, "inspect", error))?;
    if expected_length == 0 || metadata.len() != expected_length {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::FileMismatch,
            format!(
                "{subject} length does not match its published metadata: {}",
                path.display()
            ),
        ));
    }
    let mut digest = Sha256::new();
    std::io::copy(&mut BufReader::new(file), &mut digest)
        .map_err(|error| storage(subject, path, "hash", error))?;
    if format!("{:x}", digest.finalize()) != expected_sha256 {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::FileMismatch,
            format!(
                "{subject} SHA-256 does not match its published metadata: {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

pub(crate) fn verify_regular_file_length(
    path: &Path,
    subject: &str,
    expected_length: u64,
) -> Result<(), ArchiveToolError> {
    let file = open_regular_at_most(path, subject, expected_length)?;
    let metadata = file
        .metadata()
        .map_err(|error| storage(subject, path, "inspect", error))?;
    if expected_length == 0 || metadata.len() != expected_length {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::FileMismatch,
            format!(
                "{subject} length does not match its published metadata: {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

pub(crate) fn regular_file_digest(
    path: &Path,
    subject: &str,
    max_bytes: u64,
) -> Result<(u64, String), ArchiveToolError> {
    let file = open_regular_at_most(path, subject, max_bytes)?;
    let length = file
        .metadata()
        .map_err(|error| storage(subject, path, "inspect", error))?
        .len();
    if length == 0 {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::FileMismatch,
            format!("{subject} cannot be empty: {}", path.display()),
        ));
    }
    let mut digest = Sha256::new();
    std::io::copy(&mut BufReader::new(file), &mut digest)
        .map_err(|error| storage(subject, path, "hash", error))?;
    Ok((length, format!("{:x}", digest.finalize())))
}

pub(crate) fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_segment(component: &str, subject: &str) -> Result<(), ArchiveToolError> {
    if !matches!(
        Path::new(component)
            .components()
            .collect::<Vec<_>>()
            .as_slice(),
        [Component::Normal(_)]
    ) {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!("unsafe {subject} path segment '{component}'"),
        ));
    }
    Ok(())
}

pub(super) fn regular_directory(path: &Path, subject: &str) -> Result<(), ArchiveToolError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| storage(subject, path, "inspect", error))?;
    if !metadata.is_dir() || is_reparse(&metadata) {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!("{subject} must be a regular directory: {}", path.display()),
        ));
    }
    Ok(())
}

fn open_regular_at_most(
    path: &Path,
    subject: &str,
    max_bytes: u64,
) -> Result<File, ArchiveToolError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| storage(subject, path, "open", error))?;
    let metadata = file
        .metadata()
        .map_err(|error| storage(subject, path, "inspect", error))?;
    if !metadata.is_file() || is_reparse(&metadata) || metadata.len() > max_bytes {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!(
                "{subject} must be a bounded regular file: {}",
                path.display()
            ),
        ));
    }
    Ok(file)
}

fn storage(subject: &str, path: &Path, action: &str, error: std::io::Error) -> ArchiveToolError {
    let kind = if error.kind() == std::io::ErrorKind::NotFound {
        ArchiveToolErrorKind::MissingStorage
    } else {
        ArchiveToolErrorKind::Storage
    };
    ArchiveToolError::new(
        kind,
        format!("cannot {action} {subject} '{}': {error}", path.display()),
    )
}

pub(super) fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn a_concurrent_directory_winner_is_revalidated() {
        let root = std::env::temp_dir().join(format!(
            "swawkit-archive-directory-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).unwrap();
        let path = root.join("winner");
        fs::create_dir(&path).unwrap();

        create_or_validate_directory(&path, "test directory").unwrap();

        fs::remove_dir_all(root).unwrap();
    }
}
