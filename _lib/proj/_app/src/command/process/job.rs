use std::io;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

use crate::command::{CommandError, CommandResult};

/// Owns one command process tree. The root process is created suspended, joined
/// to this Job Object, and only then resumed, so descendants cannot escape in a
/// spawn-to-assign race. Closing the Job is also a fail-safe tree terminator.
pub(super) struct OwnedProcessJob {
    handle: OwnedHandle,
}

impl OwnedProcessJob {
    pub(super) fn create() -> CommandResult<Self> {
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        let handle = owned(handle, "create the command Job Object")?;
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
            return Err(last_error("configure the command Job Object"));
        }
        Ok(Self { handle })
    }

    pub(super) fn assign_and_resume(&self, child: &mut Child) -> CommandResult<()> {
        if unsafe {
            AssignProcessToJobObject(
                self.handle.as_raw_handle() as HANDLE,
                child.as_raw_handle() as HANDLE,
            )
        } == 0
        {
            return Err(last_error(
                "assign the suspended command entry to its Job Object",
            ));
        }
        let thread = initial_thread(child.id())?;
        if unsafe { ResumeThread(thread.as_raw_handle() as HANDLE) } == u32::MAX {
            return Err(last_error("resume the command entry"));
        }
        Ok(())
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        if unsafe { TerminateJobObject(self.handle.as_raw_handle() as HANDLE, 1) } == 0 {
            return Err(contextual_error("terminate the command process tree"));
        }
        Ok(())
    }
}

fn initial_thread(process_id: u32) -> CommandResult<OwnedHandle> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(last_error("inspect the suspended command entry threads"));
    }
    let snapshot = Snapshot(snapshot);
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..Default::default()
    };
    let mut matching_thread = None;
    let mut available = unsafe { Thread32First(snapshot.0, &mut entry) } != 0;
    while available {
        if entry.th32OwnerProcessID == process_id {
            if matching_thread.replace(entry.th32ThreadID).is_some() {
                return Err(CommandError::new(
                    "the suspended command entry unexpectedly has multiple threads",
                ));
            }
        }
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        available = unsafe { Thread32Next(snapshot.0, &mut entry) } != 0;
    }
    let thread_id = matching_thread.ok_or_else(|| {
        CommandError::new("cannot find the suspended command entry's initial thread")
    })?;
    let handle = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    owned(handle, "open the suspended command entry thread")
}

struct Snapshot(HANDLE);

impl Drop for Snapshot {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn owned(handle: HANDLE, action: &str) -> CommandResult<OwnedHandle> {
    if handle.is_null() {
        return Err(last_error(action));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn last_error(action: &str) -> CommandError {
    let error = contextual_error(action);
    CommandError::new(error.to_string())
}

fn contextual_error(action: &str) -> io::Error {
    let error = io::Error::last_os_error();
    io::Error::new(error.kind(), format!("cannot {action}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant, SystemTime};
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{
        CREATE_SUSPENDED, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    #[test]
    fn termination_owns_descendants_created_by_the_command() {
        let root = std::env::temp_dir().join(format!(
            "swawkit-command-job-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let marker = root.join("descendant.pid");
        let powershell = std::path::Path::new(&std::env::var_os("SystemRoot").unwrap())
            .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        let script = format!(
            "$p=Start-Process $env:ComSpec -ArgumentList '/d','/c','ping -t 127.0.0.1' -WindowStyle Hidden -PassThru; [IO.File]::WriteAllText('{}',[string]$p.Id); Wait-Process -Id $p.Id",
            marker.display().to_string().replace('\'', "''")
        );
        let mut command = Command::new(powershell);
        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &script,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_SUSPENDED);
        let job = OwnedProcessJob::create().unwrap();
        let mut child = command.spawn().unwrap();
        job.assign_and_resume(&mut child).unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        while !marker.is_file() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let descendant_id: u32 = std::fs::read_to_string(&marker)
            .expect("descendant PID marker")
            .parse()
            .expect("descendant PID");
        let descendant = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, descendant_id) };
        assert!(!descendant.is_null(), "open descendant process");
        let descendant = unsafe { OwnedHandle::from_raw_handle(descendant) };

        job.terminate().unwrap();
        child.wait().unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(descendant.as_raw_handle() as HANDLE, 10_000) },
            WAIT_OBJECT_0,
            "the command descendant survived Job termination"
        );
        drop(descendant);
        std::fs::remove_dir_all(root).unwrap();
    }
}
