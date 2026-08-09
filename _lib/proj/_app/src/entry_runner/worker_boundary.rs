use std::ffi::OsStr;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;
use std::sync::Arc;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_CANCELLED, GetLastError, HANDLE, WAIT_FAILED, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows_sys::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects};

const OBJECT_NAME_PREFIX: &str = r"Local\SwawKit.Proj.CommandRun.";

pub(super) enum WorkerReady {
    Ready,
    ProcessExited,
    TimedOut,
}

pub(super) struct WorkerBoundary {
    pub job: Arc<WorkerJob>,
    ready: OwnedHandle,
    pub job_name: String,
    pub ready_event_name: String,
}

impl WorkerBoundary {
    pub fn create(run_id: &str) -> io::Result<Self> {
        let job_name = format!("{OBJECT_NAME_PREFIX}{run_id}.Job");
        let ready_event_name = format!("{OBJECT_NAME_PREFIX}{run_id}.Ready");
        let job = Arc::new(WorkerJob::create(&job_name)?);
        let ready = create_ready_event(&ready_event_name)?;
        Ok(Self {
            job,
            ready,
            job_name,
            ready_event_name,
        })
    }

    pub fn wait(&self, child: &Child, timeout: Duration) -> io::Result<WorkerReady> {
        let handles = [
            self.ready.as_raw_handle() as HANDLE,
            child.as_raw_handle() as HANDLE,
        ];
        let milliseconds = timeout.as_millis().min(u32::MAX as u128) as u32;
        let result = unsafe {
            WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, milliseconds)
        };
        match result {
            WAIT_OBJECT_0 => Ok(WorkerReady::Ready),
            value if value == WAIT_OBJECT_0 + 1 => Ok(WorkerReady::ProcessExited),
            WAIT_TIMEOUT => Ok(WorkerReady::TimedOut),
            WAIT_FAILED => Err(contextual_error(
                "cannot wait for the Entry Launcher worker boundary",
            )),
            value => Err(io::Error::other(format!(
                "unexpected Entry Launcher worker wait result: {value}"
            ))),
        }
    }
}

pub(super) struct WorkerJob {
    handle: OwnedHandle,
}

impl WorkerJob {
    fn create(name: &str) -> io::Result<Self> {
        let name = null_terminated(name);
        unsafe { windows_sys::Win32::Foundation::SetLastError(0) };
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), name.as_ptr()) };
        let creation_error = unsafe { GetLastError() };
        let handle = owned_handle(handle, "create the command worker Job Object")?;
        if creation_error == ERROR_ALREADY_EXISTS {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the command worker Job Object name already exists",
            ));
        }

        let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                handle.as_raw_handle() as HANDLE,
                JobObjectExtendedLimitInformation,
                (&information as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(contextual_error(
                "cannot configure the command worker Job Object",
            ));
        }
        Ok(Self { handle })
    }

    pub fn cancel(&self) -> io::Result<()> {
        self.terminate(ERROR_CANCELLED)
    }

    pub fn terminate_remaining(&self) -> io::Result<()> {
        self.terminate(0)
    }

    fn terminate(&self, exit_code: u32) -> io::Result<()> {
        if unsafe { TerminateJobObject(self.handle.as_raw_handle() as HANDLE, exit_code) } == 0 {
            return Err(contextual_error(
                "cannot terminate the command worker process tree",
            ));
        }
        Ok(())
    }
}

fn create_ready_event(name: &str) -> io::Result<OwnedHandle> {
    let name = null_terminated(name);
    unsafe { windows_sys::Win32::Foundation::SetLastError(0) };
    let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, name.as_ptr()) };
    let creation_error = unsafe { GetLastError() };
    let handle = owned_handle(handle, "create the command worker ready event")?;
    if creation_error == ERROR_ALREADY_EXISTS {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the command worker ready event name already exists",
        ));
    }
    Ok(handle)
}

fn owned_handle(handle: HANDLE, action: &str) -> io::Result<OwnedHandle> {
    if handle.is_null() {
        return Err(contextual_error(&format!("cannot {action}")));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn null_terminated(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn contextual_error(action: &str) -> io::Error {
    let error = io::Error::last_os_error();
    io::Error::new(error.kind(), format!("{action}: {error}"))
}
