use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle};
use std::path::Path;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_FAILED,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION,
    ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject,
};

use super::{RustDefinition, RustError, RustErrorKind, VerifiedRustup, error};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const AMBIENT_OVERRIDES: [&str; 6] = [
    "RUSTUP_TOOLCHAIN",
    "RUSTUP_TOOLCHAIN_SOURCE",
    "RUSTUP_DIST_SERVER",
    "RUSTUP_DIST_ROOT",
    "RUSTUP_UPDATE_ROOT",
    "RUSTUP_VERSION",
];

pub(super) fn run_installer(
    definition: &RustDefinition,
    installer: &VerifiedRustup,
    stage: &Path,
) -> Result<(), RustError> {
    for relative in ["cargo", "rustup"] {
        let root = stage.join(relative);
        let mut entries = std::fs::read_dir(&root).map_err(|cause| {
            process_error(format!(
                "cannot inspect Rust staging directory '{}': {cause}",
                root.display()
            ))
        })?;
        if entries.next().is_some() {
            return Err(process_error(format!(
                "Rust staging root is not clean: {}",
                root.display()
            )));
        }
    }
    let arguments = [
        "-y".to_owned(),
        "--default-host".to_owned(),
        definition.host().to_owned(),
        "--no-modify-path".to_owned(),
        "--profile".to_owned(),
        definition.profile().to_owned(),
        "--default-toolchain".to_owned(),
        definition.toolchain().to_owned(),
        "--component".to_owned(),
        "rustfmt".to_owned(),
    ];
    let environment = isolated_environment(stage)?;
    let mut process = SuspendedProcess::create(installer.path(), &arguments, stage, &environment)?;
    let job = KillOnCloseJob::create()?;
    job.assign(process.process())?;
    process.resume()?;
    let result = process.wait(INSTALL_TIMEOUT);
    match result {
        Ok(0) => Ok(()),
        Ok(exit_code) => Err(process_error(format!(
            "rustup-init exited with code {exit_code}."
        ))),
        Err(failure) => {
            let _ = job.terminate();
            Err(failure)
        }
    }
}

fn isolated_environment(stage: &Path) -> Result<Vec<u16>, RustError> {
    let mut variables = BTreeMap::<String, (OsString, OsString)>::new();
    for (name, value) in std::env::vars_os() {
        let key = name.to_string_lossy().to_ascii_uppercase();
        if !AMBIENT_OVERRIDES.iter().any(|item| *item == key) {
            variables.insert(key, (name, value));
        }
    }
    for (name, value) in [
        ("CARGO_HOME", stage.join("cargo").into_os_string()),
        ("RUSTUP_HOME", stage.join("rustup").into_os_string()),
        ("RUSTUP_INIT_SKIP_EXISTENCE_CHECKS", OsString::from("yes")),
    ] {
        variables.insert(name.to_owned(), (OsString::from(name), value));
    }
    variables.remove("RUSTUP_INIT_SKIP_PATH_CHECK");
    let mut block = Vec::new();
    for (_, (name, value)) in variables {
        push_wide(&mut block, &name)?;
        block.push('=' as u16);
        push_wide(&mut block, &value)?;
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

fn push_wide(buffer: &mut Vec<u16>, value: &OsStr) -> Result<(), RustError> {
    for unit in value.encode_wide() {
        if unit == 0 {
            return Err(process_error("Rust process environment contains NUL"));
        }
        buffer.push(unit);
    }
    Ok(())
}

struct SuspendedProcess {
    process: OwnedHandle,
    thread: Option<OwnedHandle>,
    completed: bool,
}

impl SuspendedProcess {
    fn create(
        executable: &Path,
        arguments: &[String],
        working_directory: &Path,
        environment: &[u16],
    ) -> Result<Self, RustError> {
        let application = wide_null(executable.as_os_str())?;
        let mut command_line = encode_command_line(executable.as_os_str(), arguments)?;
        let working_directory = wide_null(working_directory.as_os_str())?;
        let inherited_handles = StandardHandles::new()?;
        let mut inherited = inherited_handles.raw();
        let attributes = HandleList::new(&mut inherited)?;
        let mut startup = STARTUPINFOEXW {
            StartupInfo: windows_sys::Win32::System::Threading::STARTUPINFOW {
                cb: size_of::<STARTUPINFOEXW>() as u32,
                dwFlags: STARTF_USESTDHANDLES,
                hStdInput: inherited[0],
                hStdOutput: inherited[1],
                hStdError: inherited[2],
                ..Default::default()
            },
            lpAttributeList: attributes.pointer,
        };
        let mut information = PROCESS_INFORMATION::default();
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
                CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                environment.as_ptr().cast(),
                working_directory.as_ptr(),
                &mut startup.StartupInfo,
                &mut information,
            )
        };
        if created == 0 {
            return Err(last_error("cannot start rustup-init"));
        }
        Ok(Self {
            process: owned(information.hProcess),
            thread: Some(owned(information.hThread)),
            completed: false,
        })
    }

    fn process(&self) -> HANDLE {
        self.process.as_raw_handle() as HANDLE
    }

    fn resume(&mut self) -> Result<(), RustError> {
        let thread = self.thread.take().expect("suspended thread");
        if unsafe { ResumeThread(thread.as_raw_handle() as HANDLE) } == u32::MAX {
            return Err(last_error("cannot resume rustup-init"));
        }
        drop(thread);
        Ok(())
    }

    fn wait(&mut self, timeout: Duration) -> Result<u32, RustError> {
        let milliseconds = timeout.as_millis().min(u32::MAX as u128) as u32;
        match unsafe { WaitForSingleObject(self.process(), milliseconds) } {
            WAIT_OBJECT_0 => {
                self.completed = true;
                let mut exit_code = 0;
                if unsafe { GetExitCodeProcess(self.process(), &mut exit_code) } == 0 {
                    return Err(last_error("cannot read rustup-init exit code"));
                }
                Ok(exit_code)
            }
            WAIT_TIMEOUT => Err(process_error(format!(
                "rustup-init timed out after {} minutes.",
                timeout.as_secs() / 60
            ))),
            WAIT_FAILED => Err(last_error("cannot wait for rustup-init")),
            value => Err(process_error(format!(
                "unexpected rustup-init wait result: {value}"
            ))),
        }
    }
}

struct StandardHandles([OwnedHandle; 3]);

impl StandardHandles {
    fn new() -> Result<Self, RustError> {
        Ok(Self([
            inheritable_standard_handle(STD_INPUT_HANDLE)?,
            inheritable_standard_handle(STD_OUTPUT_HANDLE)?,
            inheritable_standard_handle(STD_ERROR_HANDLE)?,
        ]))
    }

    fn raw(&self) -> [HANDLE; 3] {
        self.0
            .each_ref()
            .map(|handle| handle.as_raw_handle() as HANDLE)
    }
}

fn inheritable_standard_handle(kind: u32) -> Result<OwnedHandle, RustError> {
    let source = unsafe { GetStdHandle(kind) };
    if source.is_null() || source == INVALID_HANDLE_VALUE {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("NUL")
            .map_err(|cause| {
                process_error(format!("cannot open a fallback standard handle: {cause}"))
            })?;
        let file = unsafe { OwnedHandle::from_raw_handle(file.into_raw_handle()) };
        return duplicate_inheritable(file.as_raw_handle() as HANDLE);
    }
    duplicate_inheritable(source)
}

fn duplicate_inheritable(source: HANDLE) -> Result<OwnedHandle, RustError> {
    let process = unsafe { GetCurrentProcess() };
    let mut duplicate = std::ptr::null_mut();
    if unsafe {
        DuplicateHandle(
            process,
            source,
            process,
            &mut duplicate,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(last_error("cannot duplicate a Rustup standard handle"));
    }
    Ok(owned(duplicate))
}

impl Drop for SuspendedProcess {
    fn drop(&mut self) {
        if !self.completed {
            unsafe {
                TerminateProcess(self.process(), 1);
                WaitForSingleObject(self.process(), 5_000);
            }
        }
    }
}

struct HandleList {
    storage: Vec<u8>,
    pointer: *mut core::ffi::c_void,
}

impl HandleList {
    fn new(handles: &mut [HANDLE]) -> Result<Self, RustError> {
        let mut size = 0usize;
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size) };
        if size == 0 {
            return Err(last_error("cannot size the rustup handle list"));
        }
        let mut storage = vec![0u8; size];
        let pointer = storage.as_mut_ptr().cast();
        if unsafe { InitializeProcThreadAttributeList(pointer, 1, 0, &mut size) } == 0 {
            return Err(last_error("cannot initialize the rustup handle list"));
        }
        if unsafe {
            UpdateProcThreadAttribute(
                pointer,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                std::mem::size_of_val(handles),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        } == 0
        {
            unsafe { DeleteProcThreadAttributeList(pointer) };
            return Err(last_error("cannot restrict rustup inherited handles"));
        }
        Ok(Self { storage, pointer })
    }
}

impl Drop for HandleList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.pointer) };
        std::hint::black_box(&self.storage);
    }
}

struct KillOnCloseJob(OwnedHandle);

impl KillOnCloseJob {
    fn create() -> Result<Self, RustError> {
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(last_error("cannot create the rustup Job Object"));
        }
        let handle = owned(handle);
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
            return Err(last_error("cannot configure the rustup Job Object"));
        }
        Ok(Self(handle))
    }

    fn assign(&self, process: HANDLE) -> Result<(), RustError> {
        if unsafe { AssignProcessToJobObject(self.0.as_raw_handle() as HANDLE, process) } == 0 {
            return Err(last_error("cannot assign rustup-init to its Job Object"));
        }
        Ok(())
    }

    fn terminate(&self) -> Result<(), RustError> {
        if unsafe { TerminateJobObject(self.0.as_raw_handle() as HANDLE, 1) } == 0 {
            return Err(last_error("cannot terminate the rustup process tree"));
        }
        Ok(())
    }
}

fn encode_command_line(executable: &OsStr, arguments: &[String]) -> Result<Vec<u16>, RustError> {
    let mut value = quote_windows(executable)?;
    for argument in arguments {
        value.push(' ' as u16);
        value.extend(quote_windows(OsStr::new(argument))?);
    }
    value.push(0);
    Ok(value)
}

fn quote_windows(value: &OsStr) -> Result<Vec<u16>, RustError> {
    let value = value.encode_wide().collect::<Vec<_>>();
    if value.contains(&0) {
        return Err(process_error("Rust process argument contains NUL"));
    }
    let mut encoded = vec!['"' as u16];
    let mut backslashes = 0;
    for unit in value {
        if unit == '\\' as u16 {
            backslashes += 1;
        } else {
            if unit == '"' as u16 {
                encoded.extend(std::iter::repeat_n('\\' as u16, backslashes * 2 + 1));
            } else {
                encoded.extend(std::iter::repeat_n('\\' as u16, backslashes));
            }
            encoded.push(unit);
            backslashes = 0;
        }
    }
    encoded.extend(std::iter::repeat_n('\\' as u16, backslashes * 2));
    encoded.push('"' as u16);
    Ok(encoded)
}

fn wide_null(value: &OsStr) -> Result<Vec<u16>, RustError> {
    let mut value = value.encode_wide().collect::<Vec<_>>();
    if value.contains(&0) {
        return Err(process_error("Rust process path contains NUL"));
    }
    value.push(0);
    Ok(value)
}

fn owned(handle: HANDLE) -> OwnedHandle {
    debug_assert!(!handle.is_null());
    unsafe { OwnedHandle::from_raw_handle(handle) }
}

fn last_error(action: &str) -> RustError {
    let cause = std::io::Error::last_os_error();
    process_error(format!("{action}: {cause}"))
}

fn process_error(message: impl Into<String>) -> RustError {
    error(RustErrorKind::InstallationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_arguments_follow_command_line_to_argvw_rules() {
        let encoded = quote_windows(OsStr::new("a b")).unwrap();
        assert_eq!(String::from_utf16(&encoded).unwrap(), r#""a b""#);
        let trailing = quote_windows(OsStr::new("a b\\")).unwrap();
        assert_eq!(String::from_utf16(&trailing).unwrap(), r#""a b\\""#);
    }

    #[test]
    fn isolated_environment_replaces_rustup_state() {
        let stage = Path::new(r"C:\fixture rust");
        let block = isolated_environment(stage).unwrap();
        let text = String::from_utf16(&block).unwrap();
        assert!(text.contains("CARGO_HOME=C:\\fixture rust\\cargo\0"));
        assert!(text.contains("RUSTUP_HOME=C:\\fixture rust\\rustup\0"));
        assert!(text.contains("RUSTUP_INIT_SKIP_EXISTENCE_CHECKS=yes\0"));
    }
}
