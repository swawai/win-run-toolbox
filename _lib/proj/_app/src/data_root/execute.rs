use std::error::Error;
use std::fmt;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::entry::{EntryIdentity, EntryIdentityLease};

use super::plan::DataRootPlan;
use super::record::{EntryRecordWriteError, publish_entry_record};

pub(crate) fn execute_plan(
    plan: &DataRootPlan,
) -> Result<EntryIdentityLease, DataRootExecutionError> {
    execute_plan_with(plan, |data_root, entry_name, entry_file, identity| {
        publish_entry_record(data_root, entry_name, entry_file, identity).map_err(Into::into)
    })
}

fn execute_plan_with(
    plan: &DataRootPlan,
    publish: impl FnOnce(&Path, &str, &Path, &EntryIdentity) -> Result<(), DataRootExecutionError>,
) -> Result<EntryIdentityLease, DataRootExecutionError> {
    let target = plan.target();
    let structural_change = match plan {
        DataRootPlan::Direct { .. } | DataRootPlan::ClaimCurrent { .. } => None,
        DataRootPlan::Create { .. } => {
            fs::create_dir(&target.data_root)
                .map_err(|error| execution_error("create DataRoot", &target.data_root, error))?;
            let identity = EntryIdentity::read_directory(&target.data_root).map_err(|error| {
                DataRootExecutionError::new(format!(
                    "cannot identify the created DataRoot '{}': {error}; it remains unavailable and was not removed because its identity could not be proven",
                    target.data_root.display()
                ))
            })?;
            Some(StructuralChange::Created {
                path: target.data_root.clone(),
                identity,
            })
        }
        DataRootPlan::ClaimRename {
            source_data_root,
            observed_directory_identity,
            ..
        } => {
            require_directory_identity(
                source_data_root,
                "claim rename source",
                observed_directory_identity,
            )?;
            move_data_root(source_data_root, &target.data_root, "claim rename")?;
            Some(StructuralChange::Renamed {
                source: source_data_root.clone(),
                target: target.data_root.clone(),
                identity: observed_directory_identity.clone(),
            })
        }
    };

    let data_root = EntryIdentityLease::acquire_directory(&target.data_root).map_err(|error| {
        execution_error_with_optional_rollback(
            structural_change.as_ref(),
            DataRootExecutionError::new(format!(
                "cannot pin project DataRoot before publication '{}': {error}",
                target.data_root.display()
            )),
        )
    })?;
    let expected_identity = structural_change
        .as_ref()
        .map(StructuralChange::identity)
        .or_else(|| expected_directory_identity(plan));
    if let Some(expected) = expected_identity
        && data_root.identity() != expected
    {
        drop(data_root);
        return Err(execution_error_with_optional_rollback(
            structural_change.as_ref(),
            DataRootExecutionError::new(format!(
                "project DataRoot changed before its binding was published: {}",
                target.data_root.display()
            )),
        ));
    }
    if matches!(plan, DataRootPlan::Direct { .. }) {
        return Ok(data_root);
    }

    if let Err(error) = publish(
        &target.data_root,
        &target.entry_name,
        &target.entry_file,
        &target.identity,
    ) {
        drop(data_root);
        return Err(execution_error_with_optional_rollback(
            structural_change.as_ref(),
            error,
        ));
    }
    Ok(data_root)
}

fn expected_directory_identity(plan: &DataRootPlan) -> Option<&EntryIdentity> {
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
        } => Some(observed_directory_identity),
        DataRootPlan::Create { .. } => None,
    }
}

enum StructuralChange {
    Created {
        path: std::path::PathBuf,
        identity: EntryIdentity,
    },
    Renamed {
        source: std::path::PathBuf,
        target: std::path::PathBuf,
        identity: EntryIdentity,
    },
}

impl StructuralChange {
    fn identity(&self) -> &EntryIdentity {
        match self {
            Self::Created { identity, .. } | Self::Renamed { identity, .. } => identity,
        }
    }
}

fn execution_error_with_optional_rollback(
    change: Option<&StructuralChange>,
    error: DataRootExecutionError,
) -> DataRootExecutionError {
    match change {
        Some(change) => execution_error_with_rollback(change, error),
        None => error,
    }
}

fn execution_error_with_rollback(
    change: &StructuralChange,
    error: DataRootExecutionError,
) -> DataRootExecutionError {
    match rollback(change) {
        Ok(()) => error,
        Err(rollback) => DataRootExecutionError::new(format!(
            "{error}; additionally, cannot roll back the DataRoot structural change: {rollback}"
        )),
    }
}

fn rollback(change: &StructuralChange) -> Result<(), DataRootExecutionError> {
    match change {
        StructuralChange::Created { path, identity } => {
            require_directory_identity(path, "created rollback target", identity)?;
            fs::remove_dir(path)
                .map_err(|error| execution_error("remove created DataRoot", path, error))
        }
        StructuralChange::Renamed {
            source,
            target,
            identity,
        } => {
            match fs::symlink_metadata(source) {
                Ok(_) => {
                    return Err(DataRootExecutionError::new(format!(
                        "claim rename source was recreated: {}",
                        source.display()
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(DataRootExecutionError::new(format!(
                        "cannot inspect claim rename source before rollback '{}': {error}",
                        source.display()
                    )));
                }
            }
            require_directory_identity(target, "renamed rollback target", identity)?;
            fs::rename(target, source)
                .map_err(|error| execution_error("restore renamed DataRoot", target, error))
        }
    }
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

fn require_directory_identity(
    path: &Path,
    label: &str,
    expected: &EntryIdentity,
) -> Result<(), DataRootExecutionError> {
    require_directory(path, label)?;
    let actual = EntryIdentity::read_directory(path).map_err(|error| {
        DataRootExecutionError::new(format!(
            "cannot identify {label} '{}': {error}",
            path.display()
        ))
    })?;
    if &actual != expected {
        return Err(DataRootExecutionError::new(format!(
            "{label} changed before execution: {}",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_root::plan::PlanTarget;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: std::path::PathBuf,
        entry_file: std::path::PathBuf,
        entry_identity: EntryIdentity,
    }

    impl Fixture {
        fn new() -> Self {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "swawkit-data-root-execute-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("create execution fixture");
            let entry_file = root.join("new-name.exe");
            fs::write(&entry_file, b"entry").expect("create Entry fixture");
            let entry_identity = EntryIdentity::read(&entry_file).expect("Entry identity");
            Self {
                root,
                entry_file,
                entry_identity,
            }
        }

        fn target(&self) -> PlanTarget {
            PlanTarget {
                entry_name: "new-name".to_owned(),
                entry_file: self.entry_file.clone(),
                identity: self.entry_identity.clone(),
                data_root: self.root.join("proj.new-name"),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn failed_initial_publication_removes_only_the_created_directory() {
        let fixture = Fixture::new();
        let plan = DataRootPlan::Create {
            target: fixture.target(),
        };

        let error = execute_plan_with(&plan, fail_publication)
            .err()
            .expect("fixture publication must fail");

        assert!(error.to_string().contains("fixture publication failed"));
        assert!(!plan.target().data_root.exists());
        assert!(fixture.entry_file.exists());
    }

    #[test]
    fn failed_rename_publication_restores_the_original_directory_and_data() {
        let fixture = Fixture::new();
        let source = fixture.root.join("proj.old-name");
        fs::create_dir(&source).expect("create original DataRoot");
        fs::write(source.join("opaque.bin"), b"opaque").expect("write opaque module data");
        fs::write(source.join("_entry.json"), b"old record").expect("write old record");
        let directory_identity = EntryIdentity::read_directory(&source).unwrap();
        let plan = DataRootPlan::ClaimRename {
            target: fixture.target(),
            source_data_root: source.clone(),
            observed_directory_identity: directory_identity,
            observed_record_revision: "fixture".to_owned(),
            reason: "fixture".to_owned(),
        };

        let error = execute_plan_with(&plan, fail_publication)
            .err()
            .expect("fixture publication must fail");

        assert!(error.to_string().contains("fixture publication failed"));
        assert!(source.is_dir());
        assert!(!plan.target().data_root.exists());
        assert_eq!(fs::read(source.join("opaque.bin")).unwrap(), b"opaque");
        assert_eq!(fs::read(source.join("_entry.json")).unwrap(), b"old record");
    }

    fn fail_publication(
        _data_root: &Path,
        _entry_name: &str,
        _entry_file: &Path,
        _identity: &EntryIdentity,
    ) -> Result<(), DataRootExecutionError> {
        Err(DataRootExecutionError::new(
            "fixture publication failed".to_owned(),
        ))
    }
}
