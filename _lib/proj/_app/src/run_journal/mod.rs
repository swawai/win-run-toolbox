mod event;
mod read;

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::atomic_file;

pub(crate) use event::{RunJournalEvent, RunJournalEventData, RunJournalPhase, RunJournalStream};
pub use read::{RunJournalDocument, RunJournalHistoryDocument};
pub(crate) use read::{read_run, read_run_directory, read_run_history};

pub(crate) const JOURNAL_STATE_SCHEMA: &str = "swawkit.command-run-journal/v1";
pub(crate) const JOURNAL_EVENT_SCHEMA: &str = "swawkit.command-run-event/v1";
pub(crate) const JOURNAL_DIRECTORY_NAME: &str = "_runs";
pub(crate) const JOURNAL_STATE_FILE_NAME: &str = "_state.json";
pub(crate) const JOURNAL_EVENTS_FILE_NAME: &str = "events.jsonl";
const MAX_EVENTS_FILE_BYTES: usize = 8 * 1024 * 1024;
const CREATE_ATTEMPTS: usize = 8;

static NEXT_RUN: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RunJournalSource {
    Cli,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RunJournalStatus {
    Running,
    Exited,
    Canceled,
    Failed,
}

impl RunJournalStatus {
    pub(super) fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

pub(crate) struct StartRunJournal {
    pub module_data_root: PathBuf,
    pub address: String,
    pub source: RunJournalSource,
    pub argument_count: usize,
    pub profile_revision: String,
}

#[derive(Clone)]
pub(crate) struct RunJournal {
    inner: Arc<Mutex<Writer>>,
}

impl RunJournal {
    pub(crate) fn start(request: StartRunJournal) -> io::Result<Self> {
        ensure_plain_directory(&request.module_data_root)?;
        let journals_root = request.module_data_root.join(JOURNAL_DIRECTORY_NAME);
        ensure_plain_directory(&journals_root)?;

        let started_at_unix_ms = unix_time_ms()?;
        let mut collision = None;
        for _ in 0..CREATE_ATTEMPTS {
            let id = next_run_id(started_at_unix_ms);
            let run_root = journals_root.join(&id);
            match fs::create_dir(&run_root) {
                Ok(()) => {
                    assert_plain_directory(&run_root)?;
                    return Self::create(request, id, run_root, started_at_unix_ms);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    collision = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(collision.unwrap_or_else(|| io::Error::other("cannot allocate a run journal id")))
    }

    fn create(
        request: StartRunJournal,
        id: String,
        run_root: PathBuf,
        started_at_unix_ms: u64,
    ) -> io::Result<Self> {
        let events_path = run_root.join(JOURNAL_EVENTS_FILE_NAME);
        let events = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&events_path)
        {
            Ok(events) => events,
            Err(error) => {
                let _ = fs::remove_dir(&run_root);
                return Err(error);
            }
        };
        if let Err(error) = events.sync_all() {
            drop(events);
            let _ = fs::remove_file(&events_path);
            let _ = fs::remove_dir(&run_root);
            return Err(error);
        }
        let writer = Writer {
            state_path: run_root.join(JOURNAL_STATE_FILE_NAME),
            events,
            state: StoredRunState {
                schema: JOURNAL_STATE_SCHEMA.to_owned(),
                id,
                address: request.address,
                source: request.source,
                status: RunJournalStatus::Running,
                started_at_unix_ms,
                finished_at_unix_ms: None,
                exit_code: None,
                error: None,
                argument_count: request.argument_count,
                profile_revision: request.profile_revision,
                event_count: 0,
                truncated: false,
            },
            next_sequence: 0,
            stored_event_bytes: 0,
            write_error: None,
        };
        if let Err(error) = writer.publish_state() {
            drop(writer);
            let _ = fs::remove_file(&events_path);
            let _ = fs::remove_dir(&run_root);
            return Err(error);
        }
        Ok(Self {
            inner: Arc::new(Mutex::new(writer)),
        })
    }

    pub(crate) fn id(&self) -> io::Result<String> {
        self.inner
            .lock()
            .map(|writer| writer.state.id.clone())
            .map_err(|_| io::Error::other("run journal is unavailable"))
    }

    pub(crate) fn output(
        &self,
        phase: RunJournalPhase,
        stream: RunJournalStream,
        text: String,
    ) -> io::Result<Option<RunJournalEvent>> {
        if text.is_empty() {
            return Ok(None);
        }
        self.inner
            .lock()
            .map_err(|_| io::Error::other("run journal is unavailable"))?
            .append(phase, RunJournalEventData::Output { stream, text })
            .map(Some)
    }

    pub(crate) fn progress(
        &self,
        phase: RunJournalPhase,
        progress: crate::command_event::CommandProgress,
    ) -> io::Result<RunJournalEvent> {
        self.inner
            .lock()
            .map_err(|_| io::Error::other("run journal is unavailable"))?
            .append(phase, RunJournalEventData::Progress { progress })
    }

    pub(crate) fn finish_exited(&self, exit_code: i32) -> io::Result<()> {
        self.finish(RunJournalStatus::Exited, Some(exit_code), None)
    }

    pub(crate) fn finish_canceled(&self) -> io::Result<()> {
        self.finish(RunJournalStatus::Canceled, None, None)
    }

    pub(crate) fn finish_failed(&self, error: impl Into<String>) -> io::Result<()> {
        let error = error.into();
        self.finish(
            RunJournalStatus::Failed,
            None,
            Some(if error.is_empty() {
                "command failed without an error message".to_owned()
            } else {
                error
            }),
        )
    }

    fn finish(
        &self,
        status: RunJournalStatus,
        exit_code: Option<i32>,
        error: Option<String>,
    ) -> io::Result<()> {
        self.inner
            .lock()
            .map_err(|_| io::Error::other("run journal is unavailable"))?
            .finish(status, exit_code, error)
    }
}

struct Writer {
    state_path: PathBuf,
    events: File,
    state: StoredRunState,
    next_sequence: u64,
    stored_event_bytes: usize,
    write_error: Option<String>,
}

impl Writer {
    fn append(
        &mut self,
        phase: RunJournalPhase,
        data: RunJournalEventData,
    ) -> io::Result<RunJournalEvent> {
        if self.state.status.is_terminal() {
            return Err(io::Error::other("run journal is already complete"));
        }
        if let Some(error) = &self.write_error {
            return Err(io::Error::other(error.clone()));
        }
        let sequence = self.next_sequence + 1;
        self.next_sequence = sequence;
        let event = RunJournalEvent {
            sequence,
            timestamp_unix_ms: unix_time_ms()?,
            phase,
            data,
        };
        let stored = StoredRunEvent {
            schema: JOURNAL_EVENT_SCHEMA.to_owned(),
            run_id: self.state.id.clone(),
            event: event.clone(),
        };
        let mut content = serde_json::to_vec(&stored).map_err(io::Error::other)?;
        content.push(b'\n');
        if self.state.truncated
            || self.stored_event_bytes.saturating_add(content.len()) > MAX_EVENTS_FILE_BYTES
        {
            if !self.state.truncated {
                self.state.truncated = true;
                if let Err(error) = self.publish_state() {
                    return Err(self.remember_error(error));
                }
            }
            return Ok(event);
        }
        if let Err(error) = self
            .events
            .write_all(&content)
            .and_then(|()| self.events.flush())
        {
            return Err(self.remember_error(error));
        }
        self.stored_event_bytes += content.len();
        self.state.event_count = sequence;
        Ok(event)
    }

    fn finish(
        &mut self,
        requested_status: RunJournalStatus,
        requested_exit_code: Option<i32>,
        requested_error: Option<String>,
    ) -> io::Result<()> {
        if self.state.status.is_terminal() {
            return Err(io::Error::other("run journal is already complete"));
        }
        let sync_error = self.events.sync_all().err().map(|error| error.to_string());
        let journal_error = self.write_error.clone().or(sync_error);
        let (status, exit_code, error) = match journal_error.as_deref() {
            Some(error) => (
                RunJournalStatus::Failed,
                None,
                Some(format!("command output journal failed: {error}")),
            ),
            None => (requested_status, requested_exit_code, requested_error),
        };
        self.state.status = status;
        self.state.exit_code = exit_code;
        self.state.error = error;
        self.state.finished_at_unix_ms = Some(unix_time_ms()?);
        self.publish_state()?;
        match journal_error {
            Some(error) => Err(io::Error::other(error)),
            None => Ok(()),
        }
    }

    fn publish_state(&self) -> io::Result<()> {
        let mut content = serde_json::to_vec_pretty(&self.state).map_err(io::Error::other)?;
        content.push(b'\n');
        atomic_file::publish(&self.state_path, &content)
    }

    fn remember_error(&mut self, error: io::Error) -> io::Error {
        let message = error.to_string();
        self.write_error = Some(message.clone());
        io::Error::new(error.kind(), message)
    }
}

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

fn ensure_plain_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    assert_plain_directory(path)
}

pub(super) fn valid_run_id(id: &str) -> bool {
    id.len() == 42
        && id.as_bytes().get(16) == Some(&b'-')
        && id.as_bytes().get(25) == Some(&b'-')
        && id.bytes().enumerate().all(|(index, byte)| {
            index == 16 || index == 25 || byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
        })
}

fn next_run_id(timestamp: u64) -> String {
    let sequence = NEXT_RUN.fetch_add(1, Ordering::Relaxed) + 1;
    format!(
        "{timestamp:016x}-{:08x}-{sequence:016x}",
        std::process::id()
    )
}

fn unix_time_ms() -> io::Result<u64> {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis();
    milliseconds
        .try_into()
        .map_err(|_| io::Error::other("system time is outside the run journal range"))
}

#[cfg(test)]
mod tests;
