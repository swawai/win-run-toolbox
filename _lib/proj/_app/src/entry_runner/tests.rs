use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Environment::SetEnvironmentVariableW;
use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, OpenJobObjectW};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, EVENT_MODIFY_STATE, GetCurrentProcess, OpenEventW, SetEvent,
};

use super::Utf8LossyDecoder;
use crate::launch::{
    WORKER_JOB_NAME_ENV, WORKER_PROTOCOL_ENV, WORKER_PROTOCOL_VERSION, WORKER_READY_EVENT_NAME_ENV,
};

const JOB_OBJECT_ASSIGN_PROCESS_ACCESS: u32 = 0x0001;
const NORMAL_MARKER: &str = "web-native-worker.marker";
const CANCEL_PID_MARKER: &str = "web-native-worker-descendant.pid";
const STDOUT_SENTINEL: &str = "SWAWKIT_WEB_NATIVE_STDOUT_SENTINEL";
const STDERR_SENTINEL: &str = "SWAWKIT_WEB_NATIVE_STDERR_SENTINEL";
const PROGRESS_FRAME: &str = "\u{001e}swawkit-event-v1 {\"schema\":\"swawkit.command-event/v1\",\"kind\":\"progress\",\"id\":\"download:fixture.zip\",\"state\":\"completed\",\"current\":42,\"total\":42,\"unit\":\"bytes\",\"message\":\"Downloaded fixture.zip\"}";

#[test]
fn decoder_preserves_utf8_split_across_reads() {
    let mut decoder = Utf8LossyDecoder::default();
    let bytes = "中文".as_bytes();

    assert_eq!(decoder.decode(&bytes[..2], false), None);
    assert_eq!(decoder.decode(&bytes[2..4], false), Some("中".to_owned()));
    assert_eq!(decoder.decode(&bytes[4..], false), Some("文".to_owned()));
    assert_eq!(decoder.decode(&[], true), None);
}

#[test]
fn decoder_replaces_invalid_bytes_without_losing_neighbors() {
    let mut decoder = Utf8LossyDecoder::default();

    assert_eq!(
        decoder.decode(b"before\xffafter", false),
        Some("before\u{fffd}after".to_owned())
    );
}

#[test]
fn decoder_flushes_an_incomplete_sequence_at_eof() {
    let mut decoder = Utf8LossyDecoder::default();

    assert_eq!(decoder.decode(&[0xe4, 0xb8], false), None);
    assert_eq!(decoder.decode(&[], true), Some("\u{fffd}".to_owned()));
}

// These two tests double as the copied libtest executable's worker commands.
// Their names intentionally use valid Action-address syntax so the first argv
// value is also an exact libtest filter.
#[test]
fn webnativeworkerfixture() {
    if !join_declared_worker_boundary() {
        return;
    }

    fs::write(NORMAL_MARKER, "worker cwd reached\n").expect("write native worker marker");
    println!("{STDOUT_SENTINEL}");
    eprintln!("{PROGRESS_FRAME}");
    eprintln!("{STDERR_SENTINEL}");
}

#[test]
fn webnativeworkercancelfixture() {
    if !join_declared_worker_boundary() {
        return;
    }

    let powershell = windows_powershell();
    let mut descendant = Command::new(&powershell)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 60",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .unwrap_or_else(|error| {
            panic!(
                "start native worker descendant '{}': {error}",
                powershell.display()
            )
        });
    fs::write(CANCEL_PID_MARKER, descendant.id().to_string())
        .expect("write native worker descendant PID");
    descendant
        .wait()
        .expect("wait for native worker descendant");
}

fn join_declared_worker_boundary() -> bool {
    let protocol = env::var_os(WORKER_PROTOCOL_ENV);
    let job_name = env::var_os(WORKER_JOB_NAME_ENV);
    let ready_event_name = env::var_os(WORKER_READY_EVENT_NAME_ENV);
    if protocol.is_none() && job_name.is_none() && ready_event_name.is_none() {
        return false;
    }

    assert_eq!(
        protocol.as_deref(),
        Some(OsStr::new(WORKER_PROTOCOL_VERSION)),
        "invalid native worker protocol"
    );
    let job_name = job_name.expect("native worker Job Object name");
    let ready_event_name = ready_event_name.expect("native worker ready event name");
    let job_name = null_terminated(&job_name);
    let ready_event_name = null_terminated(&ready_event_name);

    let job = owned_handle(
        unsafe { OpenJobObjectW(JOB_OBJECT_ASSIGN_PROCESS_ACCESS, 0, job_name.as_ptr()) },
        "open native worker Job Object",
    );
    let ready = owned_handle(
        unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, ready_event_name.as_ptr()) },
        "open native worker ready event",
    );
    assert_ne!(
        unsafe { AssignProcessToJobObject(raw_handle(&job), GetCurrentProcess()) },
        0,
        "assign copied libtest process to native worker Job Object: {}",
        std::io::Error::last_os_error()
    );
    drop(job);

    for name in [
        WORKER_PROTOCOL_ENV,
        WORKER_JOB_NAME_ENV,
        WORKER_READY_EVENT_NAME_ENV,
    ] {
        let name = null_terminated(OsStr::new(name));
        assert_ne!(
            unsafe { SetEnvironmentVariableW(name.as_ptr(), std::ptr::null()) },
            0,
            "clear native worker declaration: {}",
            std::io::Error::last_os_error()
        );
    }
    assert_ne!(
        unsafe { SetEvent(raw_handle(&ready)) },
        0,
        "signal native worker ready event: {}",
        std::io::Error::last_os_error()
    );
    true
}

fn windows_powershell() -> PathBuf {
    PathBuf::from(env::var_os("SystemRoot").expect("SystemRoot in clean worker environment"))
        .join("System32/WindowsPowerShell/v1.0/powershell.exe")
}

fn null_terminated(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn owned_handle(handle: HANDLE, action: &str) -> OwnedHandle {
    assert!(
        !handle.is_null(),
        "{action}: {}",
        std::io::Error::last_os_error()
    );
    unsafe { OwnedHandle::from_raw_handle(handle) }
}

fn raw_handle(handle: &OwnedHandle) -> HANDLE {
    use std::os::windows::io::AsRawHandle;
    handle.as_raw_handle() as HANDLE
}
