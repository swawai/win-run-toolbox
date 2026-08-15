use std::io::Write;
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::context::EntryContext;

pub const RUNTIME_CLEANUP_PROTOCOL: &str = "swawkit.runtime-cleanup/v1";
const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeCleanupAction {
    Preview,
    Apply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeCleanupState {
    Selected,
    InUse,
    Removable,
    Removed,
    Retained,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeCleanupItem {
    pub release_id: String,
    pub state: RuntimeCleanupState,
    pub pids: Vec<u32>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeCleanupSummary {
    pub selected: usize,
    pub in_use: usize,
    pub removable: usize,
    pub removed: usize,
    pub retained: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeCleanupDocument {
    pub protocol: String,
    pub action: RuntimeCleanupAction,
    pub items: Vec<RuntimeCleanupItem>,
    pub summary: RuntimeCleanupSummary,
}

impl RuntimeCleanupDocument {
    pub fn new(action: RuntimeCleanupAction, items: Vec<RuntimeCleanupItem>) -> Self {
        let mut summary = RuntimeCleanupSummary::default();
        for item in &items {
            match item.state {
                RuntimeCleanupState::Selected => summary.selected += 1,
                RuntimeCleanupState::InUse => summary.in_use += 1,
                RuntimeCleanupState::Removable => summary.removable += 1,
                RuntimeCleanupState::Removed => summary.removed += 1,
                RuntimeCleanupState::Retained => summary.retained += 1,
            }
        }
        Self {
            protocol: RUNTIME_CLEANUP_PROTOCOL.to_owned(),
            action,
            items,
            summary,
        }
    }

    pub fn render_text(&self) -> String {
        let mut output = format!(
            "Runtime Release cleanup {}",
            match self.action {
                RuntimeCleanupAction::Preview => "preview",
                RuntimeCleanupAction::Apply => "apply",
            }
        );
        for item in &self.items {
            output.push('\n');
            match item.state {
                RuntimeCleanupState::Selected => {
                    output.push_str(&format!("[SELECTED] {}", item.release_id));
                }
                RuntimeCleanupState::InUse => {
                    let pids = item
                        .pids
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(",");
                    output.push_str(&format!("[IN USE] {} PID {pids}", item.release_id));
                }
                RuntimeCleanupState::Removable => {
                    output.push_str(&format!("[REMOVABLE] {}", item.release_id));
                }
                RuntimeCleanupState::Removed => {
                    output.push_str(&format!("[REMOVED] {}", item.release_id));
                }
                RuntimeCleanupState::Retained => {
                    output.push_str(&format!(
                        "[RETAINED] {}: {}",
                        item.release_id,
                        item.reason.as_deref().unwrap_or("unknown reason")
                    ));
                }
            }
        }
        output.push_str(&format!(
            "\nSummary: selected={}, in-use={}, removable={}, removed={}, retained={}",
            self.summary.selected,
            self.summary.in_use,
            self.summary.removable,
            self.summary.removed,
            self.summary.retained,
        ));
        if self.action == RuntimeCleanupAction::Preview && self.summary.removable > 0 {
            output.push_str("\nRun again with --apply to delete the removable Releases.");
        }
        output
    }
}

pub fn execute_text(context: &EntryContext, apply: bool) -> Result<i32, String> {
    let mut command = cleanup_command(context, apply, "text")?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = command
        .status()
        .map_err(|error| format!("cannot start Runtime cleanup Toolchain: {error}"))?;
    Ok(status.code().unwrap_or(1))
}

pub fn execute_json(context: &EntryContext, apply: bool) -> Result<RuntimeCleanupDocument, String> {
    let mut command = cleanup_command(context, apply, "json")?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);
    let output = command
        .output()
        .map_err(|error| format!("cannot start Runtime cleanup Toolchain: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if error.is_empty() {
            format!(
                "Runtime cleanup Toolchain failed with exit code {}",
                output.status.code().unwrap_or(1)
            )
        } else {
            error
        });
    }
    if output.stdout.len() > MAX_DOCUMENT_BYTES {
        return Err("Runtime cleanup document exceeds the 4 MiB safety limit".to_owned());
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Runtime cleanup Toolchain returned invalid JSON: {error}"))
}

fn cleanup_command(context: &EntryContext, apply: bool, format: &str) -> Result<Command, String> {
    let executable = context.sibling_product_executable("swawkit-proj-toolchain.exe");
    validate_toolchain(&executable)?;
    let mut command = Command::new(executable);
    command.args([
        "runtime-cleanup-v1",
        context
            .swawkit_home
            .to_str()
            .ok_or_else(|| "Swaw Kit Home must be valid Unicode".to_owned())?,
        if apply { "apply" } else { "preview" },
        format,
    ]);
    Ok(command)
}

fn validate_toolchain(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        format!(
            "the Runtime Release Toolchain is unavailable at '{}': {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "the Runtime Release Toolchain is not a regular file: '{}'",
            path.display()
        ));
    }
    Ok(())
}

pub fn write_json(
    document: &RuntimeCleanupDocument,
    output: &mut impl Write,
) -> Result<(), String> {
    serde_json::to_writer(&mut *output, document)
        .map_err(|error| format!("cannot serialize Runtime cleanup document: {error}"))?;
    output
        .write_all(b"\n")
        .map_err(|error| format!("cannot write Runtime cleanup document: {error}"))
}
