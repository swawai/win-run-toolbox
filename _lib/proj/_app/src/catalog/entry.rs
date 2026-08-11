use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::filesystem::{FileCandidate, directory_files};

const ENTRY_PROTOCOL: [(&str, CommandAdapter); 6] = [
    ("run.core.json", CommandAdapter::Core),
    ("run.exe", CommandAdapter::Exe),
    ("run.ts", CommandAdapter::Bun),
    ("run.py", CommandAdapter::Python),
    ("run.ps1", CommandAdapter::PowerShell),
    ("run.cmd", CommandAdapter::Cmd),
];
const CORE_HANDLERS: [&str; 5] = [
    "entry.claim",
    "entry.profile",
    "entry.profile.apply",
    "entry.profile.set",
    "host.start",
];

#[derive(Debug)]
pub(crate) struct ResolvedEntry {
    pub(crate) name: &'static str,
    pub(crate) adapter: CommandAdapter,
    pub(crate) path: PathBuf,
    pub(crate) handler: Option<String>,
}

pub(crate) fn resolve_entry(directory: &Path) -> io::Result<Option<ResolvedEntry>> {
    let files = directory_files(directory)?;
    let mut existing = Vec::new();

    for (canonical_name, adapter) in ENTRY_PROTOCOL {
        let matches: Vec<&FileCandidate> = files
            .iter()
            .filter(|file| file.name.eq_ignore_ascii_case(canonical_name))
            .collect();
        if matches.len() > 1 {
            return invalid_data(format!(
                "entry name collision in '{}': {}",
                directory.display(),
                matches
                    .iter()
                    .map(|file| file.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let Some(file) = matches.first() else {
            continue;
        };
        if file.name != canonical_name {
            return invalid_data(format!(
                "non-canonical entry name '{}' in '{}'; expected '{canonical_name}'",
                file.name,
                directory.display()
            ));
        }
        if file.reparse_point {
            return invalid_data(format!(
                "command entry cannot be a reparse point: {}",
                file.path.display()
            ));
        }
        let handler = (adapter == CommandAdapter::Core)
            .then(|| read_core_handler(&file.path))
            .transpose()?;
        existing.push(ResolvedEntry {
            name: canonical_name,
            adapter,
            path: file.path.clone(),
            handler,
        });
    }

    if existing.len() > 1 {
        return invalid_data(format!(
            "command directory '{}' contains multiple run entries: {}. Exactly one run.* is allowed",
            directory.display(),
            existing
                .iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(existing.pop())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandAdapter {
    Core,
    Exe,
    Bun,
    Python,
    PowerShell,
    Cmd,
}

impl CommandAdapter {
    pub(crate) fn from_name(value: &str) -> Option<Self> {
        match value {
            "core" => Some(Self::Core),
            "exe" => Some(Self::Exe),
            "bun" => Some(Self::Bun),
            "python" => Some(Self::Python),
            "powershell" => Some(Self::PowerShell),
            "cmd" => Some(Self::Cmd),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Exe => "exe",
            Self::Bun => "bun",
            Self::Python => "python",
            Self::PowerShell => "powershell",
            Self::Cmd => "cmd",
        }
    }

    pub(crate) fn is_bootstrap_safe(self) -> bool {
        matches!(self, Self::Exe | Self::PowerShell | Self::Cmd)
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CoreManifest {
    schema: String,
    handler: String,
}

fn read_core_handler(path: &Path) -> io::Result<String> {
    let content = fs::read_to_string(path)?;
    let manifest: CoreManifest = serde_json::from_str(&content).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "invalid Core command manifest '{}': {error}",
                path.display()
            ),
        )
    })?;
    if manifest.schema != "swawkit.core-command/v1" {
        return invalid_data(format!(
            "unsupported Core command schema '{}' in '{}'",
            manifest.schema,
            path.display()
        ));
    }
    if manifest.handler.is_empty() || manifest.handler.trim() != manifest.handler {
        return invalid_data(format!(
            "Core command handler must be a non-empty trimmed string in '{}'",
            path.display()
        ));
    }
    if !CORE_HANDLERS.contains(&manifest.handler.as_str()) {
        return invalid_data(format!(
            "unsupported Core command handler '{}' in '{}'",
            manifest.handler,
            path.display()
        ));
    }
    Ok(manifest.handler)
}

fn invalid_data<T>(message: String) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidData, message))
}
