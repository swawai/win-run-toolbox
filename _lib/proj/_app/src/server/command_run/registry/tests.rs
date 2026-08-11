use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use super::*;
use crate::entry_runner::{EntryOutputStream, EntryRunControl};
use crate::run_journal::{RunJournalSource, StartRunJournal};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct RunFixture {
    root: PathBuf,
    run: Option<Arc<CommandRun>>,
}

impl RunFixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("workspace root");
        let root = workspace
            .join("data/proj_cache/tests/run-registry")
            .join(format!("{}-{sequence}", std::process::id()));
        let journal = RunJournal::start(StartRunJournal {
            module_data_root: root.clone(),
            address: ".fixture".to_owned(),
            source: RunJournalSource::Web,
            argument_count: 0,
            profile_revision: "sha256-fixture".to_owned(),
        })
        .expect("start fixture journal");
        let id = journal.id().expect("fixture journal id");
        Self {
            root,
            run: Some(Arc::new(CommandRun::new(
                id,
                ".fixture".to_owned(),
                journal,
            ))),
        }
    }

    fn run(&self) -> &Arc<CommandRun> {
        self.run.as_ref().expect("fixture command run")
    }

    fn drop_run(&mut self) {
        drop(self.run.take());
    }
}

impl Drop for RunFixture {
    fn drop(&mut self) {
        self.drop_run();
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct FailingControl;

impl EntryRunControl for FailingControl {
    fn cancel(&self) -> io::Result<()> {
        Err(io::Error::other("fixture cancellation failed"))
    }

    fn join(&self) -> Result<(), String> {
        Ok(())
    }
}

struct DropAwareControl(Arc<AtomicBool>);

impl EntryRunControl for DropAwareControl {
    fn cancel(&self) -> io::Result<()> {
        Ok(())
    }

    fn join(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Drop for DropAwareControl {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[test]
fn failed_cancellation_restores_a_running_state() {
    let fixture = RunFixture::new();
    let run = fixture.run();
    run.attach_control(Arc::new(FailingControl))
        .expect("attach fixture control");

    let error = run.cancel().expect_err("fixture cancellation must fail");

    assert!(error.to_string().contains("fixture cancellation failed"));
    assert_eq!(
        run.document(0).expect("command run document").state,
        CommandRunState::Running
    );
}

#[test]
fn missing_control_fails_and_restores_a_running_state() {
    let fixture = RunFixture::new();
    let run = fixture.run();

    let error = run.cancel().expect_err("missing control must fail");

    assert!(error.to_string().contains("is not attached"));
    assert_eq!(
        run.document(0).expect("command run document").state,
        CommandRunState::Running
    );
}

#[test]
fn bounds_small_output_chunks_by_event_count() {
    let fixture = RunFixture::new();
    let run = fixture.run();

    for _ in 0..=MAX_OUTPUT_EVENTS {
        run.append(EntryOutputStream::Stdout, "x".to_owned());
    }

    let document = run.document(0).expect("command run document");
    assert!(document.truncated);
    assert_eq!(document.events.len(), MAX_OUTPUT_EVENTS);
    assert_eq!(document.events[0].sequence, 2);
    assert_eq!(document.next_cursor, (MAX_OUTPUT_EVENTS + 1) as u64);
}

#[test]
fn observer_does_not_keep_a_dropped_run_control_alive() {
    let dropped = Arc::new(AtomicBool::new(false));
    let mut fixture = RunFixture::new();
    let run = Arc::clone(fixture.run());
    run.attach_control(Arc::new(DropAwareControl(Arc::clone(&dropped))))
        .expect("attach drop-aware control");
    let observer = RunObserver {
        run: Arc::downgrade(&run),
        registry: Weak::new(),
    };

    drop(run);
    fixture.drop_run();

    assert!(dropped.load(Ordering::Acquire));
    observer.output(EntryOutputStream::Stdout, "ignored".to_owned());
    observer.completed(EntryRunOutcome::Exited(0));
}
