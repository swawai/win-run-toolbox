mod claim;
mod control;
mod logs;

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Write};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use swawkit_proj::{
    catalog::{CatalogSnapshot, is_help_marker},
    command::{CommandExecutionContext, CommandExecutor, CommandProcessMode},
    context::EntryContext,
    data_root::{DataRootClaimApprover, ResolveDataRootRequest, resolve_data_root},
    help::render_help,
    profile::{EntryProfileState, EntryProfileStore},
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

pub fn run(
    context: &EntryContext,
    argv: &[OsString],
    process_mode: CommandProcessMode,
) -> Result<i32, CliError> {
    let mut approver =
        |pending: &swawkit_proj::data_root::DataRootClaim| Err(claim::rejection(context, pending));
    let mut host_launcher = launch_entry_host;
    run_with_dependencies(
        context,
        argv,
        process_mode,
        &mut approver,
        &mut host_launcher,
    )
}

#[cfg(test)]
fn run_with_approver(
    context: &EntryContext,
    argv: &[OsString],
    approver: &mut impl DataRootClaimApprover,
) -> Result<i32, CliError> {
    let mut host_launcher = launch_entry_host;
    run_with_dependencies(
        context,
        argv,
        CommandProcessMode::InheritConsole,
        approver,
        &mut host_launcher,
    )
}

#[cfg(test)]
fn run_with_host_launcher(
    context: &EntryContext,
    argv: &[OsString],
    approver: &mut impl DataRootClaimApprover,
    host_launcher: &mut impl FnMut(&EntryContext) -> Result<i32, CliError>,
) -> Result<i32, CliError> {
    run_with_dependencies(
        context,
        argv,
        CommandProcessMode::InheritConsole,
        approver,
        host_launcher,
    )
}

fn run_with_dependencies(
    context: &EntryContext,
    argv: &[OsString],
    process_mode: CommandProcessMode,
    approver: &mut impl DataRootClaimApprover,
    host_launcher: &mut impl FnMut(&EntryContext) -> Result<i32, CliError>,
) -> Result<i32, CliError> {
    match control::dispatch_before_data_root(context, argv, host_launcher)? {
        Some(control::PreDataRootControl::Claim { snapshot, address }) => {
            return claim::run(context, argv, &snapshot, &address);
        }
        Some(control::PreDataRootControl::Complete(exit_code)) => return Ok(exit_code),
        None => {}
    }

    let resolved = resolve_data_root(
        ResolveDataRootRequest {
            swawkit_home: &context.swawkit_home,
            entry_file: &context.entry_file,
        },
        approver,
    )
    .map_err(|error| CliError::new(format!("DataRoot resolution failed: {error}")))?;

    let profile_store = EntryProfileStore::new(&context.swawkit_home, resolved.path());
    let profile_state = profile_store.read();
    let snapshot = CatalogSnapshot::discover(context, profile_state.ready())
        .map_err(|error| CliError::new(format!("catalog discovery failed: {error}")))?;
    if let Some(output) = protocol_help(&snapshot, argv)? {
        write_output(&output)
            .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))?;
        return Ok(0);
    }
    if let Some(exit_code) =
        logs::dispatch(&snapshot, argv, context, resolved.path(), &profile_state)?
    {
        return Ok(exit_code);
    }
    if let Some(exit_code) =
        control::dispatch(&snapshot, argv, context, &profile_store, host_launcher)?
    {
        return Ok(exit_code);
    }
    CommandExecutor::preflight(&context.kernel_root(), &snapshot, argv)
        .map_err(|error| CliError::new(error.to_string()))?;
    let profile = match profile_state {
        EntryProfileState::Ready(profile) => profile,
        EntryProfileState::Missing { path } => {
            return Err(CliError::new(format!(
                "this entry has no profile: {}. Run '{} ..entry' or '{} ..web' to complete initial setup",
                path.display(),
                context.entry_name,
                context.entry_name,
            )));
        }
        EntryProfileState::Invalid { path, error, .. } => {
            return Err(CliError::new(format!(
                "invalid entry profile '{}': {error}",
                path.display()
            )));
        }
    };
    let execution_context =
        CommandExecutionContext::new(context, &profile, resolved.path(), process_mode);
    let executor = CommandExecutor::new(&execution_context, &snapshot);
    match process_mode {
        CommandProcessMode::InheritConsole => executor.execute_journaled(argv),
        CommandProcessMode::NoWindow => executor.execute(argv),
    }
    .map_err(|error| CliError::new(error.to_string()))
}

fn launch_entry_host(context: &EntryContext) -> Result<i32, CliError> {
    let mut command = host_process_command(context);
    command.spawn().map_err(|error| {
        CliError::new(format!(
            "cannot start the Entry Host for '{}': {error}",
            context.entry_file.display()
        ))
    })?;
    Ok(0)
}

fn host_process_command(context: &EntryContext) -> Command {
    let mut command = Command::new(&context.entry_file);
    command
        .current_dir(&context.invocation_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    command
}

fn protocol_help(
    snapshot: &CatalogSnapshot,
    argv: &[OsString],
) -> Result<Option<String>, CliError> {
    let Some(target) = help_target(argv)? else {
        return Ok(None);
    };
    render_help(snapshot, &target)
        .map(Some)
        .map_err(|error| CliError::new(error.to_string()))
}

fn help_target(argv: &[OsString]) -> Result<Option<String>, CliError> {
    match argv {
        [marker] if marker.to_str().is_some_and(is_help_marker) => Ok(Some(String::new())),
        [marker, target] if marker.to_str().is_some_and(is_help_marker) => {
            let target = target
                .to_str()
                .ok_or_else(|| CliError::new("help target address is not valid Unicode"))?;
            Ok(Some(target.to_owned()))
        }
        _ => Ok(None),
    }
}

fn write_output(output: &str) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(output.as_bytes())?;
    handle.write_all(b"\n")?;
    handle.flush()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    message: String,
}

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CliError {}

#[cfg(test)]
mod tests;
