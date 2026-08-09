use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::*;

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
    let run = CommandRun::new("run".to_owned(), ".fixture".to_owned());
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
    let run = CommandRun::new("run".to_owned(), ".fixture".to_owned());

    let error = run.cancel().expect_err("missing control must fail");

    assert!(error.to_string().contains("is not attached"));
    assert_eq!(
        run.document(0).expect("command run document").state,
        CommandRunState::Running
    );
}

#[test]
fn bounds_small_output_chunks_by_event_count() {
    let run = CommandRun::new("run".to_owned(), ".fixture".to_owned());

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
    let run = Arc::new(CommandRun::new("run".to_owned(), ".fixture".to_owned()));
    run.attach_control(Arc::new(DropAwareControl(Arc::clone(&dropped))))
        .expect("attach drop-aware control");
    let observer = RunObserver {
        run: Arc::downgrade(&run),
        registry: Weak::new(),
    };

    drop(run);

    assert!(dropped.load(Ordering::Acquire));
    observer.output(EntryOutputStream::Stdout, "ignored".to_owned());
    observer.completed(EntryRunOutcome::Exited(0));
}
