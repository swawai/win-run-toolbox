use std::io;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

/// Owns one process tree from before its root process can execute.
///
/// The caller must create the root process with `CREATE_SUSPENDED`, assign it
/// here, and only then resume it. This closes the spawn-to-assign window in
/// which a descendant could otherwise escape the Job Object.
pub(crate) struct OwnedProcessJob {
    handle: OwnedHandle,
}

impl OwnedProcessJob {
    pub(crate) fn create() -> io::Result<Self> {
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        let handle = owned(handle, "create the process Job Object")?;
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
            return Err(last_error("configure the process Job Object"));
        }
        Ok(Self { handle })
    }

    pub(crate) fn assign_and_resume(&self, child: &mut Child) -> io::Result<()> {
        if unsafe {
            AssignProcessToJobObject(
                self.handle.as_raw_handle() as HANDLE,
                child.as_raw_handle() as HANDLE,
            )
        } == 0
        {
            return Err(last_error("assign the suspended process to its Job Object"));
        }
        let thread = initial_thread(child.id())?;
        if unsafe { ResumeThread(thread.as_raw_handle() as HANDLE) } == u32::MAX {
            return Err(last_error("resume the process entry"));
        }
        Ok(())
    }

    pub(crate) fn cancel(&self) -> io::Result<()> {
        self.terminate_with_exit_code(ERROR_CANCELLED)
    }

    pub(crate) fn terminate_remaining(&self) -> io::Result<()> {
        self.terminate_with_exit_code(0)
    }

    pub(crate) fn terminate(&self) -> io::Result<()> {
        self.terminate_with_exit_code(1)
    }

    fn terminate_with_exit_code(&self, exit_code: u32) -> io::Result<()> {
        if unsafe { TerminateJobObject(self.handle.as_raw_handle() as HANDLE, exit_code) } == 0 {
            return Err(last_error("terminate the process tree"));
        }
        Ok(())
    }
}

fn initial_thread(process_id: u32) -> io::Result<OwnedHandle> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(last_error("inspect the suspended process threads"));
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
                return Err(io::Error::other(
                    "the suspended process unexpectedly has multiple threads",
                ));
            }
        }
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        available = unsafe { Thread32Next(snapshot.0, &mut entry) } != 0;
    }
    let thread_id = matching_thread
        .ok_or_else(|| io::Error::other("cannot find the suspended process initial thread"))?;
    let handle = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id) };
    owned(handle, "open the suspended process initial thread")
}

struct Snapshot(HANDLE);

impl Drop for Snapshot {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn owned(handle: HANDLE, action: &str) -> io::Result<OwnedHandle> {
    if handle.is_null() {
        return Err(last_error(action));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn last_error(action: &str) -> io::Error {
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
    fn termination_owns_descendants_created_by_the_process() {
        let root = std::env::temp_dir().join(format!(
            "swawkit-process-job-{}-{}",
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
            "the process descendant survived Job termination"
        );
        drop(descendant);
        std::fs::remove_dir_all(root).unwrap();
    }
}
