#[cfg(not(target_os = "windows"))]
compile_error!("The Swaw Kit Proj Host V0 supports Windows only.");

#[path = "../host_instance.rs"]
mod host_instance;
#[path = "../tray.rs"]
mod tray;

use std::error::Error;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::time::Duration;

use swawkit_proj::{
    context::EntryContext,
    data_root::{DataRootSession, ResolveDataRootRequest},
    host_runtime::HostRuntimeLocator,
    launch::{LaunchMode, LaunchRequest, clear_inherited_swawkit_environment},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

use crate::host_instance::{HostInstance, HostInstanceAcquisition};

fn main() {
    let result = LaunchRequest::from_process()
        .map_err(Into::into)
        .and_then(run);
    if let Err(error) = result {
        eprintln!("[ERROR] {error}");
        show_host_error(&error.to_string());
        std::process::exit(1);
    }
}

fn run(request: LaunchRequest) -> Result<(), Box<dyn Error>> {
    if request.mode != LaunchMode::InternalHost || !request.argv.is_empty() {
        return Err(format!(
            "the Host accepts only the '{}' launch mode without arguments",
            LaunchMode::InternalHost.as_env_value()
        )
        .into());
    }
    // SAFETY: this is the Host composition root and no thread exists yet.
    unsafe { clear_inherited_swawkit_environment() };
    let context = EntryContext::from_host_launch(&request)?;
    let data_root = DataRootSession::new(ResolveDataRootRequest {
        swawkit_home: &context.swawkit_home,
        entry_file: &context.entry_file,
    })?;
    let runtime = HostRuntimeLocator::new(&context, data_root.entry_identity());
    let instance = match HostInstance::acquire(data_root.entry_identity())? {
        HostInstanceAcquisition::Primary(instance) => instance,
        HostInstanceAcquisition::Existing => {
            let document = runtime.wait_for_healthy(Duration::from_secs(5))?;
            webbrowser::open(&document.url)
                .map_err(|error| format!("cannot activate the existing Entry Host: {error}"))?;
            return Ok(());
        }
    };
    tray::run(context, data_root, instance, runtime.acquire_owner())?;
    Ok(())
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
