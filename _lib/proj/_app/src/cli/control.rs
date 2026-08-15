use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use swawkit_proj::{
    catalog::{CatalogSnapshot, CommandNode, CommandSource, is_help_marker},
    context::EntryContext,
    help::render_help,
    profile::{EntryProfileDocument, EntryProfileRecord, EntryProfileStore},
    runtime_cleanup,
    runtime_control::{self, HostAction, RuntimeStatusDocument},
};

use super::{CliError, write_output};

pub(super) enum PreDataRootControl {
    Claim {
        snapshot: CatalogSnapshot,
        address: String,
    },
    Complete(i32),
}

pub(super) fn dispatch_before_data_root(
    context: &EntryContext,
    argv: &[OsString],
    host_launcher: &mut impl FnMut(&EntryContext) -> Result<i32, CliError>,
) -> Result<Option<PreDataRootControl>, CliError> {
    let Some(address) = argv.first() else {
        return Ok(None);
    };
    let address = address
        .to_str()
        .ok_or_else(|| CliError::new("command address is not valid Unicode"))?;
    if !address.starts_with("..") {
        return Ok(None);
    }

    let snapshot = CatalogSnapshot::discover(context, None)
        .map_err(|error| CliError::new(format!("catalog discovery failed: {error}")))?;
    let arguments = argv.get(1..).unwrap_or_default();
    if matches!(arguments, [marker] if marker.to_str().is_some_and(is_help_marker)) {
        control_node(&snapshot, address)?;
        let output =
            render_help(&snapshot, address).map_err(|error| CliError::new(error.to_string()))?;
        write_output(&output)
            .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))?;
        return Ok(Some(PreDataRootControl::Complete(0)));
    }
    let command = resolve_control(&snapshot, address)?;

    match command.handler.as_deref() {
        Some("entry.claim") => Ok(Some(PreDataRootControl::Claim {
            snapshot,
            address: address.to_owned(),
        })),
        Some("host.start") => Ok(Some(PreDataRootControl::Complete(start_host(
            arguments,
            context,
            host_launcher,
        )?))),
        Some("runtime.status") => Ok(Some(PreDataRootControl::Complete(show_runtime_status(
            arguments, context,
        )?))),
        Some("host.exit") => Ok(Some(PreDataRootControl::Complete(request_host_action(
            address,
            arguments,
            context,
            HostAction::Exit,
        )?))),
        Some("host.restart") => Ok(Some(PreDataRootControl::Complete(request_host_action(
            address,
            arguments,
            context,
            HostAction::Restart,
        )?))),
        Some("runtime.cleanup") => Ok(Some(PreDataRootControl::Complete(cleanup_runtime(
            address, arguments, context,
        )?))),
        _ => Ok(None),
    }
}

pub(super) fn dispatch(
    snapshot: &CatalogSnapshot,
    argv: &[OsString],
    context: &EntryContext,
    profile_store: &EntryProfileStore,
    host_launcher: &mut impl FnMut(&EntryContext) -> Result<i32, CliError>,
) -> Result<Option<i32>, CliError> {
    let Some(address) = argv.first() else {
        return Ok(None);
    };
    let address = address
        .to_str()
        .ok_or_else(|| CliError::new("command address is not valid Unicode"))?;
    let Some(command) = snapshot.commands.iter().find(|command| {
        command.address == address
            && command.adapter.as_deref() == Some("core")
            && (command.source == CommandSource::Control
                || command.handler.as_deref() == Some("entry.profile.set"))
    }) else {
        return Ok(None);
    };
    if !command.runnable {
        let reason = command
            .diagnostic
            .as_deref()
            .unwrap_or("the command has no recognized Core entry");
        return Err(CliError::new(format!(
            "command '{address}' is not runnable: {reason}"
        )));
    }
    let arguments = argv.get(1..).unwrap_or_default();
    let exit_code = match command.handler.as_deref() {
        Some("host.start") => start_host(arguments, context, host_launcher)?,
        Some("entry.profile") => show_profile(arguments, profile_store)?,
        Some("entry.profile.set") => set_profile(address, arguments, profile_store)?,
        Some("entry.profile.apply") => apply_profile(arguments, context, profile_store)?,
        Some(handler) => {
            return Err(CliError::new(format!(
                "unsupported Core command handler: {handler}"
            )));
        }
        None => {
            return Err(CliError::new(format!(
                "Catalog invariant failed for '{address}': Core command has no handler"
            )));
        }
    };
    Ok(Some(exit_code))
}

pub(super) fn resolve_control<'a>(
    snapshot: &'a CatalogSnapshot,
    address: &str,
) -> Result<&'a CommandNode, CliError> {
    let command = control_node(snapshot, address)?;
    if !command.runnable {
        let reason = command
            .diagnostic
            .as_deref()
            .unwrap_or("the command has no recognized Core entry");
        return Err(CliError::new(format!(
            "command '{address}' is not runnable: {reason}"
        )));
    }
    if command.adapter.as_deref() != Some("core") {
        return Err(CliError::new(format!(
            "Catalog invariant failed for '{address}': Control command is not a Core command"
        )));
    }
    Ok(command)
}

fn control_node<'a>(
    snapshot: &'a CatalogSnapshot,
    address: &str,
) -> Result<&'a CommandNode, CliError> {
    snapshot
        .commands
        .iter()
        .find(|node| node.source == CommandSource::Control && node.address == address)
        .ok_or_else(|| CliError::new(format!("command not found: {address}")))
}

fn start_host(
    arguments: &[OsString],
    context: &EntryContext,
    host_launcher: &mut impl FnMut(&EntryContext) -> Result<i32, CliError>,
) -> Result<i32, CliError> {
    require_no_arguments("..web", arguments)?;
    host_launcher(context)
}

fn show_runtime_status(arguments: &[OsString], context: &EntryContext) -> Result<i32, CliError> {
    let document = runtime_control::inspect(context).map_err(CliError::new)?;
    match arguments {
        [] => write_runtime_summary(&document)?,
        [format] if format == "--json" => {
            let output = serde_json::to_string_pretty(&document).map_err(|error| {
                CliError::new(format!("cannot serialize Runtime status: {error}"))
            })?;
            write_output(&output)
                .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))?;
        }
        _ => return Err(CliError::new("usage: ..runtime [--json]")),
    }
    Ok(0)
}

fn request_host_action(
    address: &str,
    arguments: &[OsString],
    context: &EntryContext,
    action: HostAction,
) -> Result<i32, CliError> {
    require_no_arguments(address, arguments)?;
    runtime_control::request_host_action(context, action).map_err(CliError::new)?;
    let message = match action {
        HostAction::Exit => "Entry Host exit accepted.",
        HostAction::Restart => "Entry Host restart accepted.",
    };
    write_output(message)
        .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))?;
    Ok(0)
}

fn cleanup_runtime(
    address: &str,
    arguments: &[OsString],
    context: &EntryContext,
) -> Result<i32, CliError> {
    let apply = match arguments {
        [] => false,
        [argument] if argument == "--apply" => true,
        _ => return Err(CliError::new(format!("usage: {address} [--apply]"))),
    };
    runtime_cleanup::execute_text(context, apply).map_err(CliError::new)
}

fn show_profile(
    arguments: &[OsString],
    profile_store: &EntryProfileStore,
) -> Result<i32, CliError> {
    let document = profile_store.document();
    match arguments {
        [] => write_profile_summary(&document)?,
        [format] if format == "--json" => write_json(&document)?,
        _ => {
            return Err(CliError::new("usage: ..entry [--json]"));
        }
    }
    Ok(0)
}

fn set_profile(
    address: &str,
    arguments: &[OsString],
    profile_store: &EntryProfileStore,
) -> Result<i32, CliError> {
    let [value] = arguments else {
        return Err(CliError::new(format!("usage: {address} <value>")));
    };
    let value = unicode_argument(value, "profile value")?.to_owned();
    if !EntryProfileRecord::is_profile_setting_address(address) {
        return Err(CliError::new(format!(
            "Catalog invariant failed for '{address}': Entry Profile setting address is invalid"
        )));
    }
    let document = profile_store
        .update_setting(address, value)
        .map_err(|error| CliError::new(error.to_string()))?;
    write_json(&document)?;
    Ok(0)
}

fn apply_profile(
    arguments: &[OsString],
    context: &EntryContext,
    profile_store: &EntryProfileStore,
) -> Result<i32, CliError> {
    let [option, path] = arguments else {
        return Err(CliError::new("usage: ..entry.apply --file <profile.json>"));
    };
    if option != "--file" {
        return Err(CliError::new("usage: ..entry.apply --file <profile.json>"));
    }
    let path = resolve_input_path(path, &context.invocation_directory);
    let content = fs::read_to_string(&path).map_err(|error| {
        CliError::new(format!(
            "cannot read entry profile input '{}': {error}",
            path.display()
        ))
    })?;
    let record: EntryProfileRecord = serde_json::from_str(&content).map_err(|error| {
        CliError::new(format!(
            "invalid entry profile JSON '{}': {error}",
            path.display()
        ))
    })?;
    let document = profile_store
        .replace(record)
        .map_err(|error| CliError::new(error.to_string()))?;
    write_json(&document)?;
    Ok(0)
}

fn resolve_input_path(value: &OsString, invocation_directory: &Path) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        invocation_directory.join(path)
    }
}

fn unicode_argument<'a>(value: &'a OsString, label: &str) -> Result<&'a str, CliError> {
    value
        .to_str()
        .ok_or_else(|| CliError::new(format!("{label} is not valid Unicode")))
}

fn require_no_arguments(address: &str, arguments: &[OsString]) -> Result<(), CliError> {
    if arguments.is_empty() {
        Ok(())
    } else {
        Err(CliError::new(format!(
            "{address} does not accept arguments"
        )))
    }
}

fn write_profile_summary(document: &EntryProfileDocument) -> Result<(), CliError> {
    let resolved = document
        .resolved_target_project_root
        .as_deref()
        .unwrap_or("not resolved");
    let mut output = format!(
        "Entry Profile\nStatus: {}\nFile: {}\nTarget: {}\nResolved: {}",
        document.status, document.path, document.profile.target_project_root, resolved
    );
    if let Some(error) = &document.error {
        output.push_str("\nError: ");
        output.push_str(error);
    }
    write_output(&output)
        .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))
}

fn write_runtime_summary(document: &RuntimeStatusDocument) -> Result<(), CliError> {
    let mut output = format!(
        "Runtime\nSelected Release: {}\nReleases: {}",
        document.selected_release_id, document.release_count
    );
    match &document.host {
        Some(host) => {
            output.push_str(&format!(
                "\nHost: online\nPID: {}\nRunning Release: {}\nUpdate: {}",
                host.pid,
                host.running_release_id,
                if host.update_available {
                    "new Release pending restart"
                } else {
                    "current"
                }
            ));
        }
        None => output.push_str("\nHost: offline"),
    }
    write_output(&output)
        .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))
}

fn write_json(document: &EntryProfileDocument) -> Result<(), CliError> {
    let output = serde_json::to_string_pretty(document)
        .map_err(|error| CliError::new(format!("cannot serialize entry profile: {error}")))?;
    write_output(&output)
        .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))
}
