#[cfg(not(target_os = "windows"))]
compile_error!("The Swaw Kit Proj application V0 supports Windows only.");

mod cli;
mod host_instance;
mod tray;

use std::error::Error;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::time::Duration;
use swawkit_proj::{
    command::CommandProcessMode,
    context::EntryContext,
    data_root::{DataRootSession, ResolveDataRootRequest},
    host_runtime::HostRuntimeLocator,
    launch::{LAUNCH_MODE_ENV, LaunchMode, LaunchRequest, clear_inherited_swawkit_environment},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

use crate::host_instance::{HostInstance, HostInstanceAcquisition};

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
        LaunchMode::InternalHost => {
            let data_root = DataRootSession::new(ResolveDataRootRequest {
                swawkit_home: &context.swawkit_home,
                entry_file: &context.entry_file,
            })?;
            let runtime = HostRuntimeLocator::new(&context, data_root.entry_identity());
            let instance = match HostInstance::acquire(data_root.entry_identity())? {
                HostInstanceAcquisition::Primary(instance) => instance,
                HostInstanceAcquisition::Existing => {
                    let document = runtime.wait_for_healthy(Duration::from_secs(5))?;
                    webbrowser::open(&document.url).map_err(|error| {
                        format!("cannot activate the existing Entry Host: {error}")
                    })?;
                    return Ok(0);
                }
            };
            tray::run(context, data_root, instance, runtime.acquire_owner())?;
            Ok(0)
        }
    }
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
