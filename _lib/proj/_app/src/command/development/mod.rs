use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::development::BUN;

use super::{CommandError, CommandExecutionContext, CommandResult};
use filesystem::{child_file, directory_chain, is_lower_hex, read_json, verify_regular_file};

mod filesystem;

const STATE_SCHEMA: &str = "swawkit.command-provider-state/v1";
const PRODUCER_CONTRACT: &str = "swawkit.proj.dev-setup/v2";
const INSTALL_SCHEMA: &str = "swawkit.proj-dev.install.v0";
const MAX_STATE_BYTES: u64 = 16 * 1024;
const MAX_METADATA_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProviderState {
    schema: String,
    status: String,
    input_revision: String,
    token: String,
    producer_contract: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BunSelection {
    schema: String,
    selector: String,
    version: String,
    source_sha256: String,
    source_verification: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallMetadata {
    schema: String,
    name: String,
    version: String,
    source_url: String,
    source_sha256: String,
    source_verification: String,
    recipe_version: String,
    definition_signature: String,
    files: Vec<InstalledFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstalledFile {
    path: String,
    length: u64,
    sha256: String,
}

struct ResolvedVersion {
    version: String,
    source_sha256: Option<String>,
    source_verification: Option<String>,
    project_sha256: String,
}

pub(crate) fn resolve_entry_bun(context: &CommandExecutionContext) -> CommandResult<PathBuf> {
    let declaration = &context.profile.development.bun;
    if declaration.mode != "managed" {
        return Err(CommandError::new(format!(
            "Action run.ts requires managed Entry Bun. Set Bun mode to 'managed', then run '{} \
             .dev.setup'",
            context.entry_name
        )));
    }
    if declaration.version != "latest" && !BUN.accepts_exact_version(&declaration.version) {
        return Err(repair_error(
            context,
            "the Entry Bun version is not a supported Bun version",
        ));
    }

    let provider_root = directory_chain(
        &context.data_root,
        &["modules", "kernel", ".dev", "setup"],
        "development environment provider",
    )
    .map_err(|error| {
        repair_with_cause(context, "the development environment is unavailable", error)
    })?;
    let state_path = provider_root.join("_state.json");
    let initial_state = read_ready_state(context, &state_path)?;
    let bun_root = directory_chain(
        &provider_root,
        &["export", BUN.name],
        "development environment export",
    )
    .map_err(|error| {
        repair_with_cause(
            context,
            "the development environment Export is unavailable",
            error,
        )
    })?;
    let resolved = resolve_version(context, &bun_root, &declaration.version)?;
    let install_root = directory_chain(
        &bun_root,
        &["installs", &resolved.version],
        "Entry Bun installation",
    )
    .map_err(|error| {
        repair_with_cause(context, "the Entry Bun installation is unavailable", error)
    })?;
    let executable = validate_installation(context, &install_root, &resolved)?;

    let final_state = read_ready_state(context, &state_path)?;
    if final_state != initial_state {
        return Err(repair_error(
            context,
            "the development environment changed while resolving Entry Bun",
        ));
    }
    Ok(executable)
}

fn read_ready_state(
    context: &CommandExecutionContext,
    path: &Path,
) -> CommandResult<ProviderState> {
    let state: ProviderState = read_json(
        path,
        "development environment provider state",
        MAX_STATE_BYTES,
    )
    .map_err(|_| {
        repair_error(
            context,
            "the development environment state is missing or invalid",
        )
    })?;
    let valid = state.schema == STATE_SCHEMA
        && state.status == "ready"
        && state.input_revision == context.environment_input_revision
        && is_lower_hex(&state.token, 32)
        && state.producer_contract.as_deref() == Some(PRODUCER_CONTRACT);
    if !valid {
        return Err(repair_error(
            context,
            "the development environment is not ready for the current Entry Profile",
        ));
    }
    Ok(state)
}

fn resolve_version(
    context: &CommandExecutionContext,
    bun_root: &Path,
    requested: &str,
) -> CommandResult<ResolvedVersion> {
    let project_sha256 = context.profile.development.bun.sha256.to_ascii_lowercase();
    if requested != "latest" {
        return Ok(ResolvedVersion {
            version: requested.to_owned(),
            source_sha256: (!project_sha256.is_empty()).then(|| project_sha256.clone()),
            source_verification: (!project_sha256.is_empty()).then(|| "project".to_owned()),
            project_sha256,
        });
    }
    if !project_sha256.is_empty() {
        return Err(repair_error(
            context,
            "Bun latest cannot be combined with a project SHA-256",
        ));
    }
    let path = bun_root.join(".swawkit-dev-selection.json");
    let selection: BunSelection = read_json(&path, "Entry Bun selection", MAX_STATE_BYTES)
        .map_err(|_| repair_error(context, "the Bun latest selection is missing or invalid"))?;
    let valid = selection.schema == BUN.selection_schema
        && selection.selector == "latest"
        && crate::development::is_semantic_version(&selection.version)
        && is_lower_hex(&selection.source_sha256, 64)
        && matches!(
            selection.source_verification.as_str(),
            "github" | "unverified"
        );
    if !valid {
        return Err(repair_error(context, "the Bun latest selection is invalid"));
    }
    Ok(ResolvedVersion {
        version: selection.version,
        source_sha256: Some(selection.source_sha256),
        source_verification: Some(selection.source_verification),
        project_sha256,
    })
}

fn validate_installation(
    context: &CommandExecutionContext,
    install_root: &Path,
    resolved: &ResolvedVersion,
) -> CommandResult<PathBuf> {
    let metadata: InstallMetadata = read_json(
        &install_root.join(".swawkit-dev-install.json"),
        "Entry Bun installation metadata",
        MAX_METADATA_BYTES,
    )
    .map_err(|_| {
        repair_error(
            context,
            "the Entry Bun installation metadata is missing or invalid",
        )
    })?;
    let expected_signature = BUN.definition_signature(&resolved.version, &resolved.project_sha256);
    let verification_valid = resolved.source_verification.as_ref().map_or_else(
        || {
            matches!(
                metadata.source_verification.as_str(),
                "github" | "unverified"
            )
        },
        |expected| expected == &metadata.source_verification,
    );
    let metadata_valid = metadata.schema == INSTALL_SCHEMA
        && metadata.name == BUN.name
        && metadata.version == resolved.version
        && !metadata.source_url.is_empty()
        && metadata.source_url.trim() == metadata.source_url
        && is_lower_hex(&metadata.source_sha256, 64)
        && verification_valid
        && metadata.recipe_version == BUN.recipe_version
        && metadata.definition_signature == expected_signature
        && resolved
            .source_sha256
            .as_ref()
            .is_none_or(|expected| expected == &metadata.source_sha256)
        && metadata.files.len() == BUN.required_paths.len();
    if !metadata_valid {
        return Err(repair_error(
            context,
            "the Entry Bun installation metadata is stale",
        ));
    }

    let records: BTreeMap<_, _> = metadata
        .files
        .iter()
        .map(|record| (record.path.as_str(), record))
        .collect();
    if records.len() != metadata.files.len() {
        return Err(repair_error(
            context,
            "the Entry Bun installation has duplicate file records",
        ));
    }
    for relative in BUN.required_paths {
        let record = records.get(relative).ok_or_else(|| {
            repair_error(
                context,
                "the Entry Bun installation is missing a required file record",
            )
        })?;
        if record.length == 0 || !is_lower_hex(&record.sha256, 64) {
            return Err(repair_error(
                context,
                "the Entry Bun installation has an invalid file record",
            ));
        }
        let path = child_file(install_root, relative, "Entry Bun installed file")?;
        verify_regular_file(
            &path,
            "Entry Bun installed file",
            record.length,
            &record.sha256,
        )
        .map_err(|error| {
            repair_with_cause(context, "the Entry Bun installation is invalid", error)
        })?;
    }
    Ok(install_root.join(BUN.executable))
}

fn repair_error(context: &CommandExecutionContext, reason: &str) -> CommandError {
    CommandError::new(format!(
        "{reason}. Run '{} .dev.setup' to publish the current Entry development environment",
        context.entry_name
    ))
}

fn repair_with_cause(
    context: &CommandExecutionContext,
    reason: &str,
    cause: CommandError,
) -> CommandError {
    CommandError::new(format!(
        "{reason}: {cause}. Run '{} .dev.setup' to publish the current Entry development \
         environment",
        context.entry_name
    ))
}
