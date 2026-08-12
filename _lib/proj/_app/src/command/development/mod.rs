use std::path::PathBuf;

use crate::development::archive_tool::{
    ArchiveToolError, ArchiveToolErrorKind, ArchiveToolRequest, ArchiveToolStore,
};
use crate::development::msvc::{MsvcDefinition, MsvcStore};
use crate::development::rust::{RustDefinition, RustStore};
use crate::development::setup::environment::EnvironmentPlan;
use crate::development::setup::provider::{ReadyProviderState, read_ready};
use crate::development::{BUN, PWSH};
use crate::profile::VersionedTool;

use super::{CommandError, CommandExecutionContext, CommandResult};

pub(crate) struct ResolvedEntryDevelopment {
    pub(crate) bun_executable: PathBuf,
    pub(crate) environment: EnvironmentPlan,
}

pub(crate) fn resolve_entry_development(
    context: &CommandExecutionContext,
) -> CommandResult<ResolvedEntryDevelopment> {
    let bun_request = archive_request(context, &BUN, &context.profile.development.bun)?;
    let pwsh_request = (context.profile.development.pwsh.mode == "managed")
        .then(|| archive_request(context, &PWSH, &context.profile.development.pwsh))
        .transpose()?;
    let msvc_definition = (context.profile.development.msvc.mode == "managed")
        .then(|| MsvcDefinition::new(&context.profile.development.msvc.channel))
        .transpose()
        .map_err(|error| {
            repair_with_cause(context, "the managed MSVC declaration is invalid", error)
        })?;
    let rust_definition = (context.profile.development.rust.mode == "rustup")
        .then(|| {
            RustDefinition::new(
                &context.profile.development.rust.toolchain,
                &context.profile.development.rust.profile,
                &context.profile.development.rust.host,
            )
        })
        .transpose()
        .map_err(|error| {
            repair_with_cause(context, "the managed Rust declaration is invalid", error)
        })?;
    if rust_definition.is_some() && msvc_definition.is_none() {
        return Err(repair_error(
            context,
            "managed Rust requires the managed MSVC environment",
        ));
    }
    let initial_state = read_ready_state(context)?;
    let mut environment = EnvironmentPlan::default();
    let bun = resolve_archive_tool(context, &BUN, &bun_request)?;
    environment
        .prepend_path(bun.root())
        .map_err(CommandError::new)?;
    if let Some(request) = pwsh_request.as_ref() {
        let pwsh = resolve_archive_tool(context, &PWSH, request)?;
        environment
            .prepend_path(pwsh.root())
            .map_err(CommandError::new)?;
    }
    if let Some(definition) = msvc_definition.as_ref() {
        let installation = MsvcStore::new(&context.data_root, definition)
            .read_installation()
            .map_err(|error| {
                repair_with_cause(context, "the managed MSVC installation is invalid", error)
            })?;
        installation
            .add_environment(&mut environment)
            .map_err(|error| {
                repair_with_cause(context, "the managed MSVC environment is invalid", error)
            })?;
    }
    if let Some(definition) = rust_definition.as_ref() {
        let installation = RustStore::new(&context.data_root, definition)
            .read_installation()
            .map_err(|error| {
                repair_with_cause(context, "the managed Rust installation is invalid", error)
            })?;
        installation
            .add_environment(&definition, &mut environment)
            .map_err(|error| {
                repair_with_cause(context, "the managed Rust environment is invalid", error)
            })?;
    }
    let final_state = read_ready_state(context)?;
    if final_state != initial_state {
        return Err(repair_error(
            context,
            "the development environment changed while resolving the Action environment",
        ));
    }
    Ok(ResolvedEntryDevelopment {
        bun_executable: bun.executable().to_path_buf(),
        environment,
    })
}

fn resolve_archive_tool(
    context: &CommandExecutionContext,
    tool: &'static crate::development::ArchiveToolContract,
    request: &ArchiveToolRequest,
) -> CommandResult<crate::development::archive_tool::Installation> {
    let store = ArchiveToolStore::new(&context.data_root, tool);
    store.require_export().map_err(|error| {
        repair_with_archive_cause(
            context,
            "the development environment Export is unavailable",
            error,
        )
    })?;
    let resolved = store
        .resolve(request)
        .map_err(|error| {
            repair_with_archive_cause(
                context,
                &format!("the {} latest selection is invalid", tool.display_name),
                error,
            )
        })?
        .ok_or_else(|| {
            repair_error(
                context,
                &format!(
                    "the {} latest selection is missing or invalid",
                    tool.display_name
                ),
            )
        })?;
    let invalid_installation = format!("the Entry {} installation is invalid", tool.display_name);
    let installation = store
        .read_installation(&resolved)
        .map_err(|error| repair_with_archive_cause(context, &invalid_installation, error))?;
    store
        .verify_hashes(&installation)
        .map_err(|error| repair_with_archive_cause(context, &invalid_installation, error))?;
    Ok(installation)
}

fn archive_request(
    context: &CommandExecutionContext,
    tool: &'static crate::development::ArchiveToolContract,
    declaration: &VersionedTool,
) -> CommandResult<ArchiveToolRequest> {
    if declaration.mode != "managed" {
        return Err(repair_error(
            context,
            &format!("{} is not managed for this Entry", tool.display_name),
        ));
    }
    ArchiveToolRequest::new(tool, &declaration.version, &declaration.sha256).map_err(|error| {
        match error.kind() {
            ArchiveToolErrorKind::InvalidVersion => repair_error(
                context,
                &format!(
                    "the Entry {} version is not a supported {} version",
                    tool.display_name, tool.display_name
                ),
            ),
            ArchiveToolErrorKind::LatestWithProjectSha256 => repair_error(
                context,
                &format!(
                    "{} latest cannot be combined with a project SHA-256",
                    tool.display_name
                ),
            ),
            _ => repair_with_archive_cause(
                context,
                &format!("the Entry {} declaration is invalid", tool.display_name),
                error,
            ),
        }
    })
}

#[cfg(test)]
pub(crate) fn resolve_entry_bun(context: &CommandExecutionContext) -> CommandResult<PathBuf> {
    resolve_entry_development(context).map(|resolved| resolved.bun_executable)
}

fn read_ready_state(context: &CommandExecutionContext) -> CommandResult<ReadyProviderState> {
    read_ready(&context.data_root, &context.environment_input_revision).map_err(|_| {
        repair_error(
            context,
            "the development environment is not ready for the current Entry Profile",
        )
    })
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

fn repair_with_cause(
    context: &CommandExecutionContext,
    reason: &str,
    cause: impl std::fmt::Display,
) -> CommandError {
    CommandError::new(format!(
        "{reason}: {cause}. Run '{} .dev.setup' to publish the current Entry development environment",
        context.entry_name
    ))
}
