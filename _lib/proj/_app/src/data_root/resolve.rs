use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::entry::{EntryIdentity, EntryIdentityError, EntryIdentityLease};

use super::claim::{ClaimApprovalError, DataRootClaim, DataRootClaimApprover};
use super::execute::{DataRootExecutionError, execute_plan};
use super::inventory::{DataRootInventory, DataRootInventoryError};
use super::lease::{DataRootBindingLease, DataRootBindingLeaseError};
use super::lock::{DataRootLock, DataRootLockError};
use super::plan::{
    DataRootPlan, DataRootPlanError, DataRootPlanningRequest, ordinal_path_eq, ordinal_text_eq,
    plan_data_root,
};

#[derive(Clone, Copy)]
pub struct ResolveDataRootRequest<'a> {
    pub swawkit_home: &'a Path,
    pub entry_file: &'a Path,
}

#[derive(Clone)]
pub struct ResolvedDataRoot {
    path: PathBuf,
    _lease: Arc<DataRootBindingLease>,
}

impl ResolvedDataRoot {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl fmt::Debug for ResolvedDataRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResolvedDataRoot")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl PartialEq for ResolvedDataRoot {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for ResolvedDataRoot {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRootInspection {
    pub data_root: PathBuf,
    pub claim: Option<DataRootClaim>,
}

pub fn inspect_data_root(
    request: ResolveDataRootRequest<'_>,
) -> Result<DataRootInspection, ResolveDataRootError> {
    let request = OwnedRequest::from_request(request)?;
    inspect_owned_data_root(&request)
}

pub(super) fn inspect_owned_data_root(
    request: &OwnedRequest,
) -> Result<DataRootInspection, ResolveDataRootError> {
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
    claim_owned_data_root(&request, expected)
}

pub(super) fn claim_owned_data_root(
    request: &OwnedRequest,
    expected: &DataRootClaim,
) -> Result<ResolvedDataRoot, ResolveDataRootError> {
    let data_directory = request.swawkit_home.join("data");
    let lock = DataRootLock::acquire(&data_directory)?;
    let plan = build_plan(&request, &data_directory)?;
    complete_expected_claim(plan, expected, lock, request)
}

pub fn resolve_data_root(
    request: ResolveDataRootRequest<'_>,
    approver: &mut impl DataRootClaimApprover,
) -> Result<ResolvedDataRoot, ResolveDataRootError> {
    let request = OwnedRequest::from_request(request)?;
    resolve_owned_data_root(&request, approver)
}

pub(super) fn resolve_owned_data_root(
    request: &OwnedRequest,
    approver: &mut impl DataRootClaimApprover,
) -> Result<ResolvedDataRoot, ResolveDataRootError> {
    let data_directory = request.swawkit_home.join("data");

    let lock = DataRootLock::acquire(&data_directory)?;
    let initial_plan = build_plan(&request, &data_directory)?;
    let claim = DataRootClaim::from_plan(&initial_plan);
    let Some(claim) = claim else {
        return complete_locked(initial_plan, lock, request);
    };
    drop(lock);

    if !approver.approve(&claim)? {
        return Err(ResolveDataRootError::approval_denied());
    }

    let lock = DataRootLock::acquire(&data_directory)?;
    let current_plan = build_plan(&request, &data_directory)?;
    complete_expected_claim(current_plan, &claim, lock, request)
}

fn build_plan(
    request: &OwnedRequest,
    data_directory: &Path,
) -> Result<DataRootPlan, ResolveDataRootError> {
    let current = DataRootInventory::scan(data_directory)?;
    plan_data_root(DataRootPlanningRequest {
        entry_file: &request.entry_file,
        identity: request.entry_identity(),
        current: &current,
    })
    .map_err(Into::into)
}

fn complete_locked(
    plan: DataRootPlan,
    lock: DataRootLock,
    request: &OwnedRequest,
) -> Result<ResolvedDataRoot, ResolveDataRootError> {
    let target = plan.target().clone();
    execute_plan(&plan)?;
    let lease = Arc::new(DataRootBindingLease::acquire(
        &plan,
        Arc::clone(&request.entry_file_lease),
    )?);
    let resolved = ResolvedDataRoot {
        path: target.data_root,
        _lease: lease,
    };
    drop(lock);
    Ok(resolved)
}

fn complete_expected_claim(
    plan: DataRootPlan,
    expected: &DataRootClaim,
    lock: DataRootLock,
    request: &OwnedRequest,
) -> Result<ResolvedDataRoot, ResolveDataRootError> {
    match DataRootClaim::from_plan(&plan) {
        Some(current) if &current == expected => complete_locked(plan, lock, request),
        None if direct_target_matches(&plan, expected) => complete_locked(plan, lock, request),
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

pub(super) struct OwnedRequest {
    swawkit_home: PathBuf,
    entry_file: PathBuf,
    entry_file_lease: Arc<EntryIdentityLease>,
}

impl OwnedRequest {
    pub(super) fn from_request(
        request: ResolveDataRootRequest<'_>,
    ) -> Result<Self, ResolveDataRootError> {
        let swawkit_home = required_directory(request.swawkit_home, "SWAWKIT_HOME")?;
        let entry_file = absolute(request.entry_file, "project entry file")?;
        let entry_file_lease = Arc::new(EntryIdentityLease::acquire_entry(&entry_file)?);
        Ok(Self {
            swawkit_home,
            entry_file,
            entry_file_lease,
        })
    }

    pub(super) fn entry_identity(&self) -> &EntryIdentity {
        self.entry_file_lease.identity()
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
resolve_error_from!(DataRootBindingLeaseError);

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "resolve/directory_identity_tests.rs"]
mod directory_identity_tests;

#[cfg(test)]
#[path = "resolve/lease_tests.rs"]
mod lease_tests;
