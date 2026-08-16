mod run;

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::{Arc, Mutex};

use crate::entry_runner::{
    EntryQueryOutput, EntryRunObserver, EntryRunOutcome, EntryRunSpec, EntryRunner,
    NativeEntryRunner, run_entry_query,
};
use crate::run_journal::{RunJournal, StartRunJournal};

use super::CommandRunDocument;
use run::{CommandRun, RunObserver};

#[cfg(test)]
use super::CommandRunState;
#[cfg(test)]
use run::MAX_OUTPUT_EVENTS;

const MAX_ACTIVE_RUNS: usize = 4;
const MAX_TERMINAL_RUNS: usize = 32;

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
        journal_request: StartRunJournal,
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
            let journal = RunJournal::start(journal_request).map_err(RegistryError::Journal)?;
            let id = journal.id().map_err(RegistryError::Journal)?;
            spec.id = id.clone();
            let run = Arc::new(CommandRun::new(id.clone(), address, journal));
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
                run.complete(EntryRunOutcome::Failed(format!(
                    "cannot start the Entry command: {error}"
                )));
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

    pub fn query(&self, spec: EntryRunSpec) -> Result<EntryQueryOutput, String> {
        let _slot = self.reserve_query()?;
        run_entry_query(Arc::clone(&self.inner.runner), spec)
    }

    fn reserve_query(&self) -> Result<QuerySlot, String> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| "command run registry is unavailable".to_owned())?;
        if !state.accepting {
            return Err("command run registry is shutting down".to_owned());
        }
        if state.active >= MAX_ACTIVE_RUNS {
            return Err("too many command runs are active".to_owned());
        }
        state.active += 1;
        Ok(QuerySlot {
            inner: Arc::clone(&self.inner),
        })
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

struct QuerySlot {
    inner: Arc<RegistryInner>,
}

impl Drop for QuerySlot {
    fn drop(&mut self) {
        self.inner.completed();
    }
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
    active: usize,
    order: VecDeque<String>,
    runs: HashMap<String, Arc<CommandRun>>,
}

impl Default for RegistryState {
    fn default() -> Self {
        Self {
            accepting: true,
            active: 0,
            order: VecDeque::new(),
            runs: HashMap::new(),
        }
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

#[derive(Debug)]
pub(in crate::server) enum RegistryError {
    Cancel(io::Error),
    Capacity,
    NotFound,
    ShuttingDown,
    Start(io::Error),
    Journal(io::Error),
    Unavailable,
}

#[cfg(test)]
mod tests;
