use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

const LOCK_TIMEOUT: Duration = Duration::from_secs(120);
const RETRY_INTERVAL: Duration = Duration::from_millis(200);
const ERROR_SHARING_VIOLATION: i32 = 32;
const ERROR_LOCK_VIOLATION: i32 = 33;

pub(super) struct PublicationLock {
    _file: File,
}

impl PublicationLock {
    pub(super) fn acquire(swawkit_home: &Path) -> Result<Self, String> {
        let locks = ensure_lock_directory(swawkit_home)?;
        let path = locks.join("release-publish.lock");
        let started = Instant::now();
        loop {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .share_mode(0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&path)
            {
                Ok(file) => {
                    let metadata = file.metadata().map_err(|error| storage(&path, error))?;
                    if !metadata.is_file() || is_reparse(&metadata) {
                        return Err(format!(
                            "Runtime publication lock must be a regular file: {}",
                            path.display()
                        ));
                    }
                    return Ok(Self { _file: file });
                }
                Err(error) if lock_conflict(&error) && started.elapsed() < LOCK_TIMEOUT => {
                    thread::sleep(RETRY_INTERVAL);
                }
                Err(error) if lock_conflict(&error) => {
                    return Err(format!(
                        "Runtime publication lock is busy after {} seconds: {}",
                        LOCK_TIMEOUT.as_secs(),
                        path.display()
                    ));
                }
                Err(error) => return Err(storage(&path, error)),
            }
        }
    }
}

fn ensure_lock_directory(swawkit_home: &Path) -> Result<PathBuf, String> {
    let mut current = swawkit_home.to_path_buf();
    for segment in ["data", "proj_cache", "locks"] {
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !is_reparse(&metadata) => {}
            Ok(_) => {
                return Err(format!(
                    "Runtime lock path must be a regular directory: {}",
                    current.display()
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        let metadata = fs::symlink_metadata(&current)
                            .map_err(|error| storage(&current, error))?;
                        if !metadata.is_dir() || is_reparse(&metadata) {
                            return Err(format!(
                                "Runtime lock path must be a regular directory: {}",
                                current.display()
                            ));
                        }
                    }
                    Err(error) => return Err(storage(&current, error)),
                }
            }
            Err(error) => return Err(storage(&current, error)),
        }
    }
    Ok(current)
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn lock_conflict(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(ERROR_SHARING_VIOLATION) | Some(ERROR_LOCK_VIOLATION)
    )
}

fn storage(path: &Path, error: io::Error) -> String {
    format!("cannot access Runtime lock '{}': {error}", path.display())
}
