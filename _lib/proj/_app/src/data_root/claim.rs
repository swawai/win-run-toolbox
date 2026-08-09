use std::error::Error;
use std::fmt;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use super::plan::DataRootPlan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimKind {
    Current,
    Rename,
}

impl ClaimKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Rename => "rename",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataRootClaim {
    pub kind: ClaimKind,
    pub entry_name: String,
    pub entry_file: PathBuf,
    pub volume_id: String,
    pub file_id: String,
    pub data_root: PathBuf,
    pub source_data_root: Option<PathBuf>,
    pub reason: String,
    observed_directory_identity: crate::entry::EntryIdentity,
    observed_record_revision: String,
}

impl DataRootClaim {
    pub(crate) fn from_plan(plan: &DataRootPlan) -> Option<Self> {
        let (
            kind,
            source_data_root,
            observed_directory_identity,
            observed_record_revision,
            reason,
        ) = match plan {
            DataRootPlan::ClaimCurrent {
                observed_directory_identity,
                observed_record_revision,
                reason,
                ..
            } => (
                ClaimKind::Current,
                None,
                observed_directory_identity.clone(),
                observed_record_revision.clone(),
                reason.clone(),
            ),
            DataRootPlan::ClaimRename {
                source_data_root,
                observed_directory_identity,
                observed_record_revision,
                reason,
                ..
            } => (
                ClaimKind::Rename,
                Some(source_data_root.clone()),
                observed_directory_identity.clone(),
                observed_record_revision.clone(),
                reason.clone(),
            ),
            DataRootPlan::Direct { .. } | DataRootPlan::Create { .. } => return None,
        };
        let target = plan.target();
        Some(Self {
            kind,
            entry_name: target.entry_name.clone(),
            entry_file: target.entry_file.clone(),
            volume_id: target.identity.volume_id().to_owned(),
            file_id: target.identity.file_id().to_owned(),
            data_root: target.data_root.clone(),
            source_data_root,
            reason,
            observed_directory_identity,
            observed_record_revision,
        })
    }

    pub fn revision(&self) -> String {
        let mut digest = Sha256::new();
        hash_text(&mut digest, self.kind.as_str());
        hash_text(&mut digest, &self.entry_name);
        hash_path(&mut digest, &self.entry_file);
        hash_text(&mut digest, &self.volume_id);
        hash_text(&mut digest, &self.file_id);
        hash_path(&mut digest, &self.data_root);
        match &self.source_data_root {
            Some(path) => {
                digest.update([1]);
                hash_path(&mut digest, path);
            }
            None => digest.update([0]),
        }
        hash_text(&mut digest, &self.reason);
        hash_text(
            &mut digest,
            self.observed_directory_identity.volume_id(),
        );
        hash_text(&mut digest, self.observed_directory_identity.file_id());
        hash_text(&mut digest, &self.observed_record_revision);
        format!("sha256-{:x}", digest.finalize())
    }

    pub(crate) fn observed_directory_identity(&self) -> &crate::entry::EntryIdentity {
        &self.observed_directory_identity
    }
}

fn hash_text(digest: &mut Sha256, value: &str) {
    hash_bytes(digest, value.as_bytes());
}

fn hash_path(digest: &mut Sha256, value: &std::path::Path) {
    let wide = value
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    hash_bytes(digest, &wide);
}

fn hash_bytes(digest: &mut Sha256, value: &[u8]) {
    digest.update(value.len().to_le_bytes());
    digest.update(value);
}

pub trait DataRootClaimApprover {
    fn approve(&mut self, claim: &DataRootClaim) -> Result<bool, ClaimApprovalError>;
}

impl<F> DataRootClaimApprover for F
where
    F: FnMut(&DataRootClaim) -> Result<bool, ClaimApprovalError>,
{
    fn approve(&mut self, claim: &DataRootClaim) -> Result<bool, ClaimApprovalError> {
        self(claim)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimApprovalError {
    message: String,
}

impl ClaimApprovalError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ClaimApprovalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ClaimApprovalError {}
