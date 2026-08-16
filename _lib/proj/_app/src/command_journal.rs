use std::error::Error;
use std::fmt;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::UI::{
    Shell::{SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW},
    WindowsAndMessaging::SW_SHOWNORMAL,
};

use crate::{
    catalog::{CatalogSnapshot, CommandSource},
    command::catalog_command_data_root,
    context::EntryContext,
    profile::EntryProfileState,
    run_journal::{read_run, read_run_directory, read_run_history},
};

pub use crate::run_journal::{RunJournalDocument, RunJournalHistoryDocument};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSubjectRecord {
    pub id: String,
    pub source: &'static str,
    pub state: &'static str,
    pub started_at_unix_ms: u64,
    pub event_count: u64,
}

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
        if source_for_cli_address(address) != Some(source) {
            return Err(CommandJournalAccessError::InvalidLocator(
                "the command locator source does not match the address namespace".to_owned(),
            ));
        }
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

    pub fn from_cli_target(
        catalog: &CatalogSnapshot,
        target: &str,
    ) -> Result<Self, CommandJournalAccessError> {
        if target.contains('/') {
            return Self::parse(target);
        }
        let source =
            source_for_cli_address(target).ok_or(CommandJournalAccessError::CommandNotFound)?;
        let command = catalog
            .commands
            .iter()
            .find(|command| command.source == source && command.address == target)
            .ok_or(CommandJournalAccessError::CommandNotFound)?;
        Ok(Self {
            source: command.source,
            address: command.address.clone(),
        })
    }
}

fn source_for_cli_address(address: &str) -> Option<CommandSource> {
    if address.starts_with("..") {
        None
    } else if address.starts_with('.') || address.starts_with('-') {
        Some(CommandSource::Kernel)
    } else {
        Some(CommandSource::Action)
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

    pub fn subject_runs(&self) -> io::Result<Vec<RunSubjectRecord>> {
        self.history().map(|history| {
            history
                .into_runs()
                .into_iter()
                .map(|run| RunSubjectRecord {
                    id: run.id,
                    source: match run.source {
                        crate::run_journal::RunJournalSource::Cli => "CLI",
                        crate::run_journal::RunJournalSource::Web => "Web",
                    },
                    state: match run.state {
                        crate::run_journal::RunJournalStatus::Running => "running",
                        crate::run_journal::RunJournalStatus::Exited => "exited",
                        crate::run_journal::RunJournalStatus::Canceled => "canceled",
                        crate::run_journal::RunJournalStatus::Failed => "failed",
                    },
                    started_at_unix_ms: run.started_at_unix_ms,
                    event_count: run.event_count,
                })
                .collect()
        })
    }

    pub fn run(&self, id: &str, after: u64) -> io::Result<RunJournalDocument> {
        read_run(&self.module_data_root, &self.address, id, after)
    }

    pub fn latest_run(&self, ordinal: usize) -> io::Result<RunJournalDocument> {
        self.latest_runs(ordinal, ordinal)?
            .pop()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "command journal not found"))
    }

    pub fn latest_runs(
        &self,
        start_ordinal: usize,
        end_ordinal: usize,
    ) -> io::Result<Vec<RunJournalDocument>> {
        let start_index = start_ordinal.checked_sub(1).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "latest ordinal must begin at one",
            )
        })?;
        if end_ordinal < start_ordinal {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "latest ordinal range is reversed",
            ));
        }
        let history = self.history()?;
        let mut ids = Vec::with_capacity(end_ordinal - start_ordinal + 1);
        for index in start_index..end_ordinal {
            ids.push(history.run_id_at(index).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "command journal not found")
            })?);
        }
        ids.into_iter().map(|id| self.run(&id, 0)).collect()
    }

    pub fn run_directory(&self, id: &str) -> io::Result<PathBuf> {
        read_run_directory(&self.module_data_root, &self.address, id)
    }

    pub fn open_run_directory(&self, id: &str) -> io::Result<PathBuf> {
        let path = self.run_directory(id)?;
        open_directory(&path)?;
        Ok(path)
    }
}

fn open_directory(path: &Path) -> io::Result<()> {
    let verb = "open".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut request = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: verb.as_ptr(),
        lpFile: path.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    if unsafe { ShellExecuteExW(&mut request) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
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
            "action/.dev.status",
            "kernel/proj.build",
            "kernel/..runtime",
        ] {
            assert!(CommandLocator::parse(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn cli_targets_infer_the_lexically_disjoint_command_namespace() {
        fn command(address: &str, source: CommandSource) -> crate::catalog::CommandNode {
            crate::catalog::CommandNode {
                address: address.to_owned(),
                source,
                parent: None,
                alias_of: None,
                runnable: true,
                entry: Some("run.exe".to_owned()),
                adapter: Some("exe".to_owned()),
                handler: None,
                module: None,
                help: None,
                subject_kinds: Vec::new(),
                facets: Vec::new(),
                view: None,
                diagnostic: None,
                help_diagnostic: None,
                directory: PathBuf::new(),
            }
        }

        let catalog = CatalogSnapshot {
            protocol: "fixture",
            entry_name: "swawkit".to_owned(),
            language: "en",
            commands: vec![
                command(".dev.status", CommandSource::Kernel),
                command("-literal", CommandSource::Kernel),
                command("proj.build", CommandSource::Action),
            ],
        };
        assert_eq!(
            CommandLocator::from_cli_target(&catalog, ".dev.status")
                .unwrap()
                .to_string(),
            "kernel/.dev.status"
        );
        assert_eq!(
            CommandLocator::from_cli_target(&catalog, "-literal")
                .unwrap()
                .to_string(),
            "kernel/-literal"
        );
        assert_eq!(
            CommandLocator::from_cli_target(&catalog, "proj.build")
                .unwrap()
                .to_string(),
            "action/proj.build"
        );
        assert_eq!(
            CommandLocator::from_cli_target(&catalog, "dev.status"),
            Err(CommandJournalAccessError::CommandNotFound),
            "omitting the Kernel dot must not redirect to a Kernel command"
        );
    }
}
