use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use serde::Serialize;

use super::{
    JOURNAL_DIRECTORY_NAME, JOURNAL_EVENT_SCHEMA, JOURNAL_EVENTS_FILE_NAME,
    JOURNAL_STATE_FILE_NAME, JOURNAL_STATE_SCHEMA, RunJournalPhase, RunJournalSource,
    RunJournalStatus, RunJournalStream, StoredRunEvent, StoredRunState, assert_plain_directory,
    assert_plain_file, valid_run_id,
};

const HISTORY_PROTOCOL: &str = "swawkit.command-run-history/v1";
const DOCUMENT_PROTOCOL: &str = "swawkit.command-run-journal/v1";
const MAX_HISTORY_RUNS: usize = 32;
const MAX_RESPONSE_EVENTS: usize = 4096;
const MAX_RESPONSE_TEXT_BYTES: usize = 1024 * 1024;
const MAX_STORED_EVENTS_FILE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunJournalHistoryDocument {
    protocol: &'static str,
    address: String,
    runs: Vec<RunJournalSummary>,
}

impl RunJournalHistoryDocument {
    pub(crate) fn run_id_at(&self, index: usize) -> Option<String> {
        self.runs.get(index).map(|run| run.id.clone())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunJournalSummary {
    id: String,
    source: RunJournalSource,
    state: RunJournalStatus,
    started_at_unix_ms: u64,
    finished_at_unix_ms: Option<u64>,
    exit_code: Option<i32>,
    error: Option<String>,
    argument_count: usize,
    event_count: u64,
    truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunJournalDocument {
    protocol: &'static str,
    id: String,
    address: String,
    source: RunJournalSource,
    state: RunJournalStatus,
    started_at_unix_ms: u64,
    finished_at_unix_ms: Option<u64>,
    exit_code: Option<i32>,
    error: Option<String>,
    argument_count: usize,
    profile_revision: String,
    next_cursor: u64,
    events: Vec<RunJournalEvent>,
    truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunJournalEvent {
    sequence: u64,
    timestamp_unix_ms: u64,
    phase: RunJournalPhase,
    stream: RunJournalStream,
    text: String,
}

pub(crate) fn read_run_history(
    module_data_root: &Path,
    address: &str,
) -> io::Result<RunJournalHistoryDocument> {
    let journals_root = module_data_root.join(JOURNAL_DIRECTORY_NAME);
    if !journals_root.exists() {
        return Ok(RunJournalHistoryDocument {
            protocol: HISTORY_PROTOCOL,
            address: address.to_owned(),
            runs: Vec::new(),
        });
    }
    assert_plain_directory(&journals_root)?;

    let mut states = Vec::new();
    for entry in fs::read_dir(&journals_root)? {
        let entry = entry?;
        let id = entry.file_name();
        let Some(id) = id.to_str().filter(|id| valid_run_id(id)) else {
            continue;
        };
        let run_root = entry.path();
        assert_plain_directory(&run_root)?;
        states.push(read_state(&run_root, id, address)?);
    }
    states.sort_by(|left, right| {
        right
            .started_at_unix_ms
            .cmp(&left.started_at_unix_ms)
            .then_with(|| right.id.cmp(&left.id))
    });
    states.truncate(MAX_HISTORY_RUNS);

    Ok(RunJournalHistoryDocument {
        protocol: HISTORY_PROTOCOL,
        address: address.to_owned(),
        runs: states.into_iter().map(Into::into).collect(),
    })
}

pub(crate) fn read_run(
    module_data_root: &Path,
    address: &str,
    id: &str,
    after: u64,
) -> io::Result<RunJournalDocument> {
    if !valid_run_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "run journal not found",
        ));
    }
    let run_root = module_data_root.join(JOURNAL_DIRECTORY_NAME).join(id);
    if !run_root.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "run journal not found",
        ));
    }
    assert_plain_directory(&run_root)?;
    let state = read_state(&run_root, id, address)?;
    let (events, next_cursor, response_truncated) = read_events(
        &run_root,
        id,
        after,
        state.status == RunJournalStatus::Running,
    )?;
    if state.status.is_terminal() && state.event_count != next_cursor {
        return Err(invalid(
            "terminal run journal event count does not match events.jsonl",
        ));
    }
    if !state.status.is_terminal() && state.event_count > next_cursor {
        return Err(invalid(
            "running run journal event count is ahead of events.jsonl",
        ));
    }

    Ok(RunJournalDocument {
        protocol: DOCUMENT_PROTOCOL,
        id: state.id,
        address: state.address,
        source: state.source,
        state: state.status,
        started_at_unix_ms: state.started_at_unix_ms,
        finished_at_unix_ms: state.finished_at_unix_ms,
        exit_code: state.exit_code,
        error: state.error,
        argument_count: state.argument_count,
        profile_revision: state.profile_revision,
        next_cursor,
        events,
        truncated: state.truncated || response_truncated,
    })
}

pub(crate) fn read_run_directory(
    module_data_root: &Path,
    address: &str,
    id: &str,
) -> io::Result<std::path::PathBuf> {
    if !valid_run_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "run journal not found",
        ));
    }
    let run_root = module_data_root.join(JOURNAL_DIRECTORY_NAME).join(id);
    if !run_root.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "run journal not found",
        ));
    }
    assert_plain_directory(&run_root)?;
    read_state(&run_root, id, address)?;
    Ok(run_root)
}

fn read_state(run_root: &Path, expected_id: &str, address: &str) -> io::Result<StoredRunState> {
    let path = run_root.join(JOURNAL_STATE_FILE_NAME);
    assert_plain_file(&path)?;
    let content = fs::read(path)?;
    let state: StoredRunState = serde_json::from_slice(&content).map_err(invalid)?;
    if state.schema != JOURNAL_STATE_SCHEMA || state.id != expected_id || state.address != address {
        return Err(invalid("run journal identity does not match its directory"));
    }
    validate_state(&state)?;
    Ok(state)
}

fn validate_state(state: &StoredRunState) -> io::Result<()> {
    let valid = match state.status {
        RunJournalStatus::Running => {
            state.finished_at_unix_ms.is_none()
                && state.exit_code.is_none()
                && state.error.is_none()
        }
        RunJournalStatus::Exited => {
            state.finished_at_unix_ms.is_some()
                && state.exit_code.is_some()
                && state.error.is_none()
        }
        RunJournalStatus::Canceled => {
            state.finished_at_unix_ms.is_some()
                && state.exit_code.is_none()
                && state.error.is_none()
        }
        RunJournalStatus::Failed => {
            state.finished_at_unix_ms.is_some()
                && state.exit_code.is_none()
                && state.error.as_ref().is_some_and(|error| !error.is_empty())
        }
    };
    if !valid || state.profile_revision.is_empty() {
        return Err(invalid("run journal state fields are inconsistent"));
    }
    Ok(())
}

fn read_events(
    run_root: &Path,
    expected_id: &str,
    after: u64,
    allow_incomplete_tail: bool,
) -> io::Result<(Vec<RunJournalEvent>, u64, bool)> {
    let path = run_root.join(JOURNAL_EVENTS_FILE_NAME);
    assert_plain_file(&path)?;
    if fs::metadata(&path)?.len() > MAX_STORED_EVENTS_FILE_BYTES {
        return Err(invalid(
            "run journal events file exceeds its storage contract",
        ));
    }
    let mut reader = BufReader::new(File::open(path)?);
    let mut events = VecDeque::new();
    let mut response_bytes = 0;
    let mut previous_sequence = 0;
    let mut truncated = false;

    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        if line.last() != Some(&b'\n') {
            if allow_incomplete_tail {
                break;
            }
            return Err(invalid(
                "terminal run journal has an incomplete event record",
            ));
        }
        line.pop();
        let event: StoredRunEvent = serde_json::from_slice(&line).map_err(invalid)?;
        if event.schema != JOURNAL_EVENT_SCHEMA
            || event.run_id != expected_id
            || event.sequence != previous_sequence + 1
        {
            return Err(invalid(
                "run journal events are not a contiguous run stream",
            ));
        }
        previous_sequence = event.sequence;
        if event.sequence <= after {
            continue;
        }
        response_bytes += event.text.len();
        events.push_back(RunJournalEvent {
            sequence: event.sequence,
            timestamp_unix_ms: event.timestamp_unix_ms,
            phase: event.phase,
            stream: event.stream,
            text: event.text,
        });
        while response_bytes > MAX_RESPONSE_TEXT_BYTES || events.len() > MAX_RESPONSE_EVENTS {
            let removed = events.pop_front().expect("response contains an event");
            response_bytes -= removed.text.len();
            truncated = true;
        }
    }
    Ok((events.into_iter().collect(), previous_sequence, truncated))
}

impl From<StoredRunState> for RunJournalSummary {
    fn from(state: StoredRunState) -> Self {
        Self {
            id: state.id,
            source: state.source,
            state: state.status,
            started_at_unix_ms: state.started_at_unix_ms,
            finished_at_unix_ms: state.finished_at_unix_ms,
            exit_code: state.exit_code,
            error: state.error,
            argument_count: state.argument_count,
            event_count: state.event_count,
            truncated: state.truncated,
        }
    }
}

fn invalid(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
