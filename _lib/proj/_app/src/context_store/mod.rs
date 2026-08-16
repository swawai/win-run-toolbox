use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::catalog::{CatalogSnapshot, CommandSource};
use crate::data_root::DataRootLock;

mod render;
mod storage;
mod subject_projection;

pub use render::render_markdown;
pub use subject_projection::{
    CONTEXT_COLLECTION_FACET, CONTEXT_KIND, CONTEXT_OWNER_ADDRESS, project_context_collection,
};

use storage::{
    context_path, delete_record, ensure_context_directory, existing_context_directory,
    publish_new_record, publish_record, read_optional_record, resource_directories,
};

pub const CONTEXT_SCHEMA: &str = "swawkit.context/v1";
pub const MAX_CONTEXT_BYTES: usize = 128 * 1024;
pub const MAX_CONTEXT_COMMANDS: usize = 128;
pub const MAX_CONTEXT_NOTES: usize = 128;
pub const MAX_NOTE_BYTES: usize = 8 * 1024;
pub const MAX_PROMPT_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextCommand {
    pub source: CommandSource,
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRecord {
    pub schema: String,
    pub id: String,
    pub commands: Vec<ContextCommand>,
    pub notes: Vec<String>,
    pub prompt: String,
}

impl ContextRecord {
    fn empty(id: &str) -> Self {
        Self {
            schema: CONTEXT_SCHEMA.to_owned(),
            id: id.to_owned(),
            commands: Vec::new(),
            notes: Vec::new(),
            prompt: String::new(),
        }
    }

    fn validate(&self) -> ContextResult<()> {
        if self.schema != CONTEXT_SCHEMA {
            return Err(ContextStoreError::new(format!(
                "unsupported Context schema '{}'",
                self.schema
            )));
        }
        validate_id(&self.id)?;
        if self.commands.len() > MAX_CONTEXT_COMMANDS {
            return Err(ContextStoreError::new(format!(
                "a Context accepts at most {MAX_CONTEXT_COMMANDS} commands"
            )));
        }
        if self.notes.len() > MAX_CONTEXT_NOTES {
            return Err(ContextStoreError::new(format!(
                "a Context accepts at most {MAX_CONTEXT_NOTES} notes"
            )));
        }
        for command in &self.commands {
            validate_command_address(&command.address)?;
        }
        if has_duplicate_commands(&self.commands) {
            return Err(ContextStoreError::new(
                "a Context cannot contain duplicate commands",
            ));
        }
        for note in &self.notes {
            validate_text(note, "Context note", MAX_NOTE_BYTES)?;
        }
        if !self.prompt.is_empty() {
            validate_text(&self.prompt, "Context prompt", MAX_PROMPT_BYTES)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ContextStore {
    data_root: PathBuf,
    module_data_root: PathBuf,
    reserved_ids: BTreeSet<String>,
}

impl ContextStore {
    pub fn new(
        data_root: impl Into<PathBuf>,
        module_data_root: impl Into<PathBuf>,
        reserved_ids: BTreeSet<String>,
    ) -> Self {
        Self {
            data_root: data_root.into(),
            module_data_root: module_data_root.into(),
            reserved_ids,
        }
    }

    pub fn create(&self, id: &str) -> ContextResult<ContextRecord> {
        self.validate_resource_id(id)?;
        let _lock = self.acquire_lock()?;
        let directory = self.ensure_directory()?;
        let record = ContextRecord::empty(id);
        publish_new_record(&directory, &record)?;
        Ok(record)
    }

    pub fn read(&self, id: &str) -> ContextResult<ContextRecord> {
        self.read_optional(id)?.ok_or_else(|| not_found(id))
    }

    pub fn read_optional(&self, id: &str) -> ContextResult<Option<ContextRecord>> {
        self.validate_resource_id(id)?;
        let directory = existing_context_directory(&self.data_root, &self.module_data_root)?;
        let Some(directory) = directory else {
            return Ok(None);
        };
        read_optional_record(&directory, id)
    }

    pub fn list(&self) -> ContextResult<Vec<ContextRecord>> {
        let Some(directory) = existing_context_directory(&self.data_root, &self.module_data_root)?
        else {
            return Ok(Vec::new());
        };
        let mut records = Vec::new();
        for (id, path) in resource_directories(&directory, &self.reserved_ids)? {
            records.push(storage::read_record(&path.join("_resource.json"), &id)?);
        }
        Ok(records)
    }

    pub fn add_commands(
        &self,
        id: &str,
        commands: Vec<ContextCommand>,
    ) -> ContextResult<ContextRecord> {
        if commands.is_empty() {
            return Err(ContextStoreError::new("at least one command must be added"));
        }
        for command in &commands {
            validate_command_address(&command.address)?;
        }
        self.update(id, move |record| {
            for command in commands {
                if !record.commands.contains(&command) {
                    record.commands.push(command);
                }
            }
            Ok(())
        })
    }

    pub fn remove_commands(&self, id: &str, addresses: &[String]) -> ContextResult<ContextRecord> {
        if addresses.is_empty() {
            return Err(ContextStoreError::new(
                "at least one command must be removed",
            ));
        }
        for address in addresses {
            validate_command_address(address)?;
        }
        self.update(id, |record| {
            let before = record.commands.len();
            record
                .commands
                .retain(|command| !addresses.contains(&command.address));
            if record.commands.len() == before {
                return Err(ContextStoreError::new(format!(
                    "none of the requested commands belong to Context '{id}'"
                )));
            }
            Ok(())
        })
    }

    pub fn append_note(&self, id: &str, note: String) -> ContextResult<ContextRecord> {
        validate_text(&note, "Context note", MAX_NOTE_BYTES)?;
        self.update(id, move |record| {
            record.notes.push(note);
            Ok(())
        })
    }

    pub fn set_prompt(&self, id: &str, prompt: String) -> ContextResult<ContextRecord> {
        validate_text(&prompt, "Context prompt", MAX_PROMPT_BYTES)?;
        self.update(id, move |record| {
            record.prompt = prompt;
            Ok(())
        })
    }

    pub fn delete(&self, id: &str) -> ContextResult<()> {
        self.validate_resource_id(id)?;
        let _lock = self.acquire_lock()?;
        let directory = existing_context_directory(&self.data_root, &self.module_data_root)?
            .ok_or_else(|| not_found(id))?;
        delete_record(&directory, id)
    }

    fn update(
        &self,
        id: &str,
        change: impl FnOnce(&mut ContextRecord) -> ContextResult<()>,
    ) -> ContextResult<ContextRecord> {
        self.validate_resource_id(id)?;
        let _lock = self.acquire_lock()?;
        let directory = self.ensure_directory()?;
        let path = context_path(&directory, id);
        let mut record = storage::read_record(&path, id)?;
        change(&mut record)?;
        publish_record(&path, &record)?;
        Ok(record)
    }

    fn acquire_lock(&self) -> ContextResult<DataRootLock> {
        let data_directory = self.data_root.parent().ok_or_else(|| {
            ContextStoreError::new(format!(
                "Context DataRoot has no data directory: {}",
                self.data_root.display()
            ))
        })?;
        DataRootLock::acquire(data_directory)
            .map_err(|error| ContextStoreError::new(error.to_string()))
    }

    #[cfg(test)]
    fn directory(&self) -> PathBuf {
        storage::context_directory(&self.module_data_root)
    }

    fn ensure_directory(&self) -> ContextResult<PathBuf> {
        ensure_context_directory(&self.data_root, &self.module_data_root)
    }

    fn validate_resource_id(&self, id: &str) -> ContextResult<()> {
        validate_id(id)?;
        if self.reserved_ids.contains(id) {
            return Err(ContextStoreError::new(format!(
                "Context ID is reserved by a static .context subcommand: {id}"
            )));
        }
        Ok(())
    }
}

pub fn static_child_ids(snapshot: &CatalogSnapshot, owner: &str) -> BTreeSet<String> {
    snapshot
        .commands
        .iter()
        .filter(|command| {
            command.source == CommandSource::Kernel && command.parent.as_deref() == Some(owner)
        })
        .filter_map(|command| command.address.strip_prefix(&format!("{owner}.")))
        .filter(|id| validate_id(id).is_ok())
        .map(str::to_owned)
        .collect()
}

pub fn validate_id(id: &str) -> ContextResult<()> {
    let valid_length = !id.is_empty() && id.len() <= 64;
    let mut bytes = id.bytes();
    let valid_syntax = matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !valid_length || !valid_syntax || is_windows_device_name(id) {
        return Err(ContextStoreError::new(
            "Context ID must match [a-z][a-z0-9-]{0,63} and cannot be a Windows device name",
        ));
    }
    Ok(())
}

pub fn validate_text(value: &str, label: &str, max_bytes: usize) -> ContextResult<()> {
    if value.trim().is_empty() {
        return Err(ContextStoreError::new(format!("{label} cannot be empty")));
    }
    if value.contains('\0') {
        return Err(ContextStoreError::new(format!(
            "{label} cannot contain NUL characters"
        )));
    }
    if value.len() > max_bytes {
        return Err(ContextStoreError::new(format!(
            "{label} accepts at most {max_bytes} UTF-8 bytes"
        )));
    }
    Ok(())
}

fn validate_command_address(address: &str) -> ContextResult<()> {
    if address.is_empty() || address.len() > 4096 || address.contains('\0') {
        return Err(ContextStoreError::new(
            "Context command address must be non-empty, contain no NUL, and use at most 4096 UTF-8 bytes",
        ));
    }
    Ok(())
}

fn has_duplicate_commands(commands: &[ContextCommand]) -> bool {
    commands
        .iter()
        .enumerate()
        .any(|(index, command)| commands[..index].contains(command))
}

fn is_windows_device_name(id: &str) -> bool {
    matches!(
        id,
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

fn not_found(id: &str) -> ContextStoreError {
    ContextStoreError::new(format!("Context not found: {id}"))
}

pub type ContextResult<T> = Result<T, ContextStoreError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextStoreError {
    message: String,
}

impl ContextStoreError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ContextStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ContextStoreError {}

#[cfg(test)]
mod tests;
