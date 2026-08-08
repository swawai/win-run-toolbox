mod claim;
mod control;

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use swawkit_proj::{
    catalog::{CatalogSnapshot, is_help_marker},
    command::{CommandExecutionContext, CommandExecutor},
    context::EntryContext,
    data_root::{DataRootClaimApprover, ResolveDataRootRequest, resolve_data_root},
    help::{HelpRenderError, render_help},
    launch::{ENTRY_FILE_ENV, LAUNCH_MODE_ENV, LaunchMode},
    profile::{EntryProfileState, EntryProfileStore},
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

pub fn run(context: &EntryContext, argv: &[OsString]) -> Result<i32, CliError> {
    let inherited_data_root = env::var_os("SWAWKIT_PROJ_DATA_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let legacy_data_directory = env::var_os("SWAWKIT_PROJ_TARGET_PROJECT_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| path.join("data"));
    let mut approver =
        |pending: &swawkit_proj::data_root::DataRootClaim| Err(claim::rejection(context, pending));
    run_with_approver(
        context,
        argv,
        inherited_data_root.as_deref(),
        legacy_data_directory.as_deref(),
        &mut approver,
    )
}

fn run_with_approver(
    context: &EntryContext,
    argv: &[OsString],
    inherited_data_root: Option<&std::path::Path>,
    legacy_data_directory: Option<&std::path::Path>,
    approver: &mut impl DataRootClaimApprover,
) -> Result<i32, CliError> {
    let mut host_launcher = launch_entry_host;
    run_with_host_launcher(
        context,
        argv,
        inherited_data_root,
        legacy_data_directory,
        approver,
        &mut host_launcher,
    )
}

fn run_with_host_launcher(
    context: &EntryContext,
    argv: &[OsString],
    inherited_data_root: Option<&std::path::Path>,
    legacy_data_directory: Option<&std::path::Path>,
    approver: &mut impl DataRootClaimApprover,
    host_launcher: &mut impl FnMut(&EntryContext) -> Result<i32, CliError>,
) -> Result<i32, CliError> {
    match control::dispatch_before_data_root(context, argv, host_launcher)? {
        Some(control::PreDataRootControl::Claim { snapshot, address }) => {
            return claim::run(
                context,
                argv,
                inherited_data_root,
                legacy_data_directory,
                &snapshot,
                &address,
            );
        }
        Some(control::PreDataRootControl::Complete(exit_code)) => return Ok(exit_code),
        None => {}
    }

    let resolved = resolve_data_root(
        ResolveDataRootRequest {
            swawkit_home: &context.swawkit_home,
            entry_file: &context.entry_file,
            inherited_data_root,
            legacy_data_directory,
        },
        approver,
    )
    .map_err(|error| CliError::new(format!("DataRoot resolution failed: {error}")))?;
    for warning in resolved.warnings {
        eprintln!("[WARNING] {warning}");
    }

    let profile_store = EntryProfileStore::new(&context.swawkit_home, &resolved.path);
    let profile_state = profile_store.read();
    let snapshot = CatalogSnapshot::discover(
        context,
        profile_state.ready().map(|profile| profile.binding()),
    )
    .map_err(|error| CliError::new(format!("catalog discovery failed: {error}")))?;
    if let Some(output) = protocol_help(&snapshot, argv)? {
        write_output(&output)
            .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))?;
        return Ok(0);
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
    let execution_context = CommandExecutionContext::new(context, &profile, resolved.path);
    CommandExecutor::new(&execution_context, &snapshot)
        .execute(argv)
        .map_err(|error| CliError::new(error.to_string()))
}

fn launch_entry_host(context: &EntryContext) -> Result<i32, CliError> {
    let executable = env::current_exe().map_err(|error| {
        CliError::new(format!(
            "cannot locate the shared Proj executable for the Entry Host: {error}"
        ))
    })?;
    let inherited_names = env::vars_os().map(|(name, _value)| name);
    let mut command = host_process_command(context, &executable, inherited_names);
    command.spawn().map_err(|error| {
        CliError::new(format!(
            "cannot start the Entry Host for '{}': {error}",
            context.entry_file.display()
        ))
    })?;
    Ok(0)
}

fn host_process_command(
    context: &EntryContext,
    executable: &std::path::Path,
    inherited_names: impl IntoIterator<Item = OsString>,
) -> Command {
    const CONTEXT_ENVIRONMENT: [&str; 15] = [
        "SWAWKIT_HOME",
        "SWAWKIT_PROJ_PROTOCOL",
        "SWAWKIT_PROJ_TARGET_PROJECT_ROOT",
        "SWAWKIT_PROJ_ACTION_ROOT",
        "SWAWKIT_PROJ_DATA_ROOT",
        "SWAWKIT_PROJ_ENTRY_COMMAND",
        "SWAWKIT_PROJ_COMMAND_PROTOCOL",
        "SWAWKIT_PROJ_COMMAND_PHASE",
        "SWAWKIT_PROJ_COMMAND_ADDRESS",
        "SWAWKIT_PROJ_COMMAND_DIR",
        "SWAWKIT_PROJ_COMMAND_DATA_ROOT",
        "SWAWKIT_PROJ_GUARD_SCOPE",
        "SWAWKIT_PROJ_HELP_TARGET_ADDRESS",
        "SWAWKIT_PROJ_INVOCATION_DIR",
        "SWAWKIT_PROJ_INTERNAL_RUNTIME_WORKING_DIR",
    ];
    const INTERNAL_PREFIXES: [&str; 2] =
        ["SWAWKIT_PROJ_INTERNAL_PS_", "SWAWKIT_PROJ_INTERNAL_CMD_"];

    let mut command = Command::new(executable);
    command
        .current_dir(&context.invocation_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    for name in CONTEXT_ENVIRONMENT {
        command.env_remove(name);
    }
    for name in inherited_names {
        if name.to_str().is_some_and(|name| {
            INTERNAL_PREFIXES
                .iter()
                .any(|prefix| has_ascii_prefix(name, prefix))
        }) {
            command.env_remove(name);
        }
    }
    command
        .env(ENTRY_FILE_ENV, &context.entry_file)
        .env(LAUNCH_MODE_ENV, LaunchMode::InternalHost.as_env_value());
    command
}

fn has_ascii_prefix(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}

fn protocol_help(
    snapshot: &CatalogSnapshot,
    argv: &[OsString],
) -> Result<Option<String>, CliError> {
    let Some(target) = help_target(argv)? else {
        return Ok(None);
    };
    match render_help(snapshot, &target) {
        Ok(output) => Ok(Some(output)),
        Err(HelpRenderError::Unavailable(address)) if !address.is_empty() => Ok(None),
        Err(error) => Err(CliError::new(error.to_string())),
    }
}

fn help_target(argv: &[OsString]) -> Result<Option<String>, CliError> {
    match argv {
        [marker] if marker.to_str().is_some_and(is_help_marker) => Ok(Some(String::new())),
        [target, marker] if marker.to_str().is_some_and(is_help_marker) => {
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
