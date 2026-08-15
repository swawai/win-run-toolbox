mod document;
mod error;
mod language;
mod model;
mod provider_state;
mod storage;
mod variables;

use std::fs;
use std::path::PathBuf;

pub use document::{EntryProfileDocument, PROFILE_DOCUMENT_PROTOCOL};
pub use error::{ProfileError, ProfileUpdateError};
pub use language::{DEFAULT_LANGUAGE, EntryLanguage};
pub use model::{
    ChannelTool, DevelopmentProfile, EntryProfileRecord, GitProfile, ModeTool, RustTool,
    VersionedTool,
};
use storage::{read_record, revision, validate_data_root, validate_publication_target};

use crate::atomic_file;
use crate::binding::ProjectBinding;
use crate::data_root::DataRootLock;

pub const PROFILE_SCHEMA: &str = "swawkit.entry-profile/v2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryProfile {
    record: EntryProfileRecord,
    binding: ProjectBinding,
    environment_input_revision: String,
    profile_revision: String,
}

impl EntryProfile {
    pub fn record(&self) -> &EntryProfileRecord {
        &self.record
    }

    pub fn binding(&self) -> &ProjectBinding {
        &self.binding
    }

    pub fn environment_input_revision(&self) -> &str {
        &self.environment_input_revision
    }

    pub fn profile_revision(&self) -> &str {
        &self.profile_revision
    }

    pub fn language(&self) -> EntryLanguage {
        EntryLanguage::parse(&self.record.language)
            .expect("a resolved Entry Profile must have a supported language")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryProfileState {
    Missing {
        path: PathBuf,
    },
    Invalid {
        path: PathBuf,
        record: Option<EntryProfileRecord>,
        error: String,
    },
    Ready(EntryProfile),
}

impl EntryProfileState {
    pub fn ready(&self) -> Option<&EntryProfile> {
        match self {
            Self::Ready(profile) => Some(profile),
            Self::Missing { .. } | Self::Invalid { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EntryProfileStore {
    swawkit_home: PathBuf,
    data_root: PathBuf,
}

struct ProfileSnapshot {
    state: EntryProfileState,
    revision: String,
}

struct PreparedProfile {
    profile: EntryProfile,
    content: Vec<u8>,
    revision: String,
}

impl EntryProfileStore {
    pub fn new(swawkit_home: impl Into<PathBuf>, data_root: impl Into<PathBuf>) -> Self {
        Self {
            swawkit_home: swawkit_home.into(),
            data_root: data_root.into(),
        }
    }

    pub fn read(&self) -> EntryProfileState {
        self.snapshot().state
    }

    fn snapshot(&self) -> ProfileSnapshot {
        let path = self.path();
        match fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return ProfileSnapshot {
                    state: EntryProfileState::Missing { path },
                    revision: "missing".to_owned(),
                };
            }
            Err(error) => {
                return ProfileSnapshot {
                    state: EntryProfileState::Invalid {
                        path,
                        record: None,
                        error: format!("cannot inspect entry profile: {error}"),
                    },
                    revision: "unavailable".to_owned(),
                };
            }
        }

        let (record, revision) = match read_record(&path) {
            Ok(result) => result,
            Err(error) => {
                return ProfileSnapshot {
                    state: EntryProfileState::Invalid {
                        path,
                        record: None,
                        error: error.to_string(),
                    },
                    revision: error.revision.unwrap_or_else(|| "unavailable".to_owned()),
                };
            }
        };
        let state = match self.resolve(record.clone(), revision.clone()) {
            Ok(profile) => EntryProfileState::Ready(profile),
            Err(error) => EntryProfileState::Invalid {
                path,
                record: Some(record),
                error: error.to_string(),
            },
        };
        ProfileSnapshot { state, revision }
    }

    pub fn save(&self, record: EntryProfileRecord) -> Result<EntryProfile, ProfileError> {
        let lock = self.acquire_lock()?;
        let current = self.snapshot();
        self.commit_locked(&lock, &current, record)
            .map(|prepared| prepared.profile)
    }

    fn commit_locked(
        &self,
        lock: &DataRootLock,
        current: &ProfileSnapshot,
        record: EntryProfileRecord,
    ) -> Result<PreparedProfile, ProfileError> {
        validate_data_root(&self.data_root)?;
        let mut content = serde_json::to_vec_pretty(&record).map_err(|error| {
            ProfileError::new(format!("cannot serialize entry profile: {error}"))
        })?;
        content.push(b'\n');
        let profile_revision = revision(&content);
        let profile = self.resolve(record, profile_revision.clone())?;
        let prepared = PreparedProfile {
            revision: profile_revision,
            profile,
            content,
        };
        let path = self.path();
        validate_publication_target(&path)?;
        let state_transaction = if snapshot_input_revision(current).as_deref()
            != Some(prepared.profile.environment_input_revision.as_str())
        {
            Some(provider_state::begin_unavailable(
                &self.data_root,
                &prepared.profile.environment_input_revision,
                lock,
            )?)
        } else {
            None
        };
        let publication = atomic_file::publish(&path, &prepared.content).map_err(|error| {
            ProfileError::new(format!(
                "cannot publish entry profile '{}': {error}",
                path.display()
            ))
        });
        if let Err(publication_error) = publication {
            if let Some(transaction) = state_transaction {
                transaction.rollback().map_err(|rollback_error| {
                    ProfileError::new(format!(
                        "{publication_error}; additionally, {rollback_error}"
                    ))
                })?;
            }
            return Err(publication_error);
        }
        if let Some(transaction) = state_transaction {
            transaction.commit();
        }
        Ok(prepared)
    }

    pub fn document(&self) -> EntryProfileDocument {
        let snapshot = self.snapshot();
        EntryProfileDocument::from_state(
            snapshot.state,
            self.path().display().to_string(),
            snapshot.revision,
        )
    }

    pub fn update_setting(
        &self,
        address: &str,
        value: String,
    ) -> Result<EntryProfileDocument, ProfileError> {
        let lock = self.acquire_lock()?;
        let current = self.snapshot();
        let mut record = record_for_setting_update(current.state.clone())?;
        record.set_profile_setting(address, value)?;
        let prepared = self.commit_locked(&lock, &current, record)?;
        Ok(EntryProfileDocument::from_state(
            EntryProfileState::Ready(prepared.profile),
            self.path().display().to_string(),
            prepared.revision,
        ))
    }

    pub fn replace(
        &self,
        record: EntryProfileRecord,
    ) -> Result<EntryProfileDocument, ProfileError> {
        let lock = self.acquire_lock()?;
        let current = self.snapshot();
        let prepared = self.commit_locked(&lock, &current, record)?;
        Ok(EntryProfileDocument::from_state(
            EntryProfileState::Ready(prepared.profile),
            self.path().display().to_string(),
            prepared.revision,
        ))
    }

    pub fn update_setting_if_revision(
        &self,
        expected_revision: &str,
        address: &str,
        value: String,
    ) -> Result<EntryProfileDocument, ProfileUpdateError> {
        let lock = self.acquire_lock().map_err(ProfileUpdateError::Profile)?;
        let current = self.snapshot();
        if current.revision != expected_revision {
            return Err(ProfileUpdateError::Conflict {
                current_revision: current.revision,
            });
        }
        let mut record = record_for_setting_update(current.state.clone())
            .map_err(ProfileUpdateError::Profile)?;
        record
            .set_profile_setting(address, value)
            .map_err(ProfileUpdateError::Profile)?;
        let prepared = self
            .commit_locked(&lock, &current, record)
            .map_err(ProfileUpdateError::Profile)?;
        Ok(EntryProfileDocument::from_state(
            EntryProfileState::Ready(prepared.profile),
            self.path().display().to_string(),
            prepared.revision,
        ))
    }

    pub fn path(&self) -> PathBuf {
        self.data_root.join("_profile.json")
    }

    fn resolve(
        &self,
        record: EntryProfileRecord,
        profile_revision: String,
    ) -> Result<EntryProfile, ProfileError> {
        record.validate()?;
        let binding = ProjectBinding::resolve(&self.swawkit_home, &record.target_project_root)
            .map_err(|error| ProfileError::new(error.to_string()))?;
        let environment_input_revision = environment_input_revision(&record);
        Ok(EntryProfile {
            record,
            binding,
            environment_input_revision,
            profile_revision,
        })
    }

    fn acquire_lock(&self) -> Result<DataRootLock, ProfileError> {
        let data_directory = self.data_root.parent().ok_or_else(|| {
            ProfileError::new(format!(
                "entry profile DataRoot has no data directory: {}",
                self.data_root.display()
            ))
        })?;
        DataRootLock::acquire(data_directory).map_err(|error| ProfileError::new(error.to_string()))
    }
}

fn snapshot_input_revision(snapshot: &ProfileSnapshot) -> Option<String> {
    match &snapshot.state {
        EntryProfileState::Ready(profile) => Some(profile.environment_input_revision().to_owned()),
        EntryProfileState::Invalid {
            record: Some(record),
            ..
        } => Some(record.environment_input_revision()),
        EntryProfileState::Missing { .. } | EntryProfileState::Invalid { record: None, .. } => None,
    }
}

fn environment_input_revision(record: &EntryProfileRecord) -> String {
    let content = serde_json::to_vec(&record.dev_setup_input_values())
        .expect("Entry Profile environment inputs must serialize");
    revision(&content)
}

impl EntryProfileRecord {
    pub(crate) fn environment_input_revision(&self) -> String {
        environment_input_revision(self)
    }
}

fn record_for_setting_update(state: EntryProfileState) -> Result<EntryProfileRecord, ProfileError> {
    match state {
        EntryProfileState::Missing { .. } => Ok(EntryProfileRecord::default()),
        EntryProfileState::Invalid {
            record: Some(record),
            ..
        } => Ok(record),
        EntryProfileState::Invalid {
            record: None,
            error,
            ..
        } => Err(ProfileError::new(format!(
            "cannot update one setting because the current profile is unreadable: {error}. Replace it with '..entry.apply --file <path>'"
        ))),
        EntryProfileState::Ready(profile) => Ok(profile.record().clone()),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod provider_state_tests;
