use std::fs;
use std::io;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::atomic_file;

use super::{JOURNAL_STATE_SCHEMA, RunJournalEvent, RunJournalSource, RunJournalStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct StoredRunState {
    pub schema: String,
    pub id: String,
    pub address: String,
    pub source: RunJournalSource,
    pub status: RunJournalStatus,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub argument_count: usize,
    pub profile_revision: String,
    pub event_count: u64,
    pub truncated: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredRunEvent {
    pub schema: String,
    pub run_id: String,
    #[serde(flatten)]
    pub event: RunJournalEvent,
}

pub(super) fn publish_stored_state(path: &Path, state: &StoredRunState) -> io::Result<()> {
    let mut content = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
    content.push(b'\n');
    atomic_file::publish(path, &content)
}

pub(super) fn assert_plain_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other(format!(
            "run journal directory must be a normal directory: {}",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn assert_plain_file(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other(format!(
            "run journal file must be a normal file: {}",
            path.display()
        )));
    }
    Ok(())
}

pub(super) fn ensure_plain_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    assert_plain_directory(path)
}

pub(super) fn new_running_state(
    id: &str,
    address: String,
    source: RunJournalSource,
    started_at_unix_ms: u64,
    argument_count: usize,
    profile_revision: String,
) -> StoredRunState {
    StoredRunState {
        schema: JOURNAL_STATE_SCHEMA.to_owned(),
        id: id.to_owned(),
        address,
        source,
        status: RunJournalStatus::Running,
        started_at_unix_ms,
        finished_at_unix_ms: None,
        exit_code: None,
        error: None,
        argument_count,
        profile_revision,
        event_count: 0,
        truncated: false,
    }
}
