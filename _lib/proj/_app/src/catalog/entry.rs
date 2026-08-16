use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{
    CommandSource,
    filesystem::{FileCandidate, directory_files},
};
use crate::profile::EntryProfileRecord;

const ENTRY_PROTOCOL: [(&str, CommandAdapter); 7] = [
    ("run.core.json", CommandAdapter::Core),
    ("run.toolchain.json", CommandAdapter::Toolchain),
    ("run.exe", CommandAdapter::Exe),
    ("run.ts", CommandAdapter::Bun),
    ("run.py", CommandAdapter::Python),
    ("run.ps1", CommandAdapter::Pwsh),
    ("run.cmd", CommandAdapter::Cmd),
];
const CORE_HANDLERS: [&str; 20] = [
    "context.add",
    "context.delete",
    "context.list",
    "context.new",
    "context.note",
    "context.prompt",
    "context.remove",
    "context.render",
    "context.show",
    "entry.claim",
    "entry.profile",
    "entry.profile.apply",
    "entry.profile.set",
    "host.exit",
    "host.restart",
    "meta.check",
    "meta.help",
    "meta.logs",
    "runtime.cleanup",
    "runtime.status",
];
const TOOLCHAIN_HANDLERS: [&str; 2] = ["dev.setup", "dev.status"];

#[derive(Debug)]
pub(crate) struct ResolvedEntry {
    pub(crate) name: &'static str,
    pub(crate) adapter: CommandAdapter,
    pub(crate) path: PathBuf,
    pub(crate) handler: Option<String>,
}

impl ResolvedEntry {
    pub(super) fn has_valid_core_owner(&self, source: CommandSource, address: &str) -> bool {
        match source {
            CommandSource::Control => !matches!(
                self.handler.as_deref(),
                Some("meta.check" | "meta.help" | "meta.logs")
            ),
            CommandSource::Kernel => {
                matches!(
                    (address, self.handler.as_deref()),
                    (".context.add", Some("context.add"))
                        | (".context.delete", Some("context.delete"))
                        | (".context.list", Some("context.list"))
                        | (".context.new", Some("context.new"))
                        | (".context.note", Some("context.note"))
                        | (".context.prompt", Some("context.prompt"))
                        | (".context.remove", Some("context.remove"))
                        | (".context.render", Some("context.render"))
                        | (".context.show", Some("context.show"))
                        | (".check", Some("meta.check"))
                        | (".help", Some("meta.help"))
                        | (".logs", Some("meta.logs"))
                ) || (self.handler.as_deref() == Some("entry.profile.set")
                    && EntryProfileRecord::is_profile_setting_address(address))
            }
            CommandSource::Action => false,
        }
    }
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
        let handler = match adapter {
            CommandAdapter::Core => Some(read_handler_manifest(
                &file.path,
                "swawkit.core-command/v1",
                "Core",
                &CORE_HANDLERS,
            )?),
            CommandAdapter::Toolchain => Some(read_handler_manifest(
                &file.path,
                "swawkit.toolchain-command/v1",
                "Toolchain",
                &TOOLCHAIN_HANDLERS,
            )?),
            _ => None,
        };
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
    Toolchain,
    Exe,
    Bun,
    Python,
    Pwsh,
    Cmd,
}

impl CommandAdapter {
    pub(crate) fn from_name(value: &str) -> Option<Self> {
        match value {
            "core" => Some(Self::Core),
            "toolchain" => Some(Self::Toolchain),
            "exe" => Some(Self::Exe),
            "bun" => Some(Self::Bun),
            "python" => Some(Self::Python),
            "pwsh" => Some(Self::Pwsh),
            "cmd" => Some(Self::Cmd),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Toolchain => "toolchain",
            Self::Exe => "exe",
            Self::Bun => "bun",
            Self::Python => "python",
            Self::Pwsh => "pwsh",
            Self::Cmd => "cmd",
        }
    }

    pub(crate) fn is_bootstrap_safe(self) -> bool {
        matches!(self, Self::Exe | Self::Cmd)
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HandlerManifest {
    schema: String,
    handler: String,
}

fn read_handler_manifest(
    path: &Path,
    expected_schema: &str,
    owner: &str,
    allowed_handlers: &[&str],
) -> io::Result<String> {
    let content = fs::read_to_string(path)?;
    let manifest: HandlerManifest = serde_json::from_str(&content).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "invalid {owner} command manifest '{}': {error}",
                path.display()
            ),
        )
    })?;
    if manifest.schema != expected_schema {
        return invalid_data(format!(
            "unsupported {owner} command schema '{}' in '{}'",
            manifest.schema,
            path.display()
        ));
    }
    if manifest.handler.is_empty() || manifest.handler.trim() != manifest.handler {
        return invalid_data(format!(
            "{owner} command handler must be a non-empty trimmed string in '{}'",
            path.display()
        ));
    }
    if !allowed_handlers.contains(&manifest.handler.as_str()) {
        return invalid_data(format!(
            "unsupported {owner} command handler '{}' in '{}'",
            manifest.handler,
            path.display()
        ));
    }
    Ok(manifest.handler)
}

fn invalid_data<T>(message: String) -> io::Result<T> {
    Err(io::Error::new(io::ErrorKind::InvalidData, message))
}
