#[cfg(not(target_os = "windows"))]
compile_error!("The Swaw Kit Proj application V0 supports Windows only.");

mod cli;

use std::error::Error;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use swawkit_proj::{
    command::CommandProcessMode,
    context::EntryContext,
    launch::{
        ENTRY_FILE_ENV, LAUNCH_MODE_ENV, LAUNCH_PROTOCOL_ENV, LAUNCH_PROTOCOL_VERSION, LaunchMode,
        LaunchRequest, clear_inherited_swawkit_environment,
    },
};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

fn main() {
    let request = match LaunchRequest::from_process() {
        Ok(request) => request,
        Err(error) => {
            eprintln!("[ERROR] {error}");
            if std::env::var_os(LAUNCH_MODE_ENV)
                .is_some_and(|value| value == LaunchMode::InternalHost.as_env_value())
            {
                show_host_error(&error.to_string());
            }
            std::process::exit(1);
        }
    };
    let host_mode = request.mode == LaunchMode::InternalHost;
    match run(request) {
        Ok(0) => {}
        Ok(exit_code) => std::process::exit(exit_code),
        Err(error) => {
            eprintln!("[ERROR] {error}");
            if host_mode {
                show_host_error(&error.to_string());
            }
            std::process::exit(1);
        }
    }
}

fn run(request: LaunchRequest) -> Result<i32, Box<dyn Error>> {
    // SAFETY: `run` is the process composition root and no application thread
    // has been spawned yet. All launch facts needed below are owned by request.
    unsafe { clear_inherited_swawkit_environment() };
    let context = EntryContext::from_launch(&request)?;

    match request.mode {
        LaunchMode::Cli => cli::run(&context, &request.argv, CommandProcessMode::InheritConsole)
            .map_err(Into::into),
        LaunchMode::Worker => {
            cli::run(&context, &request.argv, CommandProcessMode::NoWindow).map_err(Into::into)
        }
        LaunchMode::InternalHost => launch_host(&request, &context),
    }
}

fn launch_host(request: &LaunchRequest, context: &EntryContext) -> Result<i32, Box<dyn Error>> {
    if !request.argv.is_empty() {
        return Err("the internal Host launch cannot carry user arguments".into());
    }
    let core = std::env::current_exe()?;
    let host = core.with_file_name("swawkit-proj-host.exe");
    let metadata = std::fs::symlink_metadata(&host).map_err(|error| {
        format!(
            "the Host executable is missing from the current release '{}': {error}",
            host.display()
        )
    })?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "the Host executable is not a regular release file: {}",
            host.display()
        )
        .into());
    }

    Command::new(&host)
        .current_dir(&context.invocation_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .env(LAUNCH_PROTOCOL_ENV, LAUNCH_PROTOCOL_VERSION)
        .env(ENTRY_FILE_ENV, &context.entry_file)
        .env(LAUNCH_MODE_ENV, LaunchMode::InternalHost.as_env_value())
        .spawn()
        .map_err(|error| format!("cannot start the Entry Host '{}': {error}", host.display()))?;
    Ok(0)
}

fn show_host_error(error: &str) {
    let title = null_terminated("Swaw Kit 无法打开");
    let message = null_terminated(&format!(
        "无法启动或激活 Swaw Kit 控制台。\n\n{error}\n\n请稍后重试；该错误不会被静默忽略。"
    ));
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn null_terminated(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
