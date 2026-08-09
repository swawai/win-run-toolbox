use std::error::Error;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::Arc;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
};

use crate::entry::{EntryIdentity, EntryIdentityLease};

use super::plan::{DataRootPlan, ordinal_text_eq};
use super::record::parse_entry_record;

pub(crate) struct DataRootBindingLease {
    _entry_file: Arc<EntryIdentityLease>,
    _data_root: EntryIdentityLease,
    _entry_record: File,
}

impl DataRootBindingLease {
    pub(crate) fn acquire(
        plan: &DataRootPlan,
        entry_file: Arc<EntryIdentityLease>,
    ) -> Result<Self, DataRootBindingLeaseError> {
        let target = plan.target();
        let expected_data_root_identity = expected_data_root_identity(plan);
        if entry_file.identity() != &target.identity {
            return Err(DataRootBindingLeaseError::new(format!(
                "pinned project entry identity does not match its DataRoot plan: {}",
                target.entry_file.display()
            )));
        }

        let data_root = EntryIdentityLease::acquire_directory(&target.data_root)
            .map_err(|error| binding_error("pin project DataRoot", &target.data_root, error))?;
        if let Some(expected) = expected_data_root_identity
            && data_root.identity() != expected
        {
            return Err(DataRootBindingLeaseError::new(format!(
                "project DataRoot directory changed before its binding was pinned: {}",
                target.data_root.display()
            )));
        }

        let record_path = target.data_root.join("_entry.json");
        let (entry_record, record) = open_entry_record(&record_path)?;
        if !record.matches_identity(&target.identity)
            || !ordinal_text_eq(&record.entry_name, &target.entry_name)
        {
            return Err(DataRootBindingLeaseError::new(format!(
                "project DataRoot binding changed before it was pinned: {}",
                record_path.display()
            )));
        }

        Ok(Self {
            _entry_file: entry_file,
            _data_root: data_root,
            _entry_record: entry_record,
        })
    }
}

fn expected_data_root_identity(plan: &DataRootPlan) -> Option<&EntryIdentity> {
    match plan {
        DataRootPlan::Direct {
            data_root_identity, ..
        } => Some(data_root_identity),
        DataRootPlan::ClaimCurrent {
            observed_directory_identity,
            ..
        }
        | DataRootPlan::ClaimRename {
            observed_directory_identity,
            ..
        }
        | DataRootPlan::MigrateLegacy {
            observed_directory_identity,
            ..
        }
        | DataRootPlan::ClaimMigrateLegacy {
            observed_directory_identity,
            ..
        } => Some(observed_directory_identity),
        DataRootPlan::Create { .. } => None,
    }
}

fn open_entry_record(
    path: &Path,
) -> Result<(File, super::record::EntryRecord), DataRootBindingLeaseError> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| binding_error("pin project DataRoot identity record", path, error))?;
    let metadata = file.metadata().map_err(|error| {
        binding_error(
            "inspect pinned project DataRoot identity record",
            path,
            error,
        )
    })?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(DataRootBindingLeaseError::new(format!(
            "project DataRoot identity record must be a regular file: {}",
            path.display()
        )));
    }

    let mut content = Vec::new();
    (&file).read_to_end(&mut content).map_err(|error| {
        binding_error("read pinned project DataRoot identity record", path, error)
    })?;
    let record = parse_entry_record(&content).map_err(|error| {
        DataRootBindingLeaseError::new(format!(
            "invalid pinned project DataRoot identity record '{}': {error}",
            path.display()
        ))
    })?;
    Ok((file, record))
}

fn binding_error(action: &str, path: &Path, error: impl fmt::Display) -> DataRootBindingLeaseError {
    DataRootBindingLeaseError::new(format!("cannot {action} '{}': {error}", path.display()))
}

#[derive(Debug)]
pub(crate) struct DataRootBindingLeaseError {
    message: String,
}

impl DataRootBindingLeaseError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for DataRootBindingLeaseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for DataRootBindingLeaseError {}
