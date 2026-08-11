use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use swawkit_proj::{
    catalog::{CatalogSnapshot, CommandNode, CommandSource, is_help_marker},
    command_journal::{CommandJournalAccess, CommandLocator},
    context::EntryContext,
    help::render_help,
    profile::{EntryProfileDocument, EntryProfileRecord, EntryProfileState, EntryProfileStore},
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
        _ => Ok(None),
    }
}

pub(super) fn dispatch(
    snapshot: &CatalogSnapshot,
    argv: &[OsString],
    context: &EntryContext,
    data_root: &Path,
    profile_state: &EntryProfileState,
    profile_store: &EntryProfileStore,
    host_launcher: &mut impl FnMut(&EntryContext) -> Result<i32, CliError>,
) -> Result<Option<i32>, CliError> {
    let Some(address) = argv.first() else {
        return Ok(None);
    };
    let address = address
        .to_str()
        .ok_or_else(|| CliError::new("command address is not valid Unicode"))?;
    if !address.starts_with("..") {
        return Ok(None);
    }

    let command = resolve_control(snapshot, address)?;
    let arguments = argv.get(1..).unwrap_or_default();
    let exit_code = match command.handler.as_deref() {
        Some("command.logs") => {
            show_command_logs(arguments, context, data_root, profile_state, snapshot)?
        }
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

fn show_command_logs(
    arguments: &[OsString],
    context: &EntryContext,
    data_root: &Path,
    profile_state: &EntryProfileState,
    catalog: &CatalogSnapshot,
) -> Result<i32, CliError> {
    let Some(locator) = arguments.first() else {
        return Err(command_logs_usage());
    };
    let locator = CommandLocator::parse(unicode_argument(locator, "command locator")?)
        .map_err(|error| CliError::new(error.to_string()))?;
    let journal =
        CommandJournalAccess::resolve(context, data_root, profile_state, catalog, locator)
            .map_err(|error| CliError::new(error.to_string()))?;

    match arguments.get(1..) {
        Some([]) => {
            let document = journal.history().map_err(journal_read_error)?;
            write_command_json(&document)?;
        }
        Some([option, id]) if option == "--run" => {
            let id = unicode_argument(id, "run id")?;
            let document = journal.run(id, 0).map_err(journal_read_error)?;
            write_command_json(&document)?;
        }
        Some([option, id, cursor_option, cursor])
            if option == "--run" && cursor_option == "--after" =>
        {
            let id = unicode_argument(id, "run id")?;
            let after = unicode_argument(cursor, "after cursor")?
                .parse::<u64>()
                .map_err(|_| CliError::new("after cursor must be an unsigned integer"))?;
            let document = journal.run(id, after).map_err(journal_read_error)?;
            write_command_json(&document)?;
        }
        Some([option, id]) if option == "--open" => {
            let id = unicode_argument(id, "run id")?;
            let path = journal
                .open_run_directory(id)
                .map_err(|error| CliError::new(format!("cannot open command journal: {error}")))?;
            write_output(&path.display().to_string())
                .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))?;
        }
        _ => return Err(command_logs_usage()),
    }
    Ok(0)
}

fn command_logs_usage() -> CliError {
    CliError::new(
        "usage: ..logs <source/address> [--run <run-id> [--after <cursor>] | --open <run-id>]",
    )
}

fn journal_read_error(error: std::io::Error) -> CliError {
    CliError::new(format!("cannot read command journal: {error}"))
}

fn write_command_json(document: &impl Serialize) -> Result<(), CliError> {
    let output = serde_json::to_string_pretty(document)
        .map_err(|error| CliError::new(format!("cannot serialize command journal: {error}")))?;
    write_output(&output)
        .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))
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
    let command_path = address.strip_prefix("..entry.env.").ok_or_else(|| {
        CliError::new(format!(
            "Catalog invariant failed for '{address}': Entry Profile setter address is invalid"
        ))
    })?;
    let mut segments = command_path.split('.');
    let (Some(group), Some(variable), None) = (segments.next(), segments.next(), segments.next())
    else {
        return Err(CliError::new(format!(
            "Catalog invariant failed for '{address}': Entry Profile setter address is invalid"
        )));
    };
    if !EntryProfileRecord::environment_variable_commands().contains(&(group, variable)) {
        return Err(CliError::new(format!(
            "Catalog invariant failed for '{address}': Entry Profile variable is in the wrong group"
        )));
    }
    let document = profile_store
        .update_environment_variable(variable, value)
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

fn write_json(document: &EntryProfileDocument) -> Result<(), CliError> {
    let output = serde_json::to_string_pretty(document)
        .map_err(|error| CliError::new(format!("cannot serialize entry profile: {error}")))?;
    write_output(&output)
        .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))
}
