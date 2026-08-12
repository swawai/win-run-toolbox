use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION};
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

use super::super::ArchiveToolError;
use super::super::ArchiveToolErrorKind;
use super::removal::{is_reparse, require_regular_directory, storage, unsafe_path};

/// An advisory lock backed by a live Windows file handle with no sharing.
///
/// The lock file intentionally remains after the handle is dropped. Its
/// existence carries no state; only the live handle owns the lock.
pub(in super::super) struct ExclusiveFileLock {
    _file: File,
}

impl ExclusiveFileLock {
    pub(in super::super) fn acquire(
        path: &Path,
        attempts: usize,
        delay: Duration,
    ) -> Result<Self, ArchiveToolError> {
        let parent = path.parent().ok_or_else(|| {
            ArchiveToolError::new(
                ArchiveToolErrorKind::UnsafeStorage,
                format!("installation lock has no parent: {}", path.display()),
            )
        })?;
        require_regular_directory(parent, "installation lock directory")?;

        let attempts = attempts.max(1);
        for attempt in 0..attempts {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .share_mode(0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)
            {
                Ok(file) => {
                    let metadata = file.metadata().map_err(|error| {
                        storage("inspect the opened installation lock", path, error)
                    })?;
                    if !metadata.is_file() || is_reparse(&metadata) {
                        return Err(unsafe_path("installation lock", path));
                    }
                    return Ok(Self { _file: file });
                }
                Err(error) if is_lock_contention(&error) && attempt + 1 < attempts => {
                    thread::sleep(delay)
                }
                Err(error) if is_lock_contention(&error) => {
                    return Err(ArchiveToolError::new(
                        ArchiveToolErrorKind::LockUnavailable,
                        format!(
                            "timed out waiting for installation lock '{}': {error}",
                            path.display()
                        ),
                    ));
                }
                Err(error) => return Err(storage("open installation lock", path, error)),
            }
        }
        unreachable!("at least one lock attempt is required")
    }
}

fn is_lock_contention(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code)
            if code == ERROR_SHARING_VIOLATION as i32
                || code == ERROR_LOCK_VIOLATION as i32
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "swawkit-archive-lock-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("create lock fixture");
        root
    }

    #[test]
    fn lock_ownership_is_the_live_exclusive_handle() {
        let root = fixture();
        let path = root.join("install.lock");
        let first = ExclusiveFileLock::acquire(&path, 1, Duration::ZERO).expect("first lock");
        let error = match ExclusiveFileLock::acquire(&path, 1, Duration::ZERO) {
            Ok(_) => panic!("a live exclusive handle must block another owner"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), ArchiveToolErrorKind::LockUnavailable);
        drop(first);
        ExclusiveFileLock::acquire(&path, 1, Duration::ZERO).expect("reacquire stale lock file");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn access_denied_is_not_treated_as_lock_contention() {
        assert!(!is_lock_contention(&io::Error::from_raw_os_error(5)));
        assert!(is_lock_contention(&io::Error::from_raw_os_error(
            ERROR_SHARING_VIOLATION as i32
        )));
    }
}
