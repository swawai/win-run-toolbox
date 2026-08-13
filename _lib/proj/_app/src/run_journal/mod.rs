mod event;
mod owner;
mod read;
mod storage;

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use owner::{RunOwnerLease, remove_owner_file};
use serde::{Deserialize, Serialize};
use storage::{StoredRunEvent, StoredRunState, assert_plain_directory, publish_stored_state};
use storage::{ensure_plain_directory, new_running_state};

pub(crate) use event::{RunJournalEvent, RunJournalEventData, RunJournalPhase, RunJournalStream};
pub use read::{RunJournalDocument, RunJournalHistoryDocument};
pub(crate) use read::{read_run, read_run_directory, read_run_history};

pub(crate) const JOURNAL_STATE_SCHEMA: &str = "swawkit.command-run-journal/v1";
pub(crate) const JOURNAL_EVENT_SCHEMA: &str = "swawkit.command-run-event/v1";
pub(crate) const JOURNAL_DIRECTORY_NAME: &str = "_runs";
pub(crate) const JOURNAL_STATE_FILE_NAME: &str = "_state.json";
pub(crate) const JOURNAL_EVENTS_FILE_NAME: &str = "events.jsonl";
pub(super) const INTERRUPTED_ERROR: &str =
    "command execution owner ended before publishing a terminal state";
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
            if run_root.exists() {
                collision = Some(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "run journal id already exists",
                ));
                continue;
            }
            let owner = match RunOwnerLease::create(&journals_root, &id) {
                Ok(owner) => owner,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    collision = Some(error);
                    continue;
                }
                Err(error) => return Err(error),
            };
            let work_root = journals_root.join(format!(".{id}.work"));
            match fs::create_dir(&work_root) {
                Ok(()) => {
                    if let Err(error) = assert_plain_directory(&work_root) {
                        drop(owner);
                        remove_owner_file(&journals_root, &id);
                        let _ = fs::remove_dir(&work_root);
                        return Err(error);
                    }
                }
                Err(error) => {
                    drop(owner);
                    remove_owner_file(&journals_root, &id);
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        collision = Some(error);
                        continue;
                    }
                    return Err(error);
                }
            }
            return Self::create(request, id, work_root, run_root, owner, started_at_unix_ms);
        }
        Err(collision.unwrap_or_else(|| io::Error::other("cannot allocate a run journal id")))
    }

    fn create(
        request: StartRunJournal,
        id: String,
        work_root: PathBuf,
        run_root: PathBuf,
        owner: RunOwnerLease,
        started_at_unix_ms: u64,
    ) -> io::Result<Self> {
        let journals_root = run_root.parent().expect("run journal has a parent");
        let events_path = work_root.join(JOURNAL_EVENTS_FILE_NAME);
        let prepared_events = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&events_path)
        {
            Ok(events) => events,
            Err(error) => {
                drop(owner);
                remove_owner_file(journals_root, &id);
                let _ = fs::remove_dir(&work_root);
                return Err(error);
            }
        };
        if let Err(error) = prepared_events.sync_all() {
            drop(prepared_events);
            let _ = fs::remove_file(&events_path);
            drop(owner);
            remove_owner_file(journals_root, &id);
            let _ = fs::remove_dir(&work_root);
            return Err(error);
        }
        drop(prepared_events);
        let state = new_running_state(
            &id,
            request.address,
            request.source,
            started_at_unix_ms,
            request.argument_count,
            request.profile_revision,
        );
        let work_state_path = work_root.join(JOURNAL_STATE_FILE_NAME);
        if let Err(error) = publish_stored_state(&work_state_path, &state) {
            let _ = fs::remove_file(&events_path);
            drop(owner);
            remove_owner_file(journals_root, &id);
            let _ = fs::remove_dir(&work_root);
            return Err(error);
        }
        if let Err(error) = fs::rename(&work_root, &run_root) {
            let _ = fs::remove_file(&work_state_path);
            let _ = fs::remove_file(&events_path);
            drop(owner);
            remove_owner_file(journals_root, &id);
            let _ = fs::remove_dir(&work_root);
            return Err(error);
        }
        let events = OpenOptions::new()
            .append(true)
            .open(run_root.join(JOURNAL_EVENTS_FILE_NAME))?;
        let writer = Writer {
            state_path: run_root.join(JOURNAL_STATE_FILE_NAME),
            events,
            owner: Some(owner),
            state,
            next_sequence: 0,
            stored_event_bytes: 0,
            write_error: None,
        };
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
    owner: Option<RunOwnerLease>,
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
        let mut completed = self.state.clone();
        completed.status = status;
        completed.exit_code = exit_code;
        completed.error = error;
        completed.finished_at_unix_ms = Some(unix_time_ms()?);
        publish_stored_state(&self.state_path, &completed)?;
        self.state = completed;
        if let Some(owner) = self.owner.take() {
            owner.release();
        }
        match journal_error {
            Some(error) => Err(io::Error::other(error)),
            None => Ok(()),
        }
    }

    fn publish_state(&self) -> io::Result<()> {
        publish_stored_state(&self.state_path, &self.state)
    }

    fn remember_error(&mut self, error: io::Error) -> io::Error {
        let message = error.to_string();
        self.write_error = Some(message.clone());
        io::Error::new(error.kind(), message)
    }
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

pub(super) fn unix_time_ms() -> io::Result<u64> {
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
