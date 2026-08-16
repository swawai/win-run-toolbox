use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::command_event::CommandProgress;

use super::{EntryOutputStream, EntryRunObserver, EntryRunOutcome, EntryRunSpec, EntryRunner};

const QUERY_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_QUERY_OUTPUT_BYTES: usize = 1024 * 1024;
static NEXT_QUERY: AtomicU64 = AtomicU64::new(0);

pub(crate) struct EntryQueryOutput {
    pub stdout: String,
}

pub(crate) fn run_entry_query(
    runner: Arc<dyn EntryRunner>,
    mut spec: EntryRunSpec,
) -> Result<EntryQueryOutput, String> {
    spec.id = format!(
        "query-{}-{}",
        std::process::id(),
        NEXT_QUERY.fetch_add(1, Ordering::Relaxed)
    );
    run_entry_query_with(runner, spec, QUERY_TIMEOUT, MAX_QUERY_OUTPUT_BYTES)
}

fn run_entry_query_with(
    runner: Arc<dyn EntryRunner>,
    spec: EntryRunSpec,
    timeout: Duration,
    max_output_bytes: usize,
) -> Result<EntryQueryOutput, String> {
    let observer = Arc::new(QueryObserver::new(max_output_bytes));
    let observer_boundary: Arc<dyn EntryRunObserver> = observer.clone();
    let control = runner
        .start(spec, observer_boundary)
        .map_err(|error| format!("cannot start the facet query: {error}"))?;

    let wait = observer.wait(timeout);
    if matches!(wait, QueryWait::TimedOut | QueryWait::Rejected(_)) {
        let _ = control.cancel();
    }
    let join = control.join();
    let state = observer.snapshot()?;
    join.map_err(|error| format!("cannot join the facet query: {error}"))?;

    match wait {
        QueryWait::TimedOut => return Err("facet query timed out".to_owned()),
        QueryWait::Rejected(error) => return Err(error),
        QueryWait::Completed => {}
    }
    match state.outcome {
        Some(EntryRunOutcome::Exited(0)) => {}
        Some(EntryRunOutcome::Exited(exit_code)) => {
            return Err(format!("facet query exited with code {exit_code}"));
        }
        Some(EntryRunOutcome::Failed(error)) => {
            return Err(format!("facet query failed: {error}"));
        }
        None => return Err("facet query completed without an outcome".to_owned()),
    }
    if !state.stderr.is_empty() {
        return Err("facet query wrote to stderr".to_owned());
    }
    Ok(EntryQueryOutput {
        stdout: state.stdout,
    })
}

struct QueryObserver {
    state: Mutex<QueryState>,
    completed: Condvar,
    max_output_bytes: usize,
}

impl QueryObserver {
    fn new(max_output_bytes: usize) -> Self {
        Self {
            state: Mutex::new(QueryState::default()),
            completed: Condvar::new(),
            max_output_bytes,
        }
    }

    fn wait(&self, timeout: Duration) -> QueryWait {
        let Ok(state) = self.state.lock() else {
            return QueryWait::Rejected("facet query state is unavailable".to_owned());
        };
        let Ok((state, result)) = self.completed.wait_timeout_while(state, timeout, |state| {
            state.outcome.is_none() && state.rejection.is_none()
        }) else {
            return QueryWait::Rejected("facet query state is unavailable".to_owned());
        };
        if let Some(error) = &state.rejection {
            QueryWait::Rejected(error.clone())
        } else if state.outcome.is_some() {
            QueryWait::Completed
        } else if result.timed_out() {
            QueryWait::TimedOut
        } else {
            QueryWait::Rejected("facet query wait ended without an outcome".to_owned())
        }
    }

    fn snapshot(&self) -> Result<QueryState, String> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| "facet query state is unavailable".to_owned())
    }

    fn reject(&self, error: &'static str) {
        if let Ok(mut state) = self.state.lock() {
            if state.rejection.is_none() {
                state.rejection = Some(error.to_owned());
            }
        }
        self.completed.notify_all();
    }
}

impl EntryRunObserver for QueryObserver {
    fn output(&self, stream: EntryOutputStream, text: String) {
        if text.is_empty() {
            return;
        }
        let accepted = if let Ok(mut state) = self.state.lock() {
            let next_size = state.output_bytes.checked_add(text.len());
            if state.rejection.is_some()
                || next_size.is_none_or(|size| size > self.max_output_bytes)
            {
                false
            } else {
                state.output_bytes = next_size.expect("bounded output size");
                match stream {
                    EntryOutputStream::Stdout => state.stdout.push_str(&text),
                    EntryOutputStream::Stderr => state.stderr.push_str(&text),
                }
                true
            }
        } else {
            false
        };
        if !accepted {
            self.reject("facet query output exceeded the byte limit");
        }
    }

    fn progress(&self, _progress: CommandProgress) {
        self.reject("facet query emitted a progress event");
    }

    fn completed(&self, outcome: EntryRunOutcome) {
        if let Ok(mut state) = self.state.lock()
            && state.outcome.is_none()
        {
            state.outcome = Some(outcome);
        }
        self.completed.notify_all();
    }
}

#[derive(Clone, Default)]
struct QueryState {
    stdout: String,
    stderr: String,
    output_bytes: usize,
    outcome: Option<EntryRunOutcome>,
    rejection: Option<String>,
}

enum QueryWait {
    Completed,
    Rejected(String),
    TimedOut,
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};

    use crate::entry_runner::EntryRunControl;

    use super::*;

    struct ImmediateRunner {
        stdout: &'static str,
        stderr: &'static str,
        outcome: Option<EntryRunOutcome>,
        canceled: Arc<AtomicBool>,
    }

    impl EntryRunner for ImmediateRunner {
        fn start(
            &self,
            _spec: EntryRunSpec,
            observer: Arc<dyn EntryRunObserver>,
        ) -> io::Result<Arc<dyn EntryRunControl>> {
            observer.output(EntryOutputStream::Stdout, self.stdout.to_owned());
            observer.output(EntryOutputStream::Stderr, self.stderr.to_owned());
            if let Some(outcome) = self.outcome.clone() {
                observer.completed(outcome);
            }
            Ok(Arc::new(TestControl {
                observer,
                canceled: Arc::clone(&self.canceled),
            }))
        }
    }

    struct TestControl {
        observer: Arc<dyn EntryRunObserver>,
        canceled: Arc<AtomicBool>,
    }

    impl EntryRunControl for TestControl {
        fn cancel(&self) -> io::Result<()> {
            self.canceled.store(true, Ordering::Release);
            self.observer.completed(EntryRunOutcome::Exited(1223));
            Ok(())
        }

        fn join(&self) -> Result<(), String> {
            Ok(())
        }
    }

    fn spec() -> EntryRunSpec {
        EntryRunSpec {
            id: String::new(),
            entry_file: PathBuf::from("entry.exe"),
            working_directory: PathBuf::from("."),
            argv: vec![OsString::from(".query")],
        }
    }

    #[test]
    fn returns_only_clean_successful_stdout() {
        let canceled = Arc::new(AtomicBool::new(false));
        let output = run_entry_query_with(
            Arc::new(ImmediateRunner {
                stdout: "{\"protocol\":\"fixture/v1\"}",
                stderr: "",
                outcome: Some(EntryRunOutcome::Exited(0)),
                canceled,
            }),
            spec(),
            Duration::from_secs(1),
            1024,
        )
        .unwrap();
        assert_eq!(output.stdout, "{\"protocol\":\"fixture/v1\"}");
    }

    #[test]
    fn rejects_stderr_nonzero_overflow_and_timeout() {
        for runner in [
            ImmediateRunner {
                stdout: "{}",
                stderr: "warning",
                outcome: Some(EntryRunOutcome::Exited(0)),
                canceled: Arc::new(AtomicBool::new(false)),
            },
            ImmediateRunner {
                stdout: "{}",
                stderr: "",
                outcome: Some(EntryRunOutcome::Exited(7)),
                canceled: Arc::new(AtomicBool::new(false)),
            },
        ] {
            assert!(
                run_entry_query_with(Arc::new(runner), spec(), Duration::from_secs(1), 1024,)
                    .is_err()
            );
        }

        let overflow_canceled = Arc::new(AtomicBool::new(false));
        assert!(
            run_entry_query_with(
                Arc::new(ImmediateRunner {
                    stdout: "too large",
                    stderr: "",
                    outcome: None,
                    canceled: Arc::clone(&overflow_canceled),
                }),
                spec(),
                Duration::from_secs(1),
                2,
            )
            .is_err()
        );
        assert!(overflow_canceled.load(Ordering::Acquire));

        let timeout_canceled = Arc::new(AtomicBool::new(false));
        assert!(
            run_entry_query_with(
                Arc::new(ImmediateRunner {
                    stdout: "",
                    stderr: "",
                    outcome: None,
                    canceled: Arc::clone(&timeout_canceled),
                }),
                spec(),
                Duration::from_millis(1),
                1024,
            )
            .is_err()
        );
        assert!(timeout_canceled.load(Ordering::Acquire));
    }
}
