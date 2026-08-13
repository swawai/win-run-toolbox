use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

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

    fn owner_path(&self, id: &str) -> PathBuf {
        self.root
            .join(JOURNAL_DIRECTORY_NAME)
            .join(format!(".{id}.owner.lock"))
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

    let first = journal
        .output(
            RunJournalPhase::GuardGlobal,
            RunJournalStream::Stdout,
            "guard\n".to_owned(),
        )
        .unwrap()
        .expect("non-empty journal event");
    let second = journal
        .output(
            RunJournalPhase::Run,
            RunJournalStream::Stderr,
            "target\n".to_owned(),
        )
        .unwrap()
        .expect("non-empty journal event");
    journal.finish_exited(7).unwrap();

    let run_root = fixture.root.join(JOURNAL_DIRECTORY_NAME).join(&id);
    let state: Value =
        serde_json::from_slice(&fs::read(run_root.join(JOURNAL_STATE_FILE_NAME)).unwrap()).unwrap();
    assert_eq!(state["schema"], JOURNAL_STATE_SCHEMA);
    assert_eq!(state["status"], "exited");
    assert_eq!(state["exitCode"], 7);
    assert_eq!(state["eventCount"], 2);
    assert!(state["finishedAtUnixMs"].as_u64().is_some());
    assert!(!fixture.owner_path(&id).exists());

    let lines = fs::read_to_string(run_root.join(JOURNAL_EVENTS_FILE_NAME)).unwrap();
    let events = lines
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["schema"], JOURNAL_EVENT_SCHEMA);
    assert_eq!(events[0]["runId"], id);
    assert_eq!(events[0]["sequence"], first.sequence);
    assert_eq!(events[0]["phase"], "guard-global");
    assert_eq!(events[0]["kind"], "output");
    assert_eq!(events[1]["sequence"], second.sequence);
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
    assert_eq!(document["events"][0]["kind"], "output");
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
fn a_failed_terminal_publication_keeps_the_writer_retryable() {
    let fixture = Fixture::new();
    let journal = fixture.start(RunJournalSource::Cli);
    let id = journal.id().unwrap();
    let state_path = fixture
        .root
        .join(JOURNAL_DIRECTORY_NAME)
        .join(&id)
        .join(JOURNAL_STATE_FILE_NAME);
    let blocker = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&state_path)
        .unwrap();

    let error = journal.finish_exited(0).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("cannot commit atomic publication")
    );
    let running: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(running["status"], "running");
    drop(blocker);

    journal
        .finish_exited(0)
        .expect("retry terminal publication");
    let completed = serde_json::to_value(
        read_run(&fixture.root, ".fixture", &id, 0).expect("read retried journal"),
    )
    .unwrap();
    assert_eq!(completed["state"], "exited");
    assert_eq!(completed["exitCode"], 0);
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

#[test]
fn an_abandoned_writer_is_atomically_reconciled_and_discards_only_its_partial_tail() {
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
        .open(&events_path)
        .unwrap()
        .write_all(b"{\"partial\"")
        .unwrap();

    let history =
        serde_json::to_value(read_run_history(&fixture.root, ".fixture").unwrap()).unwrap();
    assert_eq!(history["runs"][0]["state"], "failed");
    assert_eq!(history["runs"][0]["eventCount"], 1);
    assert!(!fs::read_to_string(events_path).unwrap().contains("partial"));
    assert!(!fixture.owner_path(&id).exists());

    let document = serde_json::to_value(
        read_run(&fixture.root, ".fixture", &id, 0).expect("read reconciled journal"),
    )
    .unwrap();
    assert_eq!(document["state"], "failed");
    assert_eq!(document["error"], INTERRUPTED_ERROR);
    assert_eq!(document["nextCursor"], 1);
    assert!(document["finishedAtUnixMs"].as_u64().is_some());
}

#[test]
fn unpublished_work_and_owner_entries_are_not_journals() {
    let fixture = Fixture::new();
    let journals_root = fixture.root.join(JOURNAL_DIRECTORY_NAME);
    fs::create_dir(&journals_root).unwrap();
    let id = "0000000000000001-00000001-0000000000000001";
    let work_root = journals_root.join(format!(".{id}.work"));
    fs::create_dir(&work_root).unwrap();
    fs::write(work_root.join(JOURNAL_EVENTS_FILE_NAME), b"partial").unwrap();
    fs::write(journals_root.join(format!(".{id}.owner.lock")), b"").unwrap();

    let history =
        serde_json::to_value(read_run_history(&fixture.root, ".fixture").unwrap()).unwrap();
    assert!(history["runs"].as_array().unwrap().is_empty());
}

#[test]
fn a_legacy_running_journal_without_an_owner_lease_is_not_guessed() {
    let fixture = Fixture::new();
    let journal = fixture.start(RunJournalSource::Cli);
    let id = journal.id().unwrap();
    drop(journal);
    fs::remove_file(fixture.owner_path(&id)).unwrap();

    let document =
        serde_json::to_value(read_run(&fixture.root, ".fixture", &id, 0).unwrap()).unwrap();
    assert_eq!(document["state"], "running");
    assert_eq!(document["finishedAtUnixMs"], Value::Null);
}

#[test]
fn reconciliation_does_not_hide_a_state_that_claims_missing_events() {
    let fixture = Fixture::new();
    let journal = fixture.start(RunJournalSource::Cli);
    let id = journal.id().unwrap();
    journal
        .output(
            RunJournalPhase::Run,
            RunJournalStream::Stdout,
            "only event".to_owned(),
        )
        .unwrap();
    drop(journal);
    let state_path = fixture
        .root
        .join(JOURNAL_DIRECTORY_NAME)
        .join(&id)
        .join(JOURNAL_STATE_FILE_NAME);
    let mut state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    state["eventCount"] = 2.into();
    fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let error = read_run(&fixture.root, ".fixture", &id, 0).unwrap_err();
    assert!(error.to_string().contains("event count is ahead"));
    let unchanged: Value = serde_json::from_slice(&fs::read(state_path).unwrap()).unwrap();
    assert_eq!(unchanged["status"], "running");
    assert_eq!(unchanged["eventCount"], 2);
}

#[test]
fn a_process_exit_releases_ownership_and_the_next_reader_reconciles_the_run() {
    const ROOT_VARIABLE: &str = "SWAWKIT_PROJ_TEST_ABANDONED_JOURNAL_ROOT";
    let fixture = Fixture::new();
    let id_path = fixture.root.join("child-run-id.txt");
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("run_journal::tests::subprocess_abandoned_writer")
        .arg("--nocapture")
        .env(ROOT_VARIABLE, &fixture.root)
        .status()
        .expect("start abandoned journal subprocess");
    assert!(status.success());
    let id = fs::read_to_string(id_path).unwrap();

    let document =
        serde_json::to_value(read_run(&fixture.root, ".fixture", &id, 0).unwrap()).unwrap();
    assert_eq!(document["state"], "failed");
    assert_eq!(document["error"], INTERRUPTED_ERROR);
    assert_eq!(document["nextCursor"], 1);
}

#[test]
fn subprocess_abandoned_writer() {
    const ROOT_VARIABLE: &str = "SWAWKIT_PROJ_TEST_ABANDONED_JOURNAL_ROOT";
    let Some(root) = std::env::var_os(ROOT_VARIABLE).map(PathBuf::from) else {
        return;
    };
    let journal = RunJournal::start(StartRunJournal {
        module_data_root: root.clone(),
        address: ".fixture".to_owned(),
        source: RunJournalSource::Cli,
        argument_count: 0,
        profile_revision: "sha256-subprocess".to_owned(),
    })
    .unwrap();
    let id = journal.id().unwrap();
    journal
        .output(
            RunJournalPhase::Run,
            RunJournalStream::Stdout,
            "before exit".to_owned(),
        )
        .unwrap();
    fs::write(root.join("child-run-id.txt"), id).unwrap();
    std::process::exit(0);
}
