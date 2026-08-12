use std::path::PathBuf;

use crate::development::BUN;
use crate::development::archive_tool::{
    ArchiveToolError, ArchiveToolErrorKind, ArchiveToolRequest, ArchiveToolStore,
};
use crate::development::setup::provider::{ReadyProviderState, read_ready};

use super::{CommandError, CommandExecutionContext, CommandResult};
pub(crate) fn resolve_entry_bun(context: &CommandExecutionContext) -> CommandResult<PathBuf> {
    let declaration = &context.profile.development.bun;
    if declaration.mode != "managed" {
        return Err(CommandError::new(format!(
            "Action run.ts requires managed Entry Bun. Set Bun mode to 'managed', then run '{} \
             .dev.setup'",
            context.entry_name
        )));
    }
    let request = ArchiveToolRequest::new(&BUN, &declaration.version, &declaration.sha256)
        .map_err(|error| map_request_error(context, error))?;

    let initial_state = read_ready_state(context)?;
    let store = ArchiveToolStore::new(&context.data_root, &BUN);
    store.require_export().map_err(|error| {
        repair_with_archive_cause(
            context,
            "the development environment Export is unavailable",
            error,
        )
    })?;
    let resolved = store
        .resolve(&request)
        .map_err(|error| map_selection_error(context, error))?
        .ok_or_else(|| repair_error(context, "the Bun latest selection is missing or invalid"))?;
    let installation = store
        .read_installation(&resolved)
        .map_err(|error| map_installation_error(context, error))?;
    store.verify_hashes(&installation).map_err(|error| {
        repair_with_archive_cause(context, "the Entry Bun installation is invalid", error)
    })?;
    let executable = installation.executable().to_path_buf();

    let final_state = read_ready_state(context)?;
    if final_state != initial_state {
        return Err(repair_error(
            context,
            "the development environment changed while resolving Entry Bun",
        ));
    }
    Ok(executable)
}

fn read_ready_state(context: &CommandExecutionContext) -> CommandResult<ReadyProviderState> {
    read_ready(&context.data_root, &context.environment_input_revision).map_err(|_| {
        repair_error(
            context,
            "the development environment is not ready for the current Entry Profile",
        )
    })
}

fn map_request_error(context: &CommandExecutionContext, error: ArchiveToolError) -> CommandError {
    match error.kind() {
        ArchiveToolErrorKind::InvalidVersion => repair_error(
            context,
            "the Entry Bun version is not a supported Bun version",
        ),
        ArchiveToolErrorKind::LatestWithProjectSha256 => repair_error(
            context,
            "Bun latest cannot be combined with a project SHA-256",
        ),
        _ => repair_with_archive_cause(context, "the Entry Bun declaration is invalid", error),
    }
}

fn map_selection_error(context: &CommandExecutionContext, error: ArchiveToolError) -> CommandError {
    match error.kind() {
        ArchiveToolErrorKind::SelectionInvalid => {
            repair_error(context, "the Bun latest selection is invalid")
        }
        _ => repair_error(context, "the Bun latest selection is missing or invalid"),
    }
}

fn map_installation_error(
    context: &CommandExecutionContext,
    error: ArchiveToolError,
) -> CommandError {
    match error.kind() {
        ArchiveToolErrorKind::InstallationUnavailable => {
            repair_with_archive_cause(context, "the Entry Bun installation is unavailable", error)
        }
        ArchiveToolErrorKind::MetadataUnreadable => repair_error(
            context,
            "the Entry Bun installation metadata is missing or invalid",
        ),
        ArchiveToolErrorKind::MetadataStale => {
            repair_error(context, "the Entry Bun installation metadata is stale")
        }
        ArchiveToolErrorKind::DuplicateFileRecords => repair_error(
            context,
            "the Entry Bun installation has duplicate file records",
        ),
        ArchiveToolErrorKind::MissingFileRecord => repair_error(
            context,
            "the Entry Bun installation is missing a required file record",
        ),
        ArchiveToolErrorKind::InvalidFileRecord => repair_error(
            context,
            "the Entry Bun installation has an invalid file record",
        ),
        _ => repair_with_archive_cause(context, "the Entry Bun installation is invalid", error),
    }
}

fn repair_error(context: &CommandExecutionContext, reason: &str) -> CommandError {
    CommandError::new(format!(
        "{reason}. Run '{} .dev.setup' to publish the current Entry development environment",
        context.entry_name
    ))
}

fn repair_with_archive_cause(
    context: &CommandExecutionContext,
    reason: &str,
    cause: ArchiveToolError,
) -> CommandError {
    CommandError::new(format!(
        "{reason}: {cause}. Run '{} .dev.setup' to publish the current Entry development \
         environment",
        context.entry_name
    ))
}
