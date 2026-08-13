use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

pub(super) struct RunOwnerLease {
    file: Option<File>,
    path: PathBuf,
}

pub(super) enum OwnerLeaseState {
    Active,
    Legacy,
    Acquired(RunOwnerLease),
}

impl RunOwnerLease {
    pub(super) fn create(journals_root: &Path, id: &str) -> io::Result<Self> {
        let path = owner_path(journals_root, id);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)?;
        if let Err(error) = validate(&file, &path).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        Ok(Self {
            file: Some(file),
            path,
        })
    }

    pub(super) fn try_acquire(run_root: &Path) -> io::Result<OwnerLeaseState> {
        let journals_root = run_root
            .parent()
            .ok_or_else(|| io::Error::other("run journal directory has no journals parent"))?;
        let id = run_root
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| io::Error::other("run journal directory has no Unicode id"))?;
        let path = owner_path(journals_root, id);
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(OwnerLeaseState::Legacy);
            }
            Err(error) if is_owned(error.raw_os_error()) => {
                return Ok(OwnerLeaseState::Active);
            }
            Err(error) => return Err(error),
        };
        validate(&file, &path)?;
        Ok(OwnerLeaseState::Acquired(Self {
            file: Some(file),
            path,
        }))
    }

    pub(super) fn release(mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(super) fn remove_owner_file(journals_root: &Path, id: &str) {
    let _ = std::fs::remove_file(owner_path(journals_root, id));
}

fn owner_path(journals_root: &Path, id: &str) -> PathBuf {
    journals_root.join(format!(".{id}.owner.lock"))
}

fn validate(file: &File, path: &Path) -> io::Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other(format!(
            "run journal owner lease must be a normal file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn is_owned(code: Option<i32>) -> bool {
    matches!(
        code,
        Some(value)
            if value == ERROR_SHARING_VIOLATION as i32
                || value == ERROR_LOCK_VIOLATION as i32
    )
}
