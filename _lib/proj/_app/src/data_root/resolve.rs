use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::entry::{EntryIdentity, EntryIdentityError};

use super::claim::{ClaimApprovalError, ClaimKind, DataRootClaim, DataRootClaimApprover};
use super::execute::{DataRootExecutionError, execute_plan};
use super::inventory::{DataRootInventory, DataRootInventoryError};
use super::lock::{DataRootLock, DataRootLockError};
use super::plan::{
    DataRootPlan, DataRootPlanError, DataRootPlanningRequest, ordinal_path_eq, ordinal_text_eq,
    plan_data_root,
};

#[derive(Clone, Copy)]
pub struct ResolveDataRootRequest<'a> {
    pub swawkit_home: &'a Path,
    pub entry_file: &'a Path,
    pub inherited_data_root: Option<&'a Path>,
    pub legacy_data_directory: Option<&'a Path>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDataRoot {
    pub path: PathBuf,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRootInspection {
    pub data_root: PathBuf,
    pub claim: Option<DataRootClaim>,
}

pub fn inspect_data_root(
    request: ResolveDataRootRequest<'_>,
) -> Result<DataRootInspection, ResolveDataRootError> {
    let request = OwnedRequest::from_request(request)?;
    let data_directory = request.swawkit_home.join("data");
    let plan = build_plan(&request, &data_directory)?;
    Ok(DataRootInspection {
        data_root: plan.target().data_root.clone(),
        claim: DataRootClaim::from_plan(&plan),
    })
}

pub fn claim_data_root(
    request: ResolveDataRootRequest<'_>,
    expected: &DataRootClaim,
) -> Result<ResolvedDataRoot, ResolveDataRootError> {
    let request = OwnedRequest::from_request(request)?;
    let data_directory = request.swawkit_home.join("data");
    let lock = DataRootLock::acquire(&data_directory)?;
    let plan = build_plan(&request, &data_directory)?;
    complete_expected_claim(plan, expected, lock)
}

pub fn resolve_data_root(
    request: ResolveDataRootRequest<'_>,
    approver: &mut impl DataRootClaimApprover,
) -> Result<ResolvedDataRoot, ResolveDataRootError> {
    let request = OwnedRequest::from_request(request)?;
    let data_directory = request.swawkit_home.join("data");

    let lock = DataRootLock::acquire(&data_directory)?;
    let initial_plan = build_plan(&request, &data_directory)?;
    let claim = DataRootClaim::from_plan(&initial_plan);
    let Some(claim) = claim else {
        return complete_locked(initial_plan, None, lock);
    };
    drop(lock);

    if !approver.approve(&claim)? {
        return Err(ResolveDataRootError::approval_denied());
    }

    let lock = DataRootLock::acquire(&data_directory)?;
    let current_plan = build_plan(&request, &data_directory)?;
    complete_expected_claim(current_plan, &claim, lock)
}

fn build_plan(
    request: &OwnedRequest,
    data_directory: &Path,
) -> Result<DataRootPlan, ResolveDataRootError> {
    let identity = EntryIdentity::read(&request.entry_file)?;
    let current = DataRootInventory::scan(data_directory)?;
    let legacy = request
        .legacy_data_directory
        .as_deref()
        .map(DataRootInventory::scan)
        .transpose()?;
    plan_data_root(DataRootPlanningRequest {
        entry_file: &request.entry_file,
        identity: &identity,
        current: &current,
        legacy: legacy.as_ref(),
        inherited_data_root: request.inherited_data_root.as_deref(),
    })
    .map_err(Into::into)
}

fn complete_locked(
    plan: DataRootPlan,
    completed_legacy_source: Option<PathBuf>,
    lock: DataRootLock,
) -> Result<ResolvedDataRoot, ResolveDataRootError> {
    let path = plan.target().data_root.clone();
    let execution = execute_plan(&plan)?;
    let mut warnings = Vec::new();
    if let Some(source) = execution.legacy_source.or(completed_legacy_source)
        && let Some(warning) = remove_legacy_residue(&source)
    {
        warnings.push(warning);
    }
    let resolved = ResolvedDataRoot {
        path,
        warnings,
    };
    drop(lock);
    Ok(resolved)
}

fn complete_expected_claim(
    plan: DataRootPlan,
    expected: &DataRootClaim,
    lock: DataRootLock,
) -> Result<ResolvedDataRoot, ResolveDataRootError> {
    match DataRootClaim::from_plan(&plan) {
        Some(current) if &current == expected => complete_locked(plan, None, lock),
        None if direct_target_matches(&plan, expected) => {
            let completed_legacy_source = if expected.kind == ClaimKind::MigrateLegacy {
                expected.source_data_root.clone()
            } else {
                None
            };
            complete_locked(plan, completed_legacy_source, lock)
        }
        _ => Err(ResolveDataRootError::state_changed()),
    }
}

fn direct_target_matches(plan: &DataRootPlan, expected: &DataRootClaim) -> bool {
    let DataRootPlan::Direct {
        target,
        data_root_identity,
    } = plan
    else {
        return false;
    };
    ordinal_path_eq(&target.entry_file, &expected.entry_file)
        && ordinal_path_eq(&target.data_root, &expected.data_root)
        && ordinal_text_eq(&target.entry_name, &expected.entry_name)
        && target.identity.volume_id() == expected.volume_id
        && target.identity.file_id() == expected.file_id
        && data_root_identity == expected.observed_directory_identity()
}

fn remove_legacy_residue(legacy_data_root: &Path) -> Option<String> {
    let legacy_directory = legacy_data_root.parent()?;
    if !legacy_directory.is_dir() {
        return None;
    }
    let result = (|| -> Result<(), std::io::Error> {
        let lock_path = legacy_directory.join("_proj-entry.lock");
        if lock_path.is_file() && fs::metadata(&lock_path)?.len() == 0 {
            fs::remove_file(lock_path)?;
        }
        if fs::read_dir(legacy_directory)?
            .next()
            .transpose()?
            .is_none()
        {
            fs::remove_dir(legacy_directory)?;
        }
        Ok(())
    })();
    result.err().map(|error| {
        format!(
            "the obsolete project-local data directory could not be fully cleaned: {}. {error}",
            legacy_directory.display()
        )
    })
}

struct OwnedRequest {
    swawkit_home: PathBuf,
    entry_file: PathBuf,
    inherited_data_root: Option<PathBuf>,
    legacy_data_directory: Option<PathBuf>,
}

impl OwnedRequest {
    fn from_request(request: ResolveDataRootRequest<'_>) -> Result<Self, ResolveDataRootError> {
        let swawkit_home = required_directory(request.swawkit_home, "SWAWKIT_HOME")?;
        let entry_file = absolute(request.entry_file, "project entry file")?;
        let inherited_data_root = match request.inherited_data_root {
            Some(path) if !path.is_absolute() => {
                return Err(ResolveDataRootError::invalid_input(
                    "inherited SWAWKIT_PROJ_DATA_ROOT must be absolute".to_owned(),
                ));
            }
            Some(path) => Some(absolute(path, "inherited DataRoot")?),
            None => None,
        };
        let legacy_data_directory = request
            .legacy_data_directory
            .map(|path| absolute(path, "legacy project data directory"))
            .transpose()?;
        Ok(Self {
            swawkit_home,
            entry_file,
            inherited_data_root,
            legacy_data_directory,
        })
    }
}

fn required_directory(path: &Path, label: &str) -> Result<PathBuf, ResolveDataRootError> {
    let path = absolute(path, label)?;
    if !path.is_dir() {
        return Err(ResolveDataRootError::invalid_input(format!(
            "{label} does not exist: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn absolute(path: &Path, label: &str) -> Result<PathBuf, ResolveDataRootError> {
    std::path::absolute(path).map_err(|error| {
        ResolveDataRootError::invalid_input(format!(
            "invalid {label} path '{}': {error}",
            path.display()
        ))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolveDataRootErrorKind {
    ApprovalDenied,
    StateChanged,
    Other,
}

#[derive(Debug)]
pub struct ResolveDataRootError {
    kind: ResolveDataRootErrorKind,
    message: String,
}

impl ResolveDataRootError {
    fn invalid_input(message: String) -> Self {
        Self::other(message)
    }

    fn approval_denied() -> Self {
        Self {
            kind: ResolveDataRootErrorKind::ApprovalDenied,
            message: "project DataRoot claim was not approved".to_owned(),
        }
    }

    fn state_changed() -> Self {
        Self {
            kind: ResolveDataRootErrorKind::StateChanged,
            message: concat!(
                "project DataRoot state changed during claim. ",
                "Review it and retry the entry."
            )
            .to_owned(),
        }
    }

    fn other(message: String) -> Self {
        Self {
            kind: ResolveDataRootErrorKind::Other,
            message,
        }
    }

    pub fn is_approval_denied(&self) -> bool {
        self.kind == ResolveDataRootErrorKind::ApprovalDenied
    }

    pub fn is_state_changed(&self) -> bool {
        self.kind == ResolveDataRootErrorKind::StateChanged
    }
}

impl fmt::Display for ResolveDataRootError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ResolveDataRootError {}

macro_rules! resolve_error_from {
    ($error:ty) => {
        impl From<$error> for ResolveDataRootError {
            fn from(error: $error) -> Self {
                Self::other(error.to_string())
            }
        }
    };
}

resolve_error_from!(EntryIdentityError);
resolve_error_from!(DataRootInventoryError);
resolve_error_from!(DataRootPlanError);
resolve_error_from!(DataRootLockError);
resolve_error_from!(ClaimApprovalError);
resolve_error_from!(DataRootExecutionError);

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "resolve/directory_identity_tests.rs"]
mod directory_identity_tests;
