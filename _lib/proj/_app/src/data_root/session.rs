use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::entry::EntryIdentity;

use super::resolve::{
    OwnedRequest, claim_owned_data_root, inspect_owned_data_root, resolve_owned_data_root,
};
use super::{
    ClaimApprovalError, DataRootClaim, ResolveDataRootError, ResolveDataRootRequest,
    ResolvedDataRoot,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataRootSessionState {
    Ready(ResolvedDataRoot),
    ClaimRequired(DataRootClaim),
}

#[derive(Clone)]
pub struct DataRootSession {
    request: Arc<OwnedRequest>,
    ready: Arc<Mutex<Option<ResolvedDataRoot>>>,
}

impl DataRootSession {
    pub fn new(request: ResolveDataRootRequest<'_>) -> Result<Self, DataRootSessionError> {
        Ok(Self {
            request: Arc::new(OwnedRequest::from_request(request)?),
            ready: Arc::new(Mutex::new(None)),
        })
    }

    pub fn entry_identity(&self) -> &EntryIdentity {
        self.request.entry_identity()
    }

    pub fn status(&self) -> Result<DataRootSessionState, DataRootSessionError> {
        let mut ready = self.lock_ready()?;
        if let Some(resolved) = ready.as_ref() {
            return Ok(DataRootSessionState::Ready(resolved.clone()));
        }

        let inspection = inspect_owned_data_root(&self.request)?;
        if let Some(claim) = inspection.claim {
            return Ok(DataRootSessionState::ClaimRequired(claim));
        }

        let resolved = self.resolve_without_claim()?;
        *ready = Some(resolved.clone());
        Ok(DataRootSessionState::Ready(resolved))
    }

    pub fn claim(
        &self,
        expected_revision: &str,
        confirmation: &str,
    ) -> Result<Vec<String>, DataRootSessionError> {
        let mut ready = self.lock_ready()?;
        if let Some(resolved) = ready.as_ref() {
            return Ok(resolved.warnings().to_vec());
        }

        let inspection = inspect_owned_data_root(&self.request)?;
        let Some(expected_claim) = inspection.claim else {
            let resolved = self.resolve_without_claim()?;
            let warnings = resolved.warnings().to_vec();
            *ready = Some(resolved);
            return Ok(warnings);
        };
        if expected_claim.revision() != expected_revision {
            return Err(DataRootSessionError::Conflict);
        }
        if confirmation != expected_claim.entry_name {
            return Err(DataRootSessionError::ConfirmationMismatch {
                expected: expected_claim.entry_name,
            });
        }

        let resolved = match claim_owned_data_root(&self.request, &expected_claim) {
            Ok(resolved) => resolved,
            Err(error) if error.is_state_changed() => {
                return Err(DataRootSessionError::Conflict);
            }
            Err(error) => return Err(error.into()),
        };
        let warnings = resolved.warnings().to_vec();
        *ready = Some(resolved);
        Ok(warnings)
    }

    fn resolve_without_claim(&self) -> Result<ResolvedDataRoot, DataRootSessionError> {
        let mut saw_claim = false;
        let result = {
            let mut reject = |_claim: &DataRootClaim| {
                saw_claim = true;
                Err(ClaimApprovalError::new("DataRoot claim is required"))
            };
            resolve_owned_data_root(&self.request, &mut reject)
        };
        if saw_claim {
            return Err(DataRootSessionError::Conflict);
        }
        result.map_err(Into::into)
    }

    fn lock_ready(&self) -> Result<MutexGuard<'_, Option<ResolvedDataRoot>>, DataRootSessionError> {
        self.ready
            .lock()
            .map_err(|_| DataRootSessionError::Unavailable)
    }
}

#[derive(Debug)]
pub enum DataRootSessionError {
    ConfirmationMismatch { expected: String },
    Conflict,
    Resolution(ResolveDataRootError),
    Unavailable,
}

impl fmt::Display for DataRootSessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfirmationMismatch { expected } => write!(
                formatter,
                "DataRoot confirmation must exactly match '{expected}'"
            ),
            Self::Conflict => formatter.write_str(
                "DataRoot claim state changed; reload the current claim before confirming",
            ),
            Self::Resolution(error) => error.fmt(formatter),
            Self::Unavailable => formatter.write_str("DataRoot session is unavailable"),
        }
    }
}

impl Error for DataRootSessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Resolution(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ResolveDataRootError> for DataRootSessionError {
    fn from(error: ResolveDataRootError) -> Self {
        Self::Resolution(error)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::data_root::{DataRootClaim, resolve_data_root};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn claim_required_session_pins_its_entry_from_construction_until_drop() {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-data-root-session-{}-{sequence}",
            std::process::id()
        ));
        let swawkit_home = root.join("home");
        let entry_file = root.join("session-entry.exe");
        fs::create_dir_all(&swawkit_home).expect("create Swaw Kit home");
        fs::write(&entry_file, b"first Entry").expect("create first Entry");

        let request = ResolveDataRootRequest {
            swawkit_home: &swawkit_home,
            entry_file: &entry_file,
            inherited_data_root: None,
            legacy_data_directory: None,
        };
        let mut approve = |_claim: &DataRootClaim| Ok(true);
        let resolved = resolve_data_root(request, &mut approve).expect("bind first Entry");
        drop(resolved);

        let replacement = root.join("replacement.exe");
        fs::write(&replacement, b"second Entry").expect("create replacement Entry");
        fs::remove_file(&entry_file).expect("remove first Entry");
        fs::rename(&replacement, &entry_file).expect("publish replacement Entry");
        let expected_identity =
            EntryIdentity::read(&entry_file).expect("read replacement identity");

        let session = DataRootSession::new(request).expect("pin replacement Entry");
        assert_eq!(session.entry_identity(), &expected_identity);
        assert!(
            fs::write(&entry_file, b"changed before status").is_err(),
            "the Entry must be pinned before the first session request"
        );
        assert!(matches!(
            session.status().expect("inspect DataRoot session"),
            DataRootSessionState::ClaimRequired(_)
        ));
        assert!(
            fs::rename(&entry_file, root.join("moved.exe")).is_err(),
            "ClaimRequired must retain the Entry lease"
        );

        drop(session);
        fs::write(&entry_file, b"changed after session").expect("modify released Entry");
        fs::remove_dir_all(root).expect("remove fixture");
    }
}
