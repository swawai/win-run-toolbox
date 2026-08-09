use std::error::Error;
use std::fmt;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::plan::DataRootPlan;
use super::record::{EntryRecordWriteError, publish_entry_record};

pub(crate) fn execute_plan(plan: &DataRootPlan) -> Result<(), DataRootExecutionError> {
    let target = plan.target();
    match plan {
        DataRootPlan::Direct { .. } => return Ok(()),
        DataRootPlan::Create { .. } => {
            fs::create_dir(&target.data_root)
                .map_err(|error| execution_error("create DataRoot", &target.data_root, error))?;
        }
        DataRootPlan::ClaimCurrent { .. } => {
            require_directory(&target.data_root, "claim target")?;
        }
        DataRootPlan::ClaimRename {
            source_data_root, ..
        } => {
            move_data_root(source_data_root, &target.data_root, "claim rename")?;
        }
    }

    publish_entry_record(
        &target.data_root,
        &target.entry_name,
        &target.entry_file,
        &target.identity,
    )?;
    Ok(())
}

fn move_data_root(
    source: &Path,
    target: &Path,
    operation: &str,
) -> Result<(), DataRootExecutionError> {
    if target.exists() {
        return Err(DataRootExecutionError::new(format!(
            "{operation} target already exists: {}",
            target.display()
        )));
    }
    require_directory(source, &format!("{operation} source"))?;
    fs::rename(source, target).map_err(|error| execution_error(operation, source, error))
}

fn require_directory(path: &Path, label: &str) -> Result<(), DataRootExecutionError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        DataRootExecutionError::new(format!(
            "cannot inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(DataRootExecutionError::new(format!(
            "{label} cannot be a reparse point: {}",
            path.display()
        )));
    }
    if !metadata.is_dir() {
        return Err(DataRootExecutionError::new(format!(
            "{label} disappeared: {}",
            path.display()
        )));
    }
    Ok(())
}

fn execution_error(action: &str, path: &Path, error: std::io::Error) -> DataRootExecutionError {
    DataRootExecutionError::new(format!("cannot {action} '{}': {error}", path.display()))
}

#[derive(Debug)]
pub(crate) struct DataRootExecutionError {
    message: String,
}

impl DataRootExecutionError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for DataRootExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for DataRootExecutionError {}

impl From<EntryRecordWriteError> for DataRootExecutionError {
    fn from(error: EntryRecordWriteError) -> Self {
        Self::new(error.to_string())
    }
}
