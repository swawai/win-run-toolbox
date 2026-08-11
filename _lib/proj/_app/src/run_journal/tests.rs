use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use super::*;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("workspace root");
        let root = workspace
            .join("data/proj_cache/tests/run-journal")
            .join(format!("{}-{sequence}", std::process::id()));
        fs::create_dir_all(&root).expect("create run journal fixture");
        Self { root }
    }

    fn start(&self, source: RunJournalSource) -> RunJournal {
        RunJournal::start(StartRunJournal {
            module_data_root: self.root.clone(),
            address: ".fixture".to_owned(),
            source,
            argument_count: 2,
            profile_revision: "sha256-fixture".to_owned(),
        })
        .expect("start run journal")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn publishes_append_only_events_and_an_atomic_terminal_state() {
    let fixture = Fixture::new();
    let journal = fixture.start(RunJournalSource::Cli);
    let id = journal.id().unwrap();

    journal
        .output(
            RunJournalPhase::GuardGlobal,
            RunJournalStream::Stdout,
            "guard\n".to_owned(),
        )
        .unwrap();
    journal
        .output(
            RunJournalPhase::Run,
            RunJournalStream::Stderr,
            "target\n".to_owned(),
        )
        .unwrap();
    journal.finish_exited(7).unwrap();

    let run_root = fixture.root.join(JOURNAL_DIRECTORY_NAME).join(&id);
    let state: Value =
        serde_json::from_slice(&fs::read(run_root.join(JOURNAL_STATE_FILE_NAME)).unwrap()).unwrap();
    assert_eq!(state["schema"], JOURNAL_STATE_SCHEMA);
    assert_eq!(state["status"], "exited");
    assert_eq!(state["exitCode"], 7);
    assert_eq!(state["eventCount"], 2);
    assert!(state["finishedAtUnixMs"].as_u64().is_some());

    let lines = fs::read_to_string(run_root.join(JOURNAL_EVENTS_FILE_NAME)).unwrap();
    let events = lines
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["schema"], JOURNAL_EVENT_SCHEMA);
    assert_eq!(events[0]["runId"], id);
    assert_eq!(events[0]["phase"], "guard-global");
    assert_eq!(events[1]["stream"], "stderr");
}

#[test]
fn reads_history_and_incremental_run_documents() {
    let fixture = Fixture::new();
    let journal = fixture.start(RunJournalSource::Web);
    let id = journal.id().unwrap();
    journal
        .output(
            RunJournalPhase::Worker,
            RunJournalStream::Stdout,
            "one".to_owned(),
        )
        .unwrap();
    journal
        .output(
            RunJournalPhase::Worker,
            RunJournalStream::Stdout,
            "two".to_owned(),
        )
        .unwrap();
    journal.finish_canceled().unwrap();

    let history =
        serde_json::to_value(read_run_history(&fixture.root, ".fixture").expect("read history"))
            .unwrap();
    assert_eq!(history["protocol"], "swawkit.command-run-history/v1");
    assert_eq!(history["runs"][0]["id"], id);
    assert_eq!(history["runs"][0]["state"], "canceled");

    let document =
        serde_json::to_value(read_run(&fixture.root, ".fixture", &id, 1).expect("read journal"))
            .unwrap();
    assert_eq!(document["protocol"], "swawkit.command-run-journal/v1");
    assert_eq!(document["nextCursor"], 2);
    assert_eq!(document["events"].as_array().unwrap().len(), 1);
    assert_eq!(document["events"][0]["sequence"], 2);
    assert_eq!(document["events"][0]["text"], "two");
}

#[test]
fn failed_runs_have_a_complete_failure_state() {
    let fixture = Fixture::new();
    let journal = fixture.start(RunJournalSource::Cli);
    let id = journal.id().unwrap();

    journal.finish_failed("entry could not start").unwrap();

    let document = serde_json::to_value(
        read_run(&fixture.root, ".fixture", &id, 0).expect("read failed journal"),
    )
    .unwrap();
    assert_eq!(document["state"], "failed");
    assert_eq!(document["error"], "entry could not start");
    assert_eq!(document["exitCode"], Value::Null);
}

#[test]
fn a_running_reader_ignores_only_an_incomplete_trailing_record() {
    let fixture = Fixture::new();
    let journal = fixture.start(RunJournalSource::Web);
    let id = journal.id().unwrap();
    journal
        .output(
            RunJournalPhase::Worker,
            RunJournalStream::Stdout,
            "complete".to_owned(),
        )
        .unwrap();
    drop(journal);
    let events_path = fixture
        .root
        .join(JOURNAL_DIRECTORY_NAME)
        .join(&id)
        .join(JOURNAL_EVENTS_FILE_NAME);
    OpenOptions::new()
        .append(true)
        .open(events_path)
        .unwrap()
        .write_all(b"{\"partial\"")
        .unwrap();

    let document = serde_json::to_value(
        read_run(&fixture.root, ".fixture", &id, 0).expect("read running journal"),
    )
    .unwrap();
    assert_eq!(document["state"], "running");
    assert_eq!(document["nextCursor"], 1);
    assert_eq!(document["events"].as_array().unwrap().len(), 1);
}
