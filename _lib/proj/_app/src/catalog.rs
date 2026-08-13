use crate::{context::EntryContext, profile::EntryProfile};
use serde::Serialize;
use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

mod address;
mod entry;
mod filesystem;
mod view;

use address::{child_address, parent_address};
pub(crate) use entry::{CommandAdapter, resolve_entry};
use filesystem::{
    FileCandidate, absolute_path, assert_command_root, child_directories, directory_files,
};
pub(crate) use filesystem::{NamedDirectory, named_directories};
use view::read_local_web_view;
pub use view::{ChildrenColumnView, ColumnWidth, CommandView};

pub const CATALOG_PROTOCOL: &str = "swawkit.command-catalog/v4";

pub const HELP_ADDRESS: &str = ".help";
pub const HELP_MARKERS: [&str; 4] = [HELP_ADDRESS, ".h", "-h", "--help"];

pub fn is_help_marker(value: &str) -> bool {
    HELP_MARKERS.contains(&value)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSnapshot {
    pub protocol: &'static str,
    pub entry_name: String,
    pub commands: Vec<CommandNode>,
}

impl CatalogSnapshot {
    pub fn discover(context: &EntryContext, profile: Option<&EntryProfile>) -> io::Result<Self> {
        let action_root = profile.map(|profile| profile.binding().action_root());
        let pwsh = match profile {
            Some(profile) if profile.record().development.pwsh.mode == "disabled" => {
                PwshAvailability::Disabled
            }
            Some(_) => PwshAvailability::Enabled,
            None => PwshAvailability::ProfileUnavailable,
        };
        Self::discover_optional_roots(
            &context.kernel_root(),
            action_root.as_deref(),
            &context.entry_name,
            pwsh,
        )
    }

    pub fn discover_roots(
        kernel_root: &Path,
        action_root: &Path,
        entry_name: &str,
    ) -> io::Result<Self> {
        Self::discover_optional_roots(
            kernel_root,
            Some(action_root),
            entry_name,
            PwshAvailability::Enabled,
        )
    }

    fn discover_optional_roots(
        kernel_root: &Path,
        action_root: Option<&Path>,
        entry_name: &str,
        pwsh: PwshAvailability,
    ) -> io::Result<Self> {
        assert_command_root(kernel_root)?;

        let mut pending = VecDeque::from([PendingDirectory {
            path: absolute_path(kernel_root)?,
            address: String::new(),
            source: CommandSource::Kernel,
            is_root: true,
        }]);

        if let Some(action_root) = action_root.filter(|path| path.is_dir()) {
            assert_command_root(action_root)?;
            pending.push_back(PendingDirectory {
                path: absolute_path(action_root)?,
                address: String::new(),
                source: CommandSource::Action,
                is_root: true,
            });
        }

        let mut commands = Vec::new();
        while let Some(current) = pending.pop_front() {
            if current.source == CommandSource::Kernel || !current.is_root {
                commands.push(scan_node(&current, entry_name, pwsh));
            }

            for child in child_directories(&current.path)? {
                let Some(child_command) = child_address(&current, &child.name) else {
                    continue;
                };
                pending.push_back(PendingDirectory {
                    path: child.path,
                    address: child_command.address,
                    source: child_command.source,
                    is_root: false,
                });
            }
        }

        commands.sort_by(|left, right| {
            left.source
                .cmp(&right.source)
                .then_with(|| left.address.cmp(&right.address))
        });

        Ok(Self {
            protocol: CATALOG_PROTOCOL,
            entry_name: entry_name.to_owned(),
            commands,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandNode {
    pub address: String,
    pub source: CommandSource,
    pub parent: Option<String>,
    pub alias_of: Option<String>,
    pub runnable: bool,
    pub entry: Option<String>,
    pub adapter: Option<String>,
    pub handler: Option<String>,
    pub help: Option<HelpDocument>,
    pub view: Option<CommandView>,
    pub diagnostic: Option<String>,
    /// Retains the Help protocol state without expanding the public Web API.
    #[serde(skip)]
    pub help_diagnostic: Option<String>,
    #[serde(skip)]
    pub directory: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CommandSource {
    Control,
    Kernel,
    Action,
}

#[derive(Debug, Clone, Serialize)]
pub struct HelpDocument {
    pub summary: String,
    pub text: String,
}

#[derive(Debug)]
struct PendingDirectory {
    path: PathBuf,
    address: String,
    source: CommandSource,
    is_root: bool,
}

#[derive(Debug)]
struct ChildCommand {
    address: String,
    source: CommandSource,
}

#[derive(Clone, Copy)]
enum PwshAvailability {
    Enabled,
    Disabled,
    ProfileUnavailable,
}

fn scan_node(pending: &PendingDirectory, entry_name: &str, pwsh: PwshAvailability) -> CommandNode {
    let mut diagnostics = Vec::new();
    let entry = match resolve_entry(&pending.path) {
        Ok(entry) => entry,
        Err(error) => {
            diagnostics.push(error.to_string());
            None
        }
    };
    let entry = match entry {
        Some(entry)
            if entry.adapter == CommandAdapter::Core
                && pending.source != CommandSource::Control =>
        {
            diagnostics.push(
                "run.core.json is restricted to Control Plane commands (addresses beginning with '..')"
                    .to_owned(),
            );
            None
        }
        Some(entry)
            if entry.adapter == CommandAdapter::Toolchain
                && pending.source != CommandSource::Kernel =>
        {
            diagnostics.push(
                "run.toolchain.json is restricted to Kernel commands (addresses beginning with '.')"
                    .to_owned(),
            );
            None
        }
        Some(entry)
            if entry.adapter == CommandAdapter::Bun && pending.source != CommandSource::Action =>
        {
            diagnostics.push(
                "run.ts is restricted to project Action commands; product-owned commands must use a Rust-native entry"
                    .to_owned(),
            );
            None
        }
        Some(entry) if entry.adapter == CommandAdapter::Python => {
            diagnostics.push(
                "run.py is not runnable until managed Python is owned and verified by .dev.setup"
                    .to_owned(),
            );
            None
        }
        Some(entry)
            if entry.adapter == CommandAdapter::Pwsh
                && matches!(pwsh, PwshAvailability::Disabled) =>
        {
            diagnostics.push(
                "run.ps1 is disabled by the current Entry Profile; set SWAWKIT_PROJ_PWSH_MODE to managed or system and run .dev.setup"
                    .to_owned(),
            );
            None
        }
        Some(entry)
            if entry.adapter == CommandAdapter::Pwsh
                && matches!(pwsh, PwshAvailability::ProfileUnavailable) =>
        {
            diagnostics.push(
                "run.ps1 requires a ready Entry Profile with PowerShell 7 enabled".to_owned(),
            );
            None
        }
        Some(entry)
            if !matches!(entry.adapter, CommandAdapter::Core)
                && pending.source == CommandSource::Control =>
        {
            diagnostics.push("Control Plane commands must use a run.core.json entry".to_owned());
            None
        }
        entry => entry,
    };
    let (help, help_diagnostic) = match read_local_help(&pending.path, entry_name, &pending.address)
    {
        Ok(help) => (help, None),
        Err(error) => {
            let diagnostic = error.to_string();
            diagnostics.push(diagnostic.clone());
            (None, Some(diagnostic))
        }
    };
    let view = match read_local_web_view(&pending.path) {
        Ok(view) => view,
        Err(error) => {
            diagnostics.push(error.to_string());
            None
        }
    };

    CommandNode {
        address: pending.address.clone(),
        source: pending.source,
        parent: parent_address(pending.source, &pending.address),
        alias_of: command_alias(pending.source, &pending.address).map(str::to_owned),
        runnable: entry.is_some(),
        entry: entry.as_ref().map(|entry| entry.name.to_owned()),
        adapter: entry
            .as_ref()
            .map(|entry| entry.adapter.as_str().to_owned()),
        handler: entry.and_then(|entry| entry.handler),
        help,
        view,
        diagnostic: (!diagnostics.is_empty()).then(|| diagnostics.join("; ")),
        help_diagnostic,
        directory: pending.path.clone(),
    }
}

fn command_alias(source: CommandSource, address: &str) -> Option<&'static str> {
    if source != CommandSource::Kernel {
        return None;
    }
    HELP_MARKERS
        .iter()
        .skip(1)
        .find_map(|alias| (*alias == address).then_some(HELP_ADDRESS))
}

fn read_local_help(
    command_directory: &Path,
    entry_name: &str,
    address: &str,
) -> io::Result<Option<HelpDocument>> {
    let directories = named_directories(command_directory, "_help")?;
    if directories.len() > 1 {
        return invalid_data(format!(
            "help directory name collision below '{}'",
            command_directory.display()
        ));
    }
    let Some(help_directory) = directories.first() else {
        return Ok(None);
    };
    if help_directory.name != "_help" {
        return invalid_data(format!(
            "non-canonical help directory '{}'; expected '_help'",
            help_directory.name
        ));
    }
    if help_directory.reparse_point {
        return invalid_data(format!(
            "help directory cannot be a reparse point: {}",
            help_directory.path.display()
        ));
    }

    let files: Vec<FileCandidate> = directory_files(&help_directory.path)?
        .into_iter()
        .filter(|file| file.name.eq_ignore_ascii_case("zh-CN.txt"))
        .collect();
    if files.len() > 1 {
        return invalid_data(format!(
            "help file name collision below '{}'",
            help_directory.path.display()
        ));
    }
    let Some(help_file) = files.first() else {
        return Ok(None);
    };
    if help_file.name != "zh-CN.txt" {
        return invalid_data(format!(
            "non-canonical help file '{}'; expected 'zh-CN.txt'",
            help_file.name
        ));
    }
    if help_file.reparse_point {
        return invalid_data(format!(
            "help file cannot be a reparse point: {}",
            help_file.path.display()
        ));
    }

    let text = fs::read_to_string(&help_file.path)?;
    let summary = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("help file is empty: {}", help_file.path.display()),
            )
        })?;
    let invocation = if address.is_empty() {
        entry_name.to_owned()
    } else {
        format!("{entry_name} {address}")
    };

    Ok(Some(HelpDocument {
        summary: expand_help(summary, entry_name, address, &invocation),
        text: expand_help(&text, entry_name, address, &invocation),
    }))
}

fn expand_help(text: &str, entry_name: &str, address: &str, invocation: &str) -> String {
    text.replace("{{COMMAND}}", entry_name)
        .replace("{{ADDRESS}}", address)
        .replace("{{INVOCATION}}", invocation)
}

fn invalid_data<T>(message: String) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidData, message))
}

#[cfg(test)]
mod tests;
