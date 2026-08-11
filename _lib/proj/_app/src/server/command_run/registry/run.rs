use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex, Weak};

use crate::command_event::CommandProgress;
use crate::entry_runner::{EntryOutputStream, EntryRunControl, EntryRunObserver, EntryRunOutcome};
use crate::run_journal::{RunJournal, RunJournalEvent, RunJournalPhase, RunJournalStream};

use super::{RegistryError, RegistryInner};
use crate::server::command_run::{COMMAND_RUN_PROTOCOL, CommandRunDocument, CommandRunState};

const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
pub(super) const MAX_OUTPUT_EVENTS: usize = 4096;

pub(super) struct CommandRun {
    pub(super) id: String,
    address: String,
    journal: RunJournal,
    state: Mutex<RunState>,
    control: Mutex<Option<Arc<dyn EntryRunControl>>>,
}

impl CommandRun {
    pub(super) fn new(id: String, address: String, journal: RunJournal) -> Self {
        Self {
            id,
            address,
            journal,
            state: Mutex::new(RunState::default()),
            control: Mutex::new(None),
        }
    }

    pub(super) fn attach_control(
        &self,
        control: Arc<dyn EntryRunControl>,
    ) -> Result<(), RegistryError> {
        *self
            .control
            .lock()
            .map_err(|_| RegistryError::Unavailable)? = Some(control);
        Ok(())
    }

    pub(super) fn control(&self) -> Result<Option<Arc<dyn EntryRunControl>>, RegistryError> {
        self.control
            .lock()
            .map(|control| control.clone())
            .map_err(|_| RegistryError::Unavailable)
    }

    pub(super) fn cancel(&self) -> io::Result<()> {
        let should_cancel = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| io::Error::other("command run state is unavailable"))?;
            if state.status == CommandRunState::Running {
                state.status = CommandRunState::Canceling;
                true
            } else {
                false
            }
        };
        if !should_cancel {
            return Ok(());
        }

        let result = self
            .control
            .lock()
            .map_err(|_| io::Error::other("command run control is unavailable"))
            .and_then(|control| {
                control
                    .clone()
                    .ok_or_else(|| io::Error::other("command run control is not attached"))
            })
            .and_then(|control| control.cancel());
        if let Err(error) = result {
            let rollback = self
                .state
                .lock()
                .map_err(|_| io::Error::other("command run state is unavailable"))
                .map(|mut state| {
                    if state.status == CommandRunState::Canceling {
                        state.status = CommandRunState::Running;
                    }
                });
            return match rollback {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(io::Error::other(format!(
                    "{error}; additionally, cancellation rollback failed: {rollback_error}"
                ))),
            };
        }
        Ok(())
    }

    pub(super) fn append(&self, stream: EntryOutputStream, text: String) {
        if text.is_empty() {
            return;
        }
        self.append_journal_event(|journal| {
            journal.output(
                RunJournalPhase::Worker,
                match stream {
                    EntryOutputStream::Stdout => RunJournalStream::Stdout,
                    EntryOutputStream::Stderr => RunJournalStream::Stderr,
                },
                text,
            )
        });
    }

    pub(super) fn append_progress(&self, progress: CommandProgress) {
        self.append_journal_event(|journal| {
            journal
                .progress(RunJournalPhase::Worker, progress)
                .map(Some)
        });
    }

    fn append_journal_event(
        &self,
        append: impl FnOnce(&RunJournal) -> io::Result<Option<RunJournalEvent>>,
    ) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.status.is_terminal() {
            return;
        }
        let event = if state.journal_error.is_none() {
            match append(&self.journal) {
                Ok(Some(event)) => event,
                Ok(None) => return,
                Err(error) => {
                    state.journal_error = Some(error.to_string());
                    return;
                }
            }
        } else {
            return;
        };
        state.next_cursor = event.sequence;
        state.output_bytes += event.retained_bytes();
        state.events.push_back(event);
        while state.output_bytes > MAX_OUTPUT_BYTES || state.events.len() > MAX_OUTPUT_EVENTS {
            let Some(removed) = state.events.pop_front() else {
                break;
            };
            state.output_bytes -= removed.retained_bytes();
            state.truncated = true;
        }
    }

    pub(super) fn complete(&self, outcome: EntryRunOutcome) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.status.is_terminal() {
            return false;
        }
        if state.status == CommandRunState::Canceling {
            match self.journal.finish_canceled() {
                Ok(()) => {
                    state.status = CommandRunState::Canceled;
                    state.exit_code = None;
                    state.error = None;
                }
                Err(error) => journal_failed(&mut state, error),
            }
        } else {
            match outcome {
                EntryRunOutcome::Exited(exit_code) => match self.journal.finish_exited(exit_code) {
                    Ok(()) => {
                        state.status = CommandRunState::Exited;
                        state.exit_code = Some(exit_code);
                        state.error = None;
                    }
                    Err(error) => journal_failed(&mut state, error),
                },
                EntryRunOutcome::Failed(error) => match self.journal.finish_failed(error.clone()) {
                    Ok(()) => {
                        state.status = CommandRunState::Failed;
                        state.exit_code = None;
                        state.error = Some(error);
                    }
                    Err(journal_error) => {
                        state.status = CommandRunState::Failed;
                        state.exit_code = None;
                        state.error = Some(format!(
                            "{error}; additionally, command journal completion failed: {journal_error}"
                        ));
                    }
                },
            }
        }
        true
    }

    pub(super) fn document(&self, after: u64) -> Result<CommandRunDocument, RegistryError> {
        let state = self.state.lock().map_err(|_| RegistryError::Unavailable)?;
        Ok(CommandRunDocument {
            protocol: COMMAND_RUN_PROTOCOL,
            id: self.id.clone(),
            address: self.address.clone(),
            state: state.status,
            exit_code: state.exit_code,
            error: state.error.clone(),
            next_cursor: state.next_cursor,
            events: state
                .events
                .iter()
                .filter(|event| event.sequence > after)
                .cloned()
                .collect(),
            truncated: state.truncated,
        })
    }

    pub(super) fn is_terminal(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.status.is_terminal())
            .unwrap_or(true)
    }
}

#[derive(Default)]
struct RunState {
    status: CommandRunState,
    exit_code: Option<i32>,
    error: Option<String>,
    next_cursor: u64,
    events: VecDeque<RunJournalEvent>,
    output_bytes: usize,
    truncated: bool,
    journal_error: Option<String>,
}

fn journal_failed(state: &mut RunState, error: io::Error) {
    let prior = state
        .journal_error
        .take()
        .map(|prior| format!("{prior}; completion: {error}"))
        .unwrap_or_else(|| error.to_string());
    state.status = CommandRunState::Failed;
    state.exit_code = None;
    state.error = Some(format!("command journal failed: {prior}"));
}

pub(super) struct RunObserver {
    pub(super) run: Weak<CommandRun>,
    pub(super) registry: Weak<RegistryInner>,
}

impl EntryRunObserver for RunObserver {
    fn output(&self, stream: EntryOutputStream, text: String) {
        if let Some(run) = self.run.upgrade() {
            run.append(stream, text);
        }
    }

    fn progress(&self, progress: CommandProgress) {
        if let Some(run) = self.run.upgrade() {
            run.append_progress(progress);
        }
    }

    fn completed(&self, outcome: EntryRunOutcome) {
        if self.run.upgrade().is_some_and(|run| run.complete(outcome))
            && let Some(registry) = self.registry.upgrade()
        {
            registry.completed();
        }
    }
}
