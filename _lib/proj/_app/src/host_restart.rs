use std::env;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HANDLE, SetLastError, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateEventW, EVENT_MODIFY_STATE, OpenEventW, OpenProcess,
    PROCESS_SYNCHRONIZE, SetEvent, WaitForSingleObject,
};

use crate::context::EntryContext;
use crate::entry::EntryIdentity;
use crate::launch::{
    ENTRY_FILE_ENV, LAUNCH_MODE_ENV, LAUNCH_PROTOCOL_ENV, LAUNCH_PROTOCOL_VERSION, LaunchMode,
};

const PROTOCOL_ENV: &str = "SWAWKIT_PROJ_HOST_RESTART_PROTOCOL";
const PARENT_PID_ENV: &str = "SWAWKIT_PROJ_HOST_RESTART_PARENT_PID";
const READY_EVENT_ENV: &str = "SWAWKIT_PROJ_HOST_RESTART_READY_EVENT";
const PROTOCOL_VERSION: &str = "1";
const EVENT_PREFIX: &str = r"Local\SwawKit.Proj.Host.Restart.";
const READY_TIMEOUT: Duration = Duration::from_secs(5);
static NEXT_EVENT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct HostRestartRequest {
    parent_pid: u32,
    ready_event: OsString,
}

impl HostRestartRequest {
    pub fn from_process() -> Result<Option<Self>, String> {
        let protocol = env::var_os(PROTOCOL_ENV);
        let parent_pid = env::var_os(PARENT_PID_ENV);
        let ready_event = env::var_os(READY_EVENT_ENV);
        if protocol.is_none() && parent_pid.is_none() && ready_event.is_none() {
            return Ok(None);
        }
        if protocol.as_deref() != Some(OsStr::new(PROTOCOL_VERSION)) {
            return Err(format!(
                "unsupported Host restart declaration {PROTOCOL_ENV}; expected '{PROTOCOL_VERSION}'"
            ));
        }
        let parent_pid = parent_pid
            .and_then(|value| value.to_str().and_then(|value| value.parse::<u32>().ok()))
            .filter(|value| *value != 0 && *value != std::process::id())
            .ok_or_else(|| "Host restart parent PID is invalid".to_owned())?;
        let ready_event = ready_event
            .filter(|value| {
                let value = value.to_string_lossy();
                let suffix = value.strip_prefix(EVENT_PREFIX).unwrap_or_default();
                value.starts_with(EVENT_PREFIX)
                    && value.len() <= 160
                    && !suffix.is_empty()
                    && suffix
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() || byte == b'.')
            })
            .ok_or_else(|| "Host restart ready Event name is invalid".to_owned())?;
        Ok(Some(Self {
            parent_pid,
            ready_event,
        }))
    }

    pub fn complete(self, context: &EntryContext) -> Result<(), String> {
        let entry_identity = EntryIdentity::read(&context.entry_file)
            .map_err(|error| format!("cannot pin the Entry before Host restart: {error}"))?;
        let parent = owned_handle(
            unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, self.parent_pid) },
            "open the retiring Host process",
        )?;
        let ready_event = null_terminated(&self.ready_event);
        let ready = owned_handle(
            unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, ready_event.as_ptr()) },
            "open the Host restart ready Event",
        )?;
        if unsafe { SetEvent(raw_handle(&ready)) } == 0 {
            return Err(last_error("signal Host restart readiness"));
        }
        drop(ready);

        if unsafe { WaitForSingleObject(raw_handle(&parent), u32::MAX) } != WAIT_OBJECT_0 {
            return Err(last_error("wait for the retiring Host"));
        }
        let current_identity = EntryIdentity::read(&context.entry_file)
            .map_err(|error| format!("cannot revalidate the Entry after Host shutdown: {error}"))?;
        if current_identity != entry_identity {
            return Err("the Entry Launcher changed while the Host was restarting".to_owned());
        }

        let mut launcher = Command::new(&context.entry_file)
            .current_dir(&context.invocation_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|error| {
                format!(
                    "cannot start the Entry Launcher after Host shutdown '{}': {error}",
                    context.entry_file.display()
                )
            })?;
        let status = launcher
            .wait()
            .map_err(|error| format!("cannot wait for the Entry Launcher: {error}"))?;
        if !status.success() {
            return Err(format!(
                "the Entry Launcher failed during Host restart with exit code {}",
                status.code().unwrap_or(-1)
            ));
        }
        Ok(())
    }
}

pub fn prepare(context: &EntryContext) -> Result<(), String> {
    let event_name = unique_event_name();
    let wide_event_name = null_terminated(OsStr::new(&event_name));
    unsafe { SetLastError(0) };
    let ready_handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, wide_event_name.as_ptr()) };
    let creation_error = unsafe { GetLastError() };
    let ready = owned_handle(ready_handle, "create the Host restart ready Event")?;
    if creation_error == ERROR_ALREADY_EXISTS {
        return Err("the generated Host restart ready Event already exists".to_owned());
    }

    let mut coordinator = Command::new(&context.product_executable)
        .current_dir(&context.invocation_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .env(LAUNCH_PROTOCOL_ENV, LAUNCH_PROTOCOL_VERSION)
        .env(ENTRY_FILE_ENV, &context.entry_file)
        .env(LAUNCH_MODE_ENV, LaunchMode::InternalHost.as_env_value())
        .env(PROTOCOL_ENV, PROTOCOL_VERSION)
        .env(PARENT_PID_ENV, std::process::id().to_string())
        .env(READY_EVENT_ENV, &event_name)
        .spawn()
        .map_err(|error| format!("cannot start the Host restart coordinator: {error}"))?;

    let wait = unsafe {
        WaitForSingleObject(
            raw_handle(&ready),
            u32::try_from(READY_TIMEOUT.as_millis()).expect("ready timeout fits u32"),
        )
    };
    if wait != WAIT_OBJECT_0 {
        let _ = coordinator.kill();
        let _ = coordinator.wait();
        return if wait == WAIT_TIMEOUT {
            Err("the Host restart coordinator did not become ready within 5 seconds".to_owned())
        } else {
            Err(last_error("wait for Host restart coordinator readiness"))
        };
    }
    if let Some(status) = coordinator
        .try_wait()
        .map_err(|error| format!("cannot inspect the Host restart coordinator: {error}"))?
    {
        return Err(format!(
            "the Host restart coordinator exited early with code {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

fn unique_event_name() -> String {
    let sequence = NEXT_EVENT.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{EVENT_PREFIX}{:x}.{timestamp:x}.{sequence:x}",
        std::process::id()
    )
}

fn null_terminated(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn owned_handle(handle: HANDLE, action: &str) -> Result<OwnedHandle, String> {
    if handle.is_null() {
        return Err(last_error(action));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn raw_handle(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle()
}

fn last_error(action: &str) -> String {
    let error = io::Error::last_os_error();
    format!("cannot {action}: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_event_names_fit_the_private_protocol() {
        let name = unique_event_name();
        assert!(name.starts_with(EVENT_PREFIX));
        assert!(name.len() <= 160);
        assert!(
            name[EVENT_PREFIX.len()..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b'.')
        );
    }
}
