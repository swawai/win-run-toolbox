use std::path::Path;

use crate::catalog::{CommandAdapter, NamedDirectory, named_directories, resolve_entry};

use super::{CommandError, CommandResult, ResolvedCommand};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuardScope {
    Global,
    Command,
}

impl GuardScope {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Command => "command",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedGuard {
    pub scope: GuardScope,
    pub directory: std::path::PathBuf,
    pub entry_path: std::path::PathBuf,
    pub adapter: CommandAdapter,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GuardPlan {
    pub guards: Vec<ResolvedGuard>,
}

impl GuardPlan {
    pub(crate) fn discover(kernel_root: &Path, command: &ResolvedCommand) -> CommandResult<Self> {
        let mut guards = Vec::new();
        if let Some(guard) = resolve_optional(kernel_root, "_global", GuardScope::Global)? {
            guards.push(guard);
        }
        if let Some(guard) = resolve_optional(&command.directory, "_guard", GuardScope::Command)? {
            guards.push(guard);
        }
        Ok(Self { guards })
    }
}

fn resolve_optional(
    root: &Path,
    directory_name: &str,
    scope: GuardScope,
) -> CommandResult<Option<ResolvedGuard>> {
    let directories = named_directories(root, directory_name).map_err(|error| {
        CommandError::new(format!("cannot inspect {} guard: {error}", scope.as_str()))
    })?;
    if directories.len() > 1 {
        return Err(CommandError::new(format!(
            "{} guard directory name collision below '{}'",
            scope.as_str(),
            root.display()
        )));
    }
    let Some(directory) = directories.first() else {
        return Ok(None);
    };
    validate_directory(directory, directory_name, scope)?;
    let entry = resolve_entry(&directory.path)
        .map_err(|error| CommandError::new(format!("invalid {} guard: {error}", scope.as_str())))?;
    let Some(entry) = entry else {
        return Err(CommandError::new(format!(
            "the {} guard has no executable run.* entry: {}",
            scope.as_str(),
            directory.path.display()
        )));
    };
    if !entry.adapter.is_bootstrap_safe() {
        return Err(CommandError::new(format!(
            "the {} guard entry '{}' is not bootstrap-safe; V0 guards support run.exe, \
             run.ps1, or run.cmd",
            scope.as_str(),
            entry.name
        )));
    }
    Ok(Some(ResolvedGuard {
        scope,
        directory: directory.path.clone(),
        entry_path: entry.path,
        adapter: entry.adapter,
    }))
}

fn validate_directory(
    directory: &NamedDirectory,
    expected_name: &str,
    scope: GuardScope,
) -> CommandResult<()> {
    if directory.name != expected_name {
        return Err(CommandError::new(format!(
            "non-canonical {} guard directory '{}'; expected '{expected_name}'",
            scope.as_str(),
            directory.name
        )));
    }
    if directory.reparse_point {
        return Err(CommandError::new(format!(
            "{} guard directory cannot be a reparse point: {}",
            scope.as_str(),
            directory.path.display()
        )));
    }
    Ok(())
}
