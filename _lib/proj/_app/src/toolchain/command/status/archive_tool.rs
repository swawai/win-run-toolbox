use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;
use swawkit_proj::development::{ArchiveToolContract as ArchiveTool, is_semantic_version};

use super::CommandContext;
use super::filesystem::{
    MAX_METADATA_BYTES, MAX_STATE_BYTES, child_file, directory_chain, is_lower_hex,
    optional_directory_chain, read_json, regular_file_length, sha256_regular,
};

mod trust;

use trust::{Trust, trust};

const INSTALL_SCHEMA: &str = "swawkit.proj-dev.install.v0";

pub(super) enum ArchiveReport {
    Off,
    LatestUnresolved {
        repair: String,
    },
    Resolved {
        version_label: String,
        ready: bool,
        trust: Trust,
    },
}

impl ArchiveReport {
    pub(super) fn render(&self, tool: &ArchiveTool) {
        match self {
            Self::Off => println!("[OFF] {} is disabled.", tool.name),
            Self::LatestUnresolved { repair } => {
                println!("[MISSING] {} latest unresolved; run '{repair}'", tool.name,)
            }
            Self::Resolved {
                version_label,
                ready,
                trust,
                ..
            } => {
                let state = if *ready { "READY" } else { "MISSING" };
                println!(
                    "[{state}] {} {version_label}  {}  {}",
                    tool.name, trust.level, trust.message
                );
                if let Some(warning) = &trust.warning {
                    println!("WARNING: {warning}");
                }
            }
        }
    }
}

pub(super) fn inspect(
    context: &CommandContext,
    tool: &ArchiveTool,
) -> Result<ArchiveReport, String> {
    let mode = context.environment(tool.mode_variable).to_ascii_lowercase();
    if mode.is_empty() || mode == "disabled" {
        return Ok(ArchiveReport::Off);
    }
    if mode != "managed" {
        return Err(format!(
            "Unsupported {} value '{mode}'. Expected 'managed' or 'disabled'.",
            tool.mode_variable
        ));
    }

    let requested = context.environment(tool.version_variable);
    if requested.is_empty() {
        return Err(format!(
            "Enabled {} must declare {}.",
            tool.display_name, tool.version_variable
        ));
    }
    if requested != "latest" && !tool.accepts_exact_version(&requested) {
        return Err(format!(
            "Invalid {} version '{requested}'.",
            tool.display_name
        ));
    }
    let project_sha256 = normalize_project_hash(context, tool)?;
    if requested == "latest" && !project_sha256.is_empty() {
        return Err(format!(
            "{}=latest cannot be combined with {}.",
            tool.version_variable, tool.hash_variable
        ));
    }

    let resolved = if requested == "latest" {
        let Some(selection) = read_selection(context, tool)? else {
            return Ok(ArchiveReport::LatestUnresolved {
                repair: context.repair_invocation(),
            });
        };
        ResolvedDefinition {
            version: selection.version,
            verification: selection.source_verification,
            source_sha256: Some(selection.source_sha256),
        }
    } else {
        ResolvedDefinition {
            version: requested.clone(),
            verification: if project_sha256.is_empty() {
                "unresolved".to_owned()
            } else {
                "project".to_owned()
            },
            source_sha256: (!project_sha256.is_empty()).then(|| project_sha256.clone()),
        }
    };
    let metadata = valid_metadata(context, tool, &resolved, &project_sha256);
    let ready = metadata
        .as_ref()
        .is_some_and(|metadata| installation_hashes_match(context, tool, &resolved, metadata));
    let trust = trust(tool, &resolved, &project_sha256, metadata.as_ref());
    let version_label = if requested == "latest" {
        format!("latest -> {}", resolved.version)
    } else {
        resolved.version.clone()
    };
    Ok(ArchiveReport::Resolved {
        version_label,
        ready,
        trust,
    })
}

fn normalize_project_hash(context: &CommandContext, tool: &ArchiveTool) -> Result<String, String> {
    let mut value = context.environment(tool.hash_variable).to_ascii_lowercase();
    if let Some(hash) = value.strip_prefix("sha256:") {
        value = hash.to_owned();
    }
    if !value.is_empty() && !is_lower_hex(&value, 64) {
        return Err(format!(
            "{} must be empty or a 64-character SHA-256 value.",
            tool.hash_variable
        ));
    }
    Ok(value)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Selection {
    schema: String,
    selector: String,
    version: String,
    source_sha256: String,
    source_verification: String,
}

struct ResolvedDefinition {
    version: String,
    verification: String,
    source_sha256: Option<String>,
}

fn read_selection(
    context: &CommandContext,
    tool: &ArchiveTool,
) -> Result<Option<Selection>, String> {
    let Some(root) = optional_directory_chain(
        &context.data_root,
        &["modules", "kernel", ".dev", "setup", "export", tool.name],
        "tool export",
    )?
    else {
        return Ok(None);
    };
    let path = root.join(".swawkit-dev-selection.json");
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot inspect {} selection: {error}",
                tool.display_name
            ));
        }
        Ok(_) => {}
    }
    let selection: Selection = read_json(&path, "tool version selection", MAX_STATE_BYTES)?;
    if selection.schema != tool.selection_schema
        || selection.selector != "latest"
        || !is_semantic_version(&selection.version)
        || !is_lower_hex(&selection.source_sha256, 64)
        || !matches!(
            selection.source_verification.as_str(),
            "github" | "unverified"
        )
    {
        return Err(format!(
            "The {} version selection is invalid: {}",
            tool.display_name,
            path.display()
        ));
    }
    Ok(Some(selection))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallMetadata {
    schema: String,
    name: String,
    version: String,
    source_sha256: String,
    source_verification: String,
    recipe_version: String,
    definition_signature: String,
    files: Vec<InstalledFile>,
}

#[derive(Deserialize)]
struct InstalledFile {
    path: String,
    length: u64,
    sha256: String,
}

fn valid_metadata(
    context: &CommandContext,
    tool: &ArchiveTool,
    resolved: &ResolvedDefinition,
    project_sha256: &str,
) -> Option<InstallMetadata> {
    validate_metadata(context, tool, resolved, project_sha256).ok()
}

fn validate_metadata(
    context: &CommandContext,
    tool: &ArchiveTool,
    resolved: &ResolvedDefinition,
    project_sha256: &str,
) -> Result<InstallMetadata, String> {
    let root = install_root(context, tool, &resolved.version)?;
    let metadata: InstallMetadata = read_json(
        &root.join(".swawkit-dev-install.json"),
        "tool installation metadata",
        MAX_METADATA_BYTES,
    )?;
    if metadata.schema != INSTALL_SCHEMA
        || metadata.name != tool.name
        || metadata.version != resolved.version
        || !is_lower_hex(&metadata.source_sha256, 64)
        || resolved
            .source_sha256
            .as_ref()
            .is_some_and(|expected| expected != &metadata.source_sha256)
        || match resolved.verification.as_str() {
            "unresolved" => !matches!(
                metadata.source_verification.as_str(),
                "github" | "unverified"
            ),
            expected => metadata.source_verification != expected,
        }
        || (!project_sha256.is_empty() && metadata.source_sha256 != project_sha256)
        || metadata.recipe_version != tool.recipe_version
        || metadata.definition_signature
            != tool.definition_signature(&resolved.version, project_sha256)
        || metadata.files.len() != tool.required_paths.len()
    {
        return Err("tool installation metadata is stale".to_owned());
    }
    let records: BTreeMap<_, _> = metadata
        .files
        .iter()
        .map(|record| (record.path.as_str(), record))
        .collect();
    if records.len() != metadata.files.len() {
        return Err("tool installation metadata has duplicate paths".to_owned());
    }
    for relative in tool.required_paths {
        let record = records
            .get(relative)
            .ok_or("required file record is missing")?;
        if !is_lower_hex(&record.sha256, 64) {
            return Err("required file hash is invalid".to_owned());
        }
        let path = child_file(&root, relative, "installed file")?;
        if regular_file_length(&path, "installed file")? != record.length {
            return Err("required file length changed".to_owned());
        }
    }
    Ok(metadata)
}

fn installation_hashes_match(
    context: &CommandContext,
    tool: &ArchiveTool,
    resolved: &ResolvedDefinition,
    metadata: &InstallMetadata,
) -> bool {
    let Ok(root) = install_root(context, tool, &resolved.version) else {
        return false;
    };
    metadata.files.iter().all(|record| {
        child_file(&root, &record.path, "installed file")
            .and_then(|path| sha256_regular(&path, "installed file"))
            .is_ok_and(|actual| actual == record.sha256)
    })
}

fn install_root(
    context: &CommandContext,
    tool: &ArchiveTool,
    version: &str,
) -> Result<PathBuf, String> {
    directory_chain(
        &context.data_root,
        &[
            "modules", "kernel", ".dev", "setup", "export", tool.name, "installs", version,
        ],
        "tool installation",
    )
}
