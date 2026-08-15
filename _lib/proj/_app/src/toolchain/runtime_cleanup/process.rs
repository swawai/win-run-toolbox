use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use swawkit_proj::runtime_release::RUNTIME_ARTIFACT_NAMES;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

const ERROR_NO_MORE_FILES: u32 = 18;
const MAX_PROCESS_PATH: usize = 32_768;

pub(super) type InUseReleases = BTreeMap<String, Vec<u32>>;

pub(super) fn in_use_releases(releases_root: &Path) -> Result<InUseReleases, String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(last_error("cannot snapshot Runtime processes"));
    }
    let snapshot = Handle(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
        let code = unsafe { GetLastError() };
        return if code == ERROR_NO_MORE_FILES {
            Ok(BTreeMap::new())
        } else {
            Err(format!("cannot enumerate Runtime processes (Win32 {code})"))
        };
    }

    let mut result = BTreeMap::<String, Vec<u32>>::new();
    loop {
        if is_runtime_name(&entry.szExeFile) {
            let path = process_path(entry.th32ProcessID)?;
            if let Some(release_id) = release_id_from_path(&path, releases_root) {
                result
                    .entry(release_id)
                    .or_default()
                    .push(entry.th32ProcessID);
            }
        }
        if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
            let code = unsafe { GetLastError() };
            if code == ERROR_NO_MORE_FILES {
                break;
            }
            return Err(format!("cannot enumerate Runtime processes (Win32 {code})"));
        }
    }
    for pids in result.values_mut() {
        pids.sort_unstable();
        pids.dedup();
    }
    Ok(result)
}

fn process_path(process_id: u32) -> Result<PathBuf, String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return Err(last_error(&format!(
            "cannot inspect Runtime process PID {process_id}"
        )));
    }
    let process = Handle(process);
    let mut buffer = vec![0_u16; MAX_PROCESS_PATH];
    let mut length = buffer.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &mut length,
        )
    } == 0
    {
        return Err(last_error(&format!(
            "cannot inspect Runtime process PID {process_id}"
        )));
    }
    buffer.truncate(length as usize);
    Ok(PathBuf::from(OsString::from_wide(&buffer)))
}

fn is_runtime_name(value: &[u16]) -> bool {
    let length = value
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(value.len());
    let name = String::from_utf16_lossy(&value[..length]);
    RUNTIME_ARTIFACT_NAMES
        .iter()
        .any(|expected| name.eq_ignore_ascii_case(expected))
}

fn release_id_from_path(path: &Path, releases_root: &Path) -> Option<String> {
    let release_root = path.parent()?;
    if !same_path(release_root.parent()?, releases_root) {
        return None;
    }
    let release_id = release_root.file_name()?.to_str()?;
    is_release_id(release_id).then(|| release_id.to_owned())
}

fn is_release_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn same_path(left: &Path, right: &Path) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

fn last_error(message: &str) -> String {
    let code = unsafe { GetLastError() };
    format!("{message} (Win32 {code})")
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::{Child, Command};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn maps_only_exact_runtime_release_paths() {
        let root = PathBuf::from(r"C:\kit\_lib\proj\_bin\releases");
        let id = "a".repeat(64);
        assert_eq!(
            release_id_from_path(&root.join(&id).join("swawkit-proj-host.exe"), &root),
            Some(id)
        );
        assert!(
            release_id_from_path(&root.join("not-a-release").join("swawkit-proj.exe"), &root)
                .is_none()
        );
        assert!(
            release_id_from_path(
                &root.join("a".repeat(64)).join("nested/swawkit-proj.exe"),
                &root
            )
            .is_none()
        );
    }

    #[test]
    fn discovers_a_running_release_executable_by_its_mapped_path() {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-runtime-process-{}-{sequence}",
            std::process::id()
        ));
        let releases = root.join("releases");
        let release_id = "b".repeat(64);
        let release = releases.join(&release_id);
        fs::create_dir_all(&release).expect("create release directory");
        let executable = release.join("swawkit-proj-host.exe");
        fs::copy(r"C:\Windows\System32\cmd.exe", &executable).expect("copy fixture executable");
        let mut child = Command::new(&executable)
            .args(["/d", "/c", "ping -n 30 127.0.0.1 >nul"])
            .spawn()
            .expect("start fixture process");
        let process_id = child.id();
        let _cleanup = ProcessFixture {
            child: &mut child,
            root: &root,
        };

        let mapped = in_use_releases(&releases).expect("inspect processes");
        assert_eq!(mapped.get(&release_id), Some(&vec![process_id]));
    }

    struct ProcessFixture<'a> {
        child: &'a mut Child,
        root: &'a Path,
    }

    impl Drop for ProcessFixture<'_> {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = fs::remove_dir_all(self.root);
        }
    }
}
