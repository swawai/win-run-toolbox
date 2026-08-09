use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::{Arc, Mutex, Weak};

use crate::entry_runner::{
    EntryOutputStream, EntryRunControl, EntryRunObserver, EntryRunOutcome, EntryRunSpec,
    EntryRunner, NativeEntryRunner,
};

use super::{CommandRunDocument, CommandRunEvent, CommandRunState, CommandRunStream};

const MAX_ACTIVE_RUNS: usize = 4;
const MAX_TERMINAL_RUNS: usize = 32;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_OUTPUT_EVENTS: usize = 4096;

#[derive(Clone)]
pub(in crate::server) struct CommandRuns {
    inner: Arc<RegistryInner>,
}

impl CommandRuns {
    pub fn native() -> Self {
        Self::new(Arc::new(NativeEntryRunner))
    }

    pub fn new(runner: Arc<dyn EntryRunner>) -> Self {
        Self {
            inner: Arc::new(RegistryInner {
                runner,
                state: Mutex::new(RegistryState::default()),
            }),
        }
    }

    pub fn start(
        &self,
        address: String,
        mut spec: EntryRunSpec,
    ) -> Result<CommandRunDocument, RegistryError> {
        let run = {
            let mut state = self.inner.lock()?;
            prune_terminal(&mut state);
            if !state.accepting {
                return Err(RegistryError::ShuttingDown);
            }
            if state.active >= MAX_ACTIVE_RUNS {
                return Err(RegistryError::Capacity);
            }
            let id = state.next_id();
            spec.id = id.clone();
            let run = Arc::new(CommandRun::new(id.clone(), address));
            state.active += 1;
            state.order.push_back(id.clone());
            state.runs.insert(id, Arc::clone(&run));
            run
        };

        let observer: Arc<dyn EntryRunObserver> = Arc::new(RunObserver {
            run: Arc::downgrade(&run),
            registry: Arc::downgrade(&self.inner),
        });
        match self.inner.runner.start(spec, observer) {
            Ok(control) => {
                run.attach_control(control)?;
                Ok(run.document(0)?)
            }
            Err(error) => {
                self.inner.remove_failed_start(&run)?;
                Err(RegistryError::Start(error))
            }
        }
    }

    pub fn get(&self, id: &str, after: u64) -> Result<CommandRunDocument, RegistryError> {
        self.inner.run(id)?.document(after)
    }

    pub fn cancel(&self, id: &str) -> Result<(), RegistryError> {
        self.inner.run(id)?.cancel().map_err(RegistryError::Cancel)
    }

    pub fn shutdown(&self) -> Result<(), String> {
        let runs = {
            let mut state = self
                .inner
                .state
                .lock()
                .map_err(|_| "command run registry is unavailable".to_owned())?;
            state.accepting = false;
            state.runs.values().cloned().collect::<Vec<_>>()
        };

        let mut errors = Vec::new();
        for run in &runs {
            if let Err(error) = run.cancel() {
                errors.push(error.to_string());
            }
        }
        for run in runs {
            if let Some(control) = run
                .control()
                .map_err(|_| "command run registry is unavailable".to_owned())?
                && let Err(error) = control.join()
            {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

struct RegistryInner {
    runner: Arc<dyn EntryRunner>,
    state: Mutex<RegistryState>,
}

impl RegistryInner {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, RegistryState>, RegistryError> {
        self.state.lock().map_err(|_| RegistryError::Unavailable)
    }

    fn run(&self, id: &str) -> Result<Arc<CommandRun>, RegistryError> {
        self.lock()?
            .runs
            .get(id)
            .cloned()
            .ok_or(RegistryError::NotFound)
    }

    fn remove_failed_start(&self, run: &CommandRun) -> Result<(), RegistryError> {
        let mut state = self.lock()?;
        if state.runs.remove(&run.id).is_some() {
            state.order.retain(|id| id != &run.id);
            state.active = state.active.saturating_sub(1);
        }
        Ok(())
    }

    fn completed(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.active = state.active.saturating_sub(1);
        prune_terminal(&mut state);
    }
}

struct RegistryState {
    accepting: bool,
    next_sequence: u64,
    active: usize,
    order: VecDeque<String>,
    runs: HashMap<String, Arc<CommandRun>>,
}

impl Default for RegistryState {
    fn default() -> Self {
        Self {
            accepting: true,
            next_sequence: 0,
            active: 0,
            order: VecDeque::new(),
            runs: HashMap::new(),
        }
    }
}

impl RegistryState {
    fn next_id(&mut self) -> String {
        self.next_sequence += 1;
        format!("{:08x}{:016x}", std::process::id(), self.next_sequence)
    }
}

fn prune_terminal(state: &mut RegistryState) {
    let mut terminal = state.runs.values().filter(|run| run.is_terminal()).count();
    if terminal <= MAX_TERMINAL_RUNS {
        return;
    }
    let mut removed = Vec::new();
    for id in &state.order {
        if terminal <= MAX_TERMINAL_RUNS {
            break;
        }
        if state.runs.get(id).is_some_and(|run| run.is_terminal()) {
            removed.push(id.clone());
            terminal -= 1;
        }
    }
    for id in &removed {
        state.runs.remove(id);
    }
    state.order.retain(|id| !removed.contains(id));
}

struct CommandRun {
    id: String,
    address: String,
    state: Mutex<RunState>,
    control: Mutex<Option<Arc<dyn EntryRunControl>>>,
}

impl CommandRun {
    fn new(id: String, address: String) -> Self {
        Self {
            id,
            address,
            state: Mutex::new(RunState::default()),
            control: Mutex::new(None),
        }
    }

    fn attach_control(&self, control: Arc<dyn EntryRunControl>) -> Result<(), RegistryError> {
        *self
            .control
            .lock()
            .map_err(|_| RegistryError::Unavailable)? = Some(control);
        Ok(())
    }

    fn control(&self) -> Result<Option<Arc<dyn EntryRunControl>>, RegistryError> {
        self.control
            .lock()
            .map(|control| control.clone())
            .map_err(|_| RegistryError::Unavailable)
    }

    fn cancel(&self) -> io::Result<()> {
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

    fn append(&self, stream: EntryOutputStream, text: String) {
        if text.is_empty() {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.status.is_terminal() {
            return;
        }
        state.next_cursor += 1;
        state.output_bytes += text.len();
        let sequence = state.next_cursor;
        state.events.push_back(StoredEvent {
            sequence,
            stream: stream.into(),
            text,
        });
        while state.output_bytes > MAX_OUTPUT_BYTES || state.events.len() > MAX_OUTPUT_EVENTS {
            let Some(removed) = state.events.pop_front() else {
                break;
            };
            state.output_bytes -= removed.text.len();
            state.truncated = true;
        }
    }

    fn complete(&self, outcome: EntryRunOutcome) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.status.is_terminal() {
            return false;
        }
        if state.status == CommandRunState::Canceling {
            state.status = CommandRunState::Canceled;
            state.exit_code = None;
            state.error = None;
        } else {
            match outcome {
                EntryRunOutcome::Exited(exit_code) => {
                    state.status = CommandRunState::Exited;
                    state.exit_code = Some(exit_code);
                    state.error = None;
                }
                EntryRunOutcome::Failed(error) => {
                    state.status = CommandRunState::Failed;
                    state.exit_code = None;
                    state.error = Some(error);
                }
            }
        }
        true
    }

    fn document(&self, after: u64) -> Result<CommandRunDocument, RegistryError> {
        let state = self.state.lock().map_err(|_| RegistryError::Unavailable)?;
        Ok(CommandRunDocument {
            protocol: super::COMMAND_RUN_PROTOCOL,
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
                .map(Into::into)
                .collect(),
            truncated: state.truncated,
        })
    }

    fn is_terminal(&self) -> bool {
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
    events: VecDeque<StoredEvent>,
    output_bytes: usize,
    truncated: bool,
}

#[derive(Clone)]
struct StoredEvent {
    sequence: u64,
    stream: CommandRunStream,
    text: String,
}

impl From<StoredEvent> for CommandRunEvent {
    fn from(event: StoredEvent) -> Self {
        Self {
            sequence: event.sequence,
            stream: event.stream,
            text: event.text,
        }
    }
}

struct RunObserver {
    run: Weak<CommandRun>,
    registry: Weak<RegistryInner>,
}

impl EntryRunObserver for RunObserver {
    fn output(&self, stream: EntryOutputStream, text: String) {
        if let Some(run) = self.run.upgrade() {
            run.append(stream, text);
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

#[derive(Debug)]
pub(in crate::server) enum RegistryError {
    Cancel(io::Error),
    Capacity,
    NotFound,
    ShuttingDown,
    Start(io::Error),
    Unavailable,
}

#[cfg(test)]
mod tests;
