use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{
    catalog::{CatalogSnapshot, CommandSource},
    command::catalog_command_data_root,
    context::EntryContext,
    profile::EntryProfileState,
    run_journal::{read_run, read_run_directory, read_run_history},
};

pub use crate::run_journal::{RunJournalDocument, RunJournalHistoryDocument};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLocator {
    source: CommandSource,
    address: String,
}

impl CommandLocator {
    pub fn parse(value: impl Into<String>) -> Result<Self, CommandJournalAccessError> {
        let value = value.into();
        let (source, address) = value.split_once('/').ok_or_else(|| {
            CommandJournalAccessError::InvalidLocator(
                "the command locator must use the '<source>/<address>' form".to_owned(),
            )
        })?;
        if address.is_empty() || address.contains('/') {
            return Err(CommandJournalAccessError::InvalidLocator(
                "the command locator must contain one non-empty address".to_owned(),
            ));
        }
        let source = match source {
            "kernel" => CommandSource::Kernel,
            "action" => CommandSource::Action,
            _ => {
                return Err(CommandJournalAccessError::InvalidLocator(
                    "the command locator source must be either 'kernel' or 'action'".to_owned(),
                ));
            }
        };
        Ok(Self {
            source,
            address: address.to_owned(),
        })
    }

    pub fn source(&self) -> CommandSource {
        self.source
    }

    pub fn address(&self) -> &str {
        &self.address
    }
}

impl fmt::Display for CommandLocator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let source = match self.source {
            CommandSource::Kernel => "kernel",
            CommandSource::Action => "action",
            CommandSource::Control => "control",
        };
        write!(formatter, "{source}/{}", self.address)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandJournalAccessError {
    InvalidLocator(String),
    ProfileRequired,
    CommandNotFound,
    CatalogInvariant(String),
}

impl fmt::Display for CommandJournalAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLocator(message) | Self::CatalogInvariant(message) => {
                formatter.write_str(message)
            }
            Self::ProfileRequired => formatter
                .write_str("a ready Entry Profile is required to locate Action command journals"),
            Self::CommandNotFound => formatter.write_str("command not found"),
        }
    }
}

impl Error for CommandJournalAccessError {}

#[derive(Debug, Clone)]
pub struct CommandJournalAccess {
    address: String,
    module_data_root: PathBuf,
}

impl CommandJournalAccess {
    pub fn resolve(
        context: &EntryContext,
        data_root: &Path,
        profile_state: &EntryProfileState,
        catalog: &CatalogSnapshot,
        locator: CommandLocator,
    ) -> Result<Self, CommandJournalAccessError> {
        let binding = profile_state.ready().map(|profile| profile.binding());
        if locator.source == CommandSource::Action && binding.is_none() {
            return Err(CommandJournalAccessError::ProfileRequired);
        }
        let command = catalog
            .commands
            .iter()
            .find(|command| command.source == locator.source && command.address == locator.address)
            .ok_or(CommandJournalAccessError::CommandNotFound)?;
        let module_data_root = catalog_command_data_root(context, data_root, binding, command)
            .map_err(|error| CommandJournalAccessError::CatalogInvariant(error.to_string()))?;
        Ok(Self {
            address: locator.address,
            module_data_root,
        })
    }

    pub fn history(&self) -> io::Result<RunJournalHistoryDocument> {
        read_run_history(&self.module_data_root, &self.address)
    }

    pub fn run(&self, id: &str, after: u64) -> io::Result<RunJournalDocument> {
        read_run(&self.module_data_root, &self.address, id, after)
    }

    pub fn run_directory(&self, id: &str) -> io::Result<PathBuf> {
        read_run_directory(&self.module_data_root, &self.address, id)
    }

    pub fn open_run_directory(&self, id: &str) -> io::Result<PathBuf> {
        let path = self.run_directory(id)?;
        Command::new("explorer.exe").arg(&path).spawn()?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locator_uses_one_unambiguous_source_separator() {
        assert_eq!(
            CommandLocator::parse("kernel/.dev.status").unwrap(),
            CommandLocator {
                source: CommandSource::Kernel,
                address: ".dev.status".to_owned(),
            }
        );
        assert_eq!(
            CommandLocator::parse("action/proj.build")
                .unwrap()
                .to_string(),
            "action/proj.build"
        );
        for invalid in [
            "kernel..dev.status",
            "kernel/.dev/status",
            "control/..entry",
            "kernel/",
        ] {
            assert!(CommandLocator::parse(invalid).is_err(), "{invalid}");
        }
    }
}
