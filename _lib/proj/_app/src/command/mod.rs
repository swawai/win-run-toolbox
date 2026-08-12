mod development;
mod environment;
mod execute;
mod guard;
mod invocation;
mod process;
mod resolve;

#[cfg(test)]
pub(crate) use development::resolve_entry_bun;
pub(crate) use development::resolve_entry_development;
pub use environment::{CommandExecutionContext, CommandProcessMode};
pub(crate) use environment::{
    ExecutionPhase, ProcessEnvironment, catalog_command_data_root, command_data_root,
};
pub use execute::CommandExecutor;
pub(crate) use guard::{GuardPlan, GuardScope};
pub(crate) use invocation::Invocation;
pub(crate) use resolve::ResolvedCommand;

use std::error::Error;
use std::fmt;

pub type CommandResult<T> = Result<T, CommandError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    message: String,
}

impl CommandError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CommandError {}

#[cfg(test)]
mod development_tests;
#[cfg(test)]
mod tests;
