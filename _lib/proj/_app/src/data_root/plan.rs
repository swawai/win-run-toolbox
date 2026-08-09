use std::error::Error;
use std::fmt;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};

use crate::entry::EntryIdentity;

use super::inventory::{DataRootInventory, DataRootSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTarget {
    pub entry_name: String,
    pub entry_file: PathBuf,
    pub identity: EntryIdentity,
    pub data_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataRootPlan {
    Direct {
        target: PlanTarget,
        data_root_identity: EntryIdentity,
    },
    Create {
        target: PlanTarget,
    },
    ClaimCurrent {
        target: PlanTarget,
        observed_directory_identity: EntryIdentity,
        observed_record_revision: String,
        reason: String,
    },
    ClaimRename {
        target: PlanTarget,
        source_data_root: PathBuf,
        observed_directory_identity: EntryIdentity,
        observed_record_revision: String,
        reason: String,
    },
    MigrateLegacy {
        target: PlanTarget,
        source_data_root: PathBuf,
        observed_directory_identity: EntryIdentity,
        reason: String,
    },
    ClaimMigrateLegacy {
        target: PlanTarget,
        source_data_root: PathBuf,
        observed_directory_identity: EntryIdentity,
        observed_record_revision: String,
        reason: String,
    },
}

impl DataRootPlan {
    pub fn target(&self) -> &PlanTarget {
        match self {
            Self::Direct { target, .. }
            | Self::Create { target }
            | Self::ClaimCurrent { target, .. }
            | Self::ClaimRename { target, .. }
            | Self::MigrateLegacy { target, .. }
            | Self::ClaimMigrateLegacy { target, .. } => target,
        }
    }
}

pub struct DataRootPlanningRequest<'a> {
    pub entry_file: &'a Path,
    pub identity: &'a EntryIdentity,
    pub current: &'a DataRootInventory,
    pub legacy: Option<&'a DataRootInventory>,
    pub inherited_data_root: Option<&'a Path>,
}

pub fn plan_data_root(
    request: DataRootPlanningRequest<'_>,
) -> Result<DataRootPlan, DataRootPlanError> {
    if !request.entry_file.is_absolute() {
        return Err(DataRootPlanError::EntryFileNotAbsolute(
            request.entry_file.to_path_buf(),
        ));
    }
    let entry_name = request
        .entry_file
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| !name.trim().is_empty())
        .ok_or(DataRootPlanError::InvalidEntryName)?
        .to_owned();
    let candidate = request
        .current
        .directory()
        .join(format!("proj.{entry_name}"));
    let current_matches = identity_matches(request.current, request.identity);
    if current_matches.len() > 1 {
        return Err(DataRootPlanError::MultipleCurrentBindings(paths(
            &current_matches,
        )));
    }

    let legacy_inventory = request
        .legacy
        .filter(|inventory| !ordinal_path_eq(inventory.directory(), request.current.directory()));
    let legacy_matches = legacy_inventory
        .map(|inventory| identity_matches(inventory, request.identity))
        .unwrap_or_default();
    if legacy_matches.len() > 1 {
        return Err(DataRootPlanError::MultipleLegacyBindings(paths(
            &legacy_matches,
        )));
    }
    if let (Some(current), Some(legacy)) = (current_matches.first(), legacy_matches.first()) {
        return Err(DataRootPlanError::CurrentAndLegacyBindings {
            current: current.path.clone(),
            legacy: legacy.path.clone(),
        });
    }

    let inherited = normalize_inherited(request.inherited_data_root)?;
    if let Some(inherited) = inherited.as_deref()
        && !ordinal_path_eq(inherited, &candidate)
        && !contains_path(&current_matches, inherited)
        && !contains_path(&legacy_matches, inherited)
    {
        return Err(DataRootPlanError::ForeignInherited(inherited.to_path_buf()));
    }

    let target = PlanTarget {
        entry_name: entry_name.clone(),
        entry_file: request.entry_file.to_path_buf(),
        identity: request.identity.clone(),
        data_root: candidate.clone(),
    };
    let candidate_root = request
        .current
        .roots()
        .iter()
        .find(|root| ordinal_path_eq(&root.path, &candidate));

    if let Some(candidate_root) = candidate_root {
        let record = candidate_root.record.valid_record();
        let identity_matches = record
            .map(|record| record.matches_identity(request.identity))
            .unwrap_or(false);
        let name_matches = record
            .map(|record| ordinal_text_eq(&record.entry_name, &entry_name))
            .unwrap_or(false);
        if identity_matches && name_matches {
            return Ok(DataRootPlan::Direct {
                target,
                data_root_identity: candidate_root.directory_identity().clone(),
            });
        }
        if let Some(current) = current_matches.first()
            && !ordinal_path_eq(&current.path, &candidate)
        {
            return Err(DataRootPlanError::CandidateCollision {
                candidate,
                current: current.path.clone(),
            });
        }
        let reason = match record {
            None => candidate_root
                .record
                .invalid_reason()
                .unwrap_or("candidate identity record is unavailable")
                .to_owned(),
            Some(_) if !name_matches => "entry name does not match the identity record".to_owned(),
            Some(_) => "File ID does not match the identity record".to_owned(),
        };
        return Ok(DataRootPlan::ClaimCurrent {
            target,
            observed_directory_identity: candidate_root.directory_identity().clone(),
            observed_record_revision: candidate_root.record_revision(),
            reason,
        });
    }

    if let Some(current) = current_matches.first() {
        return Ok(DataRootPlan::ClaimRename {
            target,
            source_data_root: current.path.clone(),
            observed_directory_identity: current.directory_identity().clone(),
            observed_record_revision: current.record_revision(),
            reason: "the entry File ID is bound under another entry name".to_owned(),
        });
    }

    if let Some(legacy) = legacy_matches.first() {
        let record = legacy
            .record
            .valid_record()
            .expect("identity matches only contain valid records");
        let legacy_name_matches = ordinal_text_eq(&record.entry_name, &entry_name)
            && legacy
                .path
                .file_name()
                .is_some_and(|name| ordinal_os_eq(name, format!("proj.{entry_name}")));
        if legacy_name_matches {
            return Ok(DataRootPlan::MigrateLegacy {
                target,
                source_data_root: legacy.path.clone(),
                observed_directory_identity: legacy.directory_identity().clone(),
                reason: "DataRoot is stored in the legacy project-local data directory".to_owned(),
            });
        }
        return Ok(DataRootPlan::ClaimMigrateLegacy {
            target,
            source_data_root: legacy.path.clone(),
            observed_directory_identity: legacy.directory_identity().clone(),
            observed_record_revision: legacy.record_revision(),
            reason: "the entry File ID is stored under a renamed legacy DataRoot".to_owned(),
        });
    }

    if let Some(inherited) = inherited {
        return Err(DataRootPlanError::UnboundInherited(inherited));
    }
    Ok(DataRootPlan::Create { target })
}

fn identity_matches<'a>(
    inventory: &'a DataRootInventory,
    identity: &EntryIdentity,
) -> Vec<&'a DataRootSnapshot> {
    inventory
        .roots()
        .iter()
        .filter(|root| {
            root.record
                .valid_record()
                .is_some_and(|record| record.matches_identity(identity))
        })
        .collect()
}

fn paths(roots: &[&DataRootSnapshot]) -> Vec<PathBuf> {
    roots.iter().map(|root| root.path.clone()).collect()
}

fn contains_path(roots: &[&DataRootSnapshot], path: &Path) -> bool {
    roots.iter().any(|root| ordinal_path_eq(&root.path, path))
}

fn normalize_inherited(path: Option<&Path>) -> Result<Option<PathBuf>, DataRootPlanError> {
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.is_absolute() {
        return Err(DataRootPlanError::InheritedNotAbsolute(path.to_path_buf()));
    }
    std::path::absolute(path)
        .map(Some)
        .map_err(|_| DataRootPlanError::InheritedNotAbsolute(path.to_path_buf()))
}

pub(crate) fn ordinal_path_eq(left: &Path, right: &Path) -> bool {
    ordinal_os_eq(left.as_os_str(), right.as_os_str())
}

pub(crate) fn ordinal_text_eq(left: &str, right: &str) -> bool {
    ordinal_wide_eq(
        &left.encode_utf16().collect::<Vec<_>>(),
        &right.encode_utf16().collect::<Vec<_>>(),
    )
}

fn ordinal_os_eq(left: &std::ffi::OsStr, right: impl AsRef<std::ffi::OsStr>) -> bool {
    ordinal_wide_eq(
        &left.encode_wide().collect::<Vec<_>>(),
        &right.as_ref().encode_wide().collect::<Vec<_>>(),
    )
}

fn ordinal_wide_eq(left: &[u16], right: &[u16]) -> bool {
    if left.len() > i32::MAX as usize || right.len() > i32::MAX as usize {
        return false;
    }
    unsafe {
        CompareStringOrdinal(
            left.as_ptr(),
            left.len() as i32,
            right.as_ptr(),
            right.len() as i32,
            1,
        ) == CSTR_EQUAL
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataRootPlanError {
    InvalidEntryName,
    EntryFileNotAbsolute(PathBuf),
    InheritedNotAbsolute(PathBuf),
    ForeignInherited(PathBuf),
    UnboundInherited(PathBuf),
    MultipleCurrentBindings(Vec<PathBuf>),
    MultipleLegacyBindings(Vec<PathBuf>),
    CurrentAndLegacyBindings {
        current: PathBuf,
        legacy: PathBuf,
    },
    CandidateCollision {
        candidate: PathBuf,
        current: PathBuf,
    },
}

impl fmt::Display for DataRootPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntryName => formatter.write_str("project entry has no usable file name"),
            Self::EntryFileNotAbsolute(path) => write!(
                formatter,
                "project entry file must be absolute: {}",
                path.display()
            ),
            Self::InheritedNotAbsolute(path) => write!(
                formatter,
                "inherited SWAWKIT_PROJ_DATA_ROOT must be absolute: {}",
                path.display()
            ),
            Self::ForeignInherited(path) => write!(
                formatter,
                concat!(
                    "another project's DataRoot is already active: {}. ",
                    "Exit that project shell before invoking this entry."
                ),
                path.display()
            ),
            Self::UnboundInherited(path) => write!(
                formatter,
                concat!(
                    "inherited DataRoot '{}' has no valid binding for the current entry. ",
                    "Manual repair is required."
                ),
                path.display()
            ),
            Self::MultipleCurrentBindings(paths) => write!(
                formatter,
                concat!(
                    "multiple project DataRoots contain the current entry File ID: {}. ",
                    "Manual repair is required."
                ),
                display_paths(paths)
            ),
            Self::MultipleLegacyBindings(paths) => write!(
                formatter,
                concat!(
                    "multiple legacy project DataRoots contain the current entry File ID: {}. ",
                    "Manual repair is required."
                ),
                display_paths(paths)
            ),
            Self::CurrentAndLegacyBindings { current, legacy } => write!(
                formatter,
                concat!(
                    "both the Swaw Kit Home and legacy project directory contain DataRoots ",
                    "for the current entry File ID: '{}', '{}'. Manual repair is required."
                ),
                current.display(),
                legacy.display()
            ),
            Self::CandidateCollision { candidate, current } => write!(
                formatter,
                concat!(
                    "the desired DataRoot '{}' belongs to another File ID while '{}' ",
                    "contains the current File ID. Manual repair is required."
                ),
                candidate.display(),
                current.display()
            ),
        }
    }
}

impl Error for DataRootPlanError {}

fn display_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests;
