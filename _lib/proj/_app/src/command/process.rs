use std::env;
use std::ffi::{OsStr, OsString};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use crate::catalog::{CommandAdapter, is_help_marker};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use super::{CommandError, CommandProcessMode, CommandResult, ProcessEnvironment};

const ADAPTER_ENVIRONMENT_PREFIX: &str = "SWAWKIT_PROJ_CORE_COMMAND_ADAPTER_";
const POWERSHELL_ARGUMENT_PREFIX: &str = "SWAWKIT_PROJ_CORE_COMMAND_ADAPTER_POWERSHELL_ARG_";
const POWERSHELL_ENTRY_ENV: &str = "SWAWKIT_PROJ_CORE_COMMAND_ADAPTER_POWERSHELL_ENTRY_PATH";
const POWERSHELL_COUNT_ENV: &str = "SWAWKIT_PROJ_CORE_COMMAND_ADAPTER_POWERSHELL_ARG_COUNT";
const CMD_ENTRY_ENV: &str = "SWAWKIT_PROJ_CORE_COMMAND_ADAPTER_CMD_ENTRY_PATH";

const POWERSHELL_RUNNER: &str = r#"
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
try {
    $entryPathName = 'SWAWKIT_PROJ_CORE_COMMAND_ADAPTER_POWERSHELL_ENTRY_PATH'
    $countName = 'SWAWKIT_PROJ_CORE_COMMAND_ADAPTER_POWERSHELL_ARG_COUNT'
    $argumentPrefix = 'SWAWKIT_PROJ_CORE_COMMAND_ADAPTER_POWERSHELL_ARG_'
    $entryPath = [Environment]::GetEnvironmentVariable($entryPathName, 'Process')
    $countText = [Environment]::GetEnvironmentVariable($countName, 'Process')
    $count = [int]::Parse($countText, [Globalization.CultureInfo]::InvariantCulture)
    [string[]]$entryArguments = @()
    for ($index = 0; $index -lt $count; $index++) {
        $entryArguments += [Environment]::GetEnvironmentVariable(
            ($argumentPrefix + $index),
            'Process'
        )
    }
    $processEnvironment = [Environment]::GetEnvironmentVariables(
        [EnvironmentVariableTarget]::Process
    )
    foreach ($name in [string[]]@($processEnvironment.Keys)) {
        if ($name.Equals($entryPathName, [StringComparison]::OrdinalIgnoreCase) -or
            $name.Equals($countName, [StringComparison]::OrdinalIgnoreCase) -or
            $name.StartsWith($argumentPrefix, [StringComparison]::OrdinalIgnoreCase)) {
            [Environment]::SetEnvironmentVariable($name, $null, 'Process')
        }
    }
    $global:LASTEXITCODE = 0
    & $entryPath @entryArguments
    $entrySucceeded = $?
    $entryExitCode = [int]$global:LASTEXITCODE
    if (-not $entrySucceeded -and $entryExitCode -eq 0) {
        $entryExitCode = 1
    }
    exit $entryExitCode
} catch {
    [Console]::Error.WriteLine(
        ('PowerShell entry failed: entry={0}; data={1}; error={2}; at={3}' -f
            $entryPath,
            $env:SWAWKIT_PROJ_DATA_ROOT,
            $_.Exception.Message,
            $_.InvocationInfo.PositionMessage)
    )
    exit 1
}
"#;

pub(crate) fn run_process(
    adapter: CommandAdapter,
    entry_path: &Path,
    arguments: &[OsString],
    working_directory: &Path,
    environment: &ProcessEnvironment,
    process_mode: CommandProcessMode,
) -> CommandResult<i32> {
    validate_adapter(adapter)?;
    let mut command = match adapter {
        CommandAdapter::Exe => executable_command(entry_path, arguments),
        CommandAdapter::PowerShell => powershell_command(entry_path, arguments)?,
        CommandAdapter::Cmd => cmd_command(entry_path, arguments)?,
        CommandAdapter::Core | CommandAdapter::Bun | CommandAdapter::Python => unreachable!(),
    };
    command.current_dir(working_directory);
    command.creation_flags(process_creation_flags(process_mode));
    environment.apply(&mut command);
    let status = command.status().map_err(|error| {
        CommandError::new(format!(
            "cannot start command entry '{}': {error}",
            entry_path.display()
        ))
    })?;
    Ok(status.code().unwrap_or(1))
}

fn process_creation_flags(process_mode: CommandProcessMode) -> u32 {
    match process_mode {
        CommandProcessMode::InheritConsole => 0,
        CommandProcessMode::NoWindow => CREATE_NO_WINDOW,
    }
}

pub(crate) fn validate_adapter(adapter: CommandAdapter) -> CommandResult<()> {
    if matches!(
        adapter,
        CommandAdapter::Exe | CommandAdapter::PowerShell | CommandAdapter::Cmd
    ) {
        return Ok(());
    }
    Err(CommandError::new(format!(
        "the Rust V0 executor does not yet support the '{}' adapter",
        adapter.as_str()
    )))
}

fn executable_command(entry_path: &Path, arguments: &[OsString]) -> Command {
    let mut command = Command::new(entry_path);
    remove_inherited_adapter_environment(&mut command);
    command.args(arguments);
    command
}

fn powershell_command(entry_path: &Path, arguments: &[OsString]) -> CommandResult<Command> {
    let system_root = env::var_os("SystemRoot")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CommandError::new("SystemRoot is unavailable"))?;
    let executable = Path::new(&system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    if !executable.is_file() {
        return Err(CommandError::new(format!(
            "Windows PowerShell is unavailable: {}",
            executable.display()
        )));
    }

    let mut command = Command::new(executable);
    remove_inherited_adapter_environment(&mut command);
    command.args([
        OsStr::new("-NoLogo"),
        OsStr::new("-NoProfile"),
        OsStr::new("-ExecutionPolicy"),
        OsStr::new("Bypass"),
        OsStr::new("-Command"),
        OsStr::new(POWERSHELL_RUNNER),
    ]);
    command.env(POWERSHELL_ENTRY_ENV, entry_path);
    command.env(POWERSHELL_COUNT_ENV, arguments.len().to_string());
    for (index, argument) in arguments.iter().enumerate() {
        command.env(format!("{POWERSHELL_ARGUMENT_PREFIX}{index}"), argument);
    }
    Ok(command)
}

fn cmd_command(entry_path: &Path, arguments: &[OsString]) -> CommandResult<Command> {
    let marker = match arguments {
        [] => None,
        [marker] if marker.to_str().is_some_and(is_help_marker) => marker.to_str(),
        _ => {
            return Err(CommandError::new(
                "the V0 run.cmd adapter accepts no dynamic arguments except one standalone help \
                 selector",
            ));
        }
    };
    let executable = env::var_os("ComSpec")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CommandError::new("the Windows command processor is unavailable"))?;
    if !Path::new(&executable).is_file() {
        return Err(CommandError::new(format!(
            "the Windows command processor is unavailable: {}",
            Path::new(&executable).display()
        )));
    }

    let command_line = match marker {
        Some(marker) => {
            format!("/d /s /v:off /c \"set \"{CMD_ENTRY_ENV}=\" & \"%{CMD_ENTRY_ENV}%\" {marker}\"")
        }
        None => format!("/d /s /v:off /c \"set \"{CMD_ENTRY_ENV}=\" & \"%{CMD_ENTRY_ENV}%\"\""),
    };
    let mut command = Command::new(executable);
    remove_inherited_adapter_environment(&mut command);
    command.raw_arg(command_line);
    command.env(CMD_ENTRY_ENV, entry_path);
    Ok(command)
}

fn remove_inherited_adapter_environment(command: &mut Command) {
    for (name, _value) in env::vars_os() {
        if name
            .to_str()
            .is_some_and(|name| has_ascii_prefix(name, ADAPTER_ENVIRONMENT_PREFIX))
        {
            command.env_remove(name);
        }
    }
}

fn has_ascii_prefix(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_mode_hides_the_window_without_changing_normal_cli() {
        assert_eq!(
            process_creation_flags(CommandProcessMode::InheritConsole),
            0
        );
        assert_eq!(
            process_creation_flags(CommandProcessMode::NoWindow),
            CREATE_NO_WINDOW
        );
    }
}
