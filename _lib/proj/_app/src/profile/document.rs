use serde::Serialize;
use std::collections::BTreeMap;

use super::{EntryProfileRecord, EntryProfileState};

pub const PROFILE_DOCUMENT_PROTOCOL: &str = "swawkit.entry-profile-state/v5";

/// Transport-neutral representation shared by the CLI and Web API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryProfileDocument {
    pub protocol: &'static str,
    pub revision: String,
    pub status: &'static str,
    pub required_complete: bool,
    pub path: String,
    pub profile: EntryProfileRecord,
    pub settings: BTreeMap<&'static str, String>,
    pub resolved_target_project_root: Option<String>,
    pub error: Option<String>,
}

impl EntryProfileDocument {
    pub(super) fn from_state(state: EntryProfileState, path: String, revision: String) -> Self {
        match state {
            EntryProfileState::Missing { .. } => Self {
                protocol: PROFILE_DOCUMENT_PROTOCOL,
                revision,
                status: "setupRequired",
                required_complete: false,
                path,
                settings: EntryProfileRecord::default().profile_setting_values(),
                profile: EntryProfileRecord::default(),
                resolved_target_project_root: None,
                error: None,
            },
            EntryProfileState::Invalid { record, error, .. } => {
                let profile = record.unwrap_or_default();
                Self {
                    protocol: PROFILE_DOCUMENT_PROTOCOL,
                    revision,
                    status: "invalid",
                    required_complete: false,
                    path,
                    settings: profile.profile_setting_values(),
                    profile,
                    resolved_target_project_root: None,
                    error: Some(error),
                }
            }
            EntryProfileState::Ready(profile) => Self {
                protocol: PROFILE_DOCUMENT_PROTOCOL,
                revision,
                status: "ready",
                required_complete: true,
                path,
                resolved_target_project_root: Some(
                    profile
                        .binding()
                        .target_project_root()
                        .display()
                        .to_string(),
                ),
                settings: profile.record().profile_setting_values(),
                profile: profile.record().clone(),
                error: None,
            },
        }
    }
}
