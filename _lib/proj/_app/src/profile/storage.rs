use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::{EntryProfileRecord, ProfileError, error::ProfileReadError};

pub(super) fn read_record(path: &Path) -> Result<(EntryProfileRecord, String), ProfileReadError> {
    validate_publication_target(path)?;
    let content = fs::read(path).map_err(|error| {
        ProfileReadError::new(format!(
            "cannot read entry profile '{}': {error}",
            path.display()
        ))
    })?;
    let revision = revision(&content);
    serde_json::from_slice(&content)
        .map(|record| (record, revision.clone()))
        .map_err(|error| {
            ProfileReadError::with_revision(
                format!("invalid entry profile JSON: {error}"),
                revision,
            )
        })
}

pub(super) fn revision(content: &[u8]) -> String {
    format!("sha256-{:x}", Sha256::digest(content))
}

pub(super) fn validate_data_root(data_root: &Path) -> Result<(), ProfileError> {
    let metadata = fs::symlink_metadata(data_root).map_err(|error| {
        ProfileError::new(format!(
            "cannot inspect entry profile DataRoot '{}': {error}",
            data_root.display()
        ))
    })?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ProfileError::new(format!(
            "entry profile DataRoot must be a regular directory: {}",
            data_root.display()
        )));
    }
    Ok(())
}

pub(super) fn validate_publication_target(path: &Path) -> Result<(), ProfileError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(ProfileError::new(format!(
                "cannot inspect entry profile '{}': {error}",
                path.display()
            )));
        }
    };
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ProfileError::new(format!(
            "entry profile must be a regular file: {}",
            path.display()
        )));
    }
    Ok(())
}
