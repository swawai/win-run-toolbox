use std::error::Error;
use std::fmt;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::entry::EntryIdentity;

use super::record::{EntryRecordFingerprint, EntryRecordState, read_entry_record_with_fingerprint};

#[derive(Debug, Clone)]
pub struct DataRootInventory {
    directory: PathBuf,
    roots: Vec<DataRootSnapshot>,
}

impl DataRootInventory {
    pub fn scan(directory: &Path) -> Result<Self, DataRootInventoryError> {
        let directory = std::path::absolute(directory).map_err(|error| {
            DataRootInventoryError::new(format!(
                "invalid project data directory '{}': {error}",
                directory.display()
            ))
        })?;
        if !directory.exists() {
            return Ok(Self {
                directory,
                roots: Vec::new(),
            });
        }
        reject_reparse_point(&directory, "project data directory")?;
        if !directory.is_dir() {
            return Err(DataRootInventoryError::new(format!(
                "project data directory is not a directory: {}",
                directory.display()
            )));
        }

        let mut roots = Vec::new();
        let entries = fs::read_dir(&directory).map_err(|error| {
            DataRootInventoryError::new(format!(
                "cannot enumerate project data directory '{}': {error}",
                directory.display()
            ))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                DataRootInventoryError::new(format!(
                    "cannot enumerate project data directory '{}': {error}",
                    directory.display()
                ))
            })?;
            let name = entry.file_name();
            if !name
                .to_string_lossy()
                .to_ascii_lowercase()
                .starts_with("proj.")
            {
                continue;
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                DataRootInventoryError::new(format!(
                    "cannot inspect project DataRoot '{}': {error}",
                    path.display()
                ))
            })?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(DataRootInventoryError::new(format!(
                    "project DataRoot cannot be a reparse point: {}",
                    path.display()
                )));
            }
            if !metadata.is_dir() {
                continue;
            }
            let directory_identity = EntryIdentity::read_directory(&path).map_err(|error| {
                DataRootInventoryError::new(format!(
                    "cannot read project DataRoot identity '{}': {error}",
                    path.display()
                ))
            })?;
            let record_read = read_entry_record_with_fingerprint(&path);
            roots.push(DataRootSnapshot {
                record: record_read.state,
                record_fingerprint: record_read.fingerprint,
                directory_identity,
                path,
            });
        }
        roots.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(Self { directory, roots })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn roots(&self) -> &[DataRootSnapshot] {
        &self.roots
    }

    #[cfg(test)]
    pub(crate) fn from_snapshots(
        directory: PathBuf,
        snapshots: Vec<(PathBuf, EntryRecordState)>,
    ) -> Self {
        Self {
            directory,
            roots: snapshots
                .into_iter()
                .enumerate()
                .map(|(index, (path, record))| DataRootSnapshot {
                    record_fingerprint: EntryRecordFingerprint::from_state(&record),
                    directory_identity: EntryIdentity::from_parts(
                        r"\\?\volume{91cf565a-694f-4232-be2d-368578d28629}",
                        format!("{:032x}", index + 1),
                    )
                    .expect("synthetic DataRoot identity"),
                    path,
                    record,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DataRootSnapshot {
    pub(crate) path: PathBuf,
    pub(crate) record: EntryRecordState,
    record_fingerprint: EntryRecordFingerprint,
    directory_identity: EntryIdentity,
}

impl DataRootSnapshot {
    pub(crate) fn directory_identity(&self) -> &EntryIdentity {
        &self.directory_identity
    }

    pub(crate) fn record_revision(&self) -> String {
        self.record_fingerprint.revision()
    }
}

fn reject_reparse_point(path: &Path, label: &str) -> Result<(), DataRootInventoryError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        DataRootInventoryError::new(format!(
            "cannot inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(DataRootInventoryError::new(format!(
            "{label} cannot be a reparse point: {}",
            path.display()
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRootInventoryError {
    message: String,
}

impl DataRootInventoryError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for DataRootInventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for DataRootInventoryError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn scans_only_project_directories_and_preserves_invalid_records() {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-data-inventory-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("proj.valid")).expect("create valid root");
        fs::create_dir(root.join("PROJ.invalid")).expect("create invalid root");
        fs::create_dir(root.join("cache")).expect("create unrelated directory");
        fs::write(root.join("proj.file"), "not a directory").expect("write unrelated file");
        fs::write(root.join("PROJ.invalid/_entry.json"), "not json").expect("write invalid record");

        let inventory = DataRootInventory::scan(&root).expect("scan inventory");
        assert_eq!(inventory.roots.len(), 2);
        assert!(
            inventory
                .roots
                .iter()
                .any(|root| matches!(root.record, EntryRecordState::Missing { .. }))
        );
        assert!(
            inventory
                .roots
                .iter()
                .any(|root| matches!(root.record, EntryRecordState::Invalid { .. }))
        );

        fs::remove_dir_all(root).expect("remove fixture");
    }
}
