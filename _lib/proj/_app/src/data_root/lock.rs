use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

const LOCK_FILE_NAME: &str = "_proj-entry.lock";
const DEFAULT_ATTEMPTS: usize = 100;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_millis(50);

pub(crate) struct DataRootLock {
    _file: File,
}

impl DataRootLock {
    pub(crate) fn acquire(data_directory: &Path) -> Result<Self, DataRootLockError> {
        Self::acquire_with(data_directory, DEFAULT_ATTEMPTS, DEFAULT_RETRY_DELAY)
    }

    fn acquire_with(
        data_directory: &Path,
        attempts: usize,
        retry_delay: Duration,
    ) -> Result<Self, DataRootLockError> {
        fs::create_dir_all(data_directory).map_err(|error| {
            DataRootLockError::new(format!(
                "cannot create project data directory '{}': {error}",
                data_directory.display()
            ))
        })?;
        let metadata = fs::symlink_metadata(data_directory).map_err(|error| {
            DataRootLockError::new(format!(
                "cannot inspect project data directory '{}': {error}",
                data_directory.display()
            ))
        })?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(DataRootLockError::new(format!(
                "project data directory cannot be a reparse point: {}",
                data_directory.display()
            )));
        }
        if !metadata.is_dir() {
            return Err(DataRootLockError::new(format!(
                "project data directory is not a directory: {}",
                data_directory.display()
            )));
        }

        let path = data_directory.join(LOCK_FILE_NAME);
        let attempts = attempts.max(1);
        for attempt in 0..attempts {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .share_mode(0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&path)
            {
                Ok(file) => {
                    let metadata = file.metadata().map_err(|error| {
                        DataRootLockError::new(format!(
                            "cannot inspect the opened project DataRoot lock '{}': {error}",
                            path.display()
                        ))
                    })?;
                    if !metadata.is_file()
                        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                    {
                        return Err(DataRootLockError::new(format!(
                            "project DataRoot lock must be a regular file: {}",
                            path.display()
                        )));
                    }
                    return Ok(Self { _file: file });
                }
                Err(_) if attempt + 1 < attempts => thread::sleep(retry_delay),
                Err(error) => {
                    return Err(DataRootLockError::new(format!(
                        "timed out waiting for the project DataRoot lock '{}': {error}",
                        path.display()
                    )));
                }
            }
        }
        unreachable!("at least one lock attempt is required")
    }

    #[cfg(test)]
    pub(crate) fn acquire_for_test(
        data_directory: &Path,
        attempts: usize,
        retry_delay: Duration,
    ) -> Result<Self, DataRootLockError> {
        Self::acquire_with(data_directory, attempts, retry_delay)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DataRootLockError {
    message: String,
}

impl DataRootLockError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for DataRootLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for DataRootLockError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn a_stale_lock_file_is_harmless_but_a_live_handle_is_exclusive() {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-data-lock-{}-{sequence}",
            std::process::id()
        ));
        let first = DataRootLock::acquire_for_test(&root, 1, Duration::ZERO)
            .expect("first lock");
        assert!(root.join(LOCK_FILE_NAME).is_file());
        assert!(DataRootLock::acquire_for_test(&root, 2, Duration::from_millis(1)).is_err());

        drop(first);
        assert!(first_lock_file_remains(&root));
        DataRootLock::acquire_for_test(&root, 1, Duration::ZERO)
            .expect("reacquire stale lock file");

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn a_reparse_lock_file_is_never_followed() {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-data-lock-reparse-{}-{sequence}",
            std::process::id()
        ));
        let target = root.join("external.lock");
        let lock_path = root.join(LOCK_FILE_NAME);
        fs::create_dir(&root).expect("create reparse lock fixture");
        fs::write(&target, b"external").expect("create reparse target");
        if let Err(error) = std::os::windows::fs::symlink_file(&target, &lock_path) {
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                fs::remove_dir_all(root).expect("remove unsupported reparse fixture");
                return;
            }
            panic!("create reparse lock: {error}");
        }

        let error = match DataRootLock::acquire_for_test(&root, 1, Duration::ZERO) {
            Ok(lock) => {
                drop(lock);
                panic!("a reparse lock file must be rejected");
            }
            Err(error) => error,
        };
        assert!(error.to_string().contains("must be a regular file"));
        assert_eq!(fs::read(&target).expect("read untouched target"), b"external");

        fs::remove_file(lock_path).expect("remove reparse lock");
        fs::remove_dir_all(root).expect("remove reparse lock fixture");
    }

    fn first_lock_file_remains(root: &Path) -> bool {
        root.join(LOCK_FILE_NAME).is_file()
    }
}
