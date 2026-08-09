use std::fs::{self, File, OpenOptions};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};
use windows_sys::Win32::System::Com::CoCreateGuid;
use windows_sys::core::GUID;

use super::ProfileError;
use crate::atomic_file;
use crate::data_root::DataRootLock;

const PROVIDER_DIRECTORY_COMPONENTS: [&str; 4] = ["modules", "kernel", ".dev", "setup"];
const LOCKS_DIRECTORY_NAME: &str = "locks";
const STATE_LOCK_FILE_NAME: &str = "state.lock";
const STATE_FILE_NAME: &str = "_state.json";
const STATE_SCHEMA: &str = "swawkit.command-provider-state/v1";
const LOCK_ATTEMPTS: usize = 100;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(50);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UnavailableProviderState<'a> {
    schema: &'static str,
    status: &'static str,
    input_revision: &'a str,
    token: String,
}

pub(super) struct ProviderStateTransaction {
    state_path: PathBuf,
    previous: Option<Vec<u8>>,
    committed: bool,
    _state_lock: File,
}

impl ProviderStateTransaction {
    pub(super) fn commit(mut self) {
        self.committed = true;
    }

    pub(super) fn rollback(mut self) -> Result<(), ProfileError> {
        let result = self.restore_previous();
        if result.is_ok() {
            self.committed = true;
        }
        result
    }

    fn restore_previous(&self) -> Result<(), ProfileError> {
        validate_regular_file_or_missing(&self.state_path, "command provider state")?;
        match &self.previous {
            Some(content) => atomic_file::publish(&self.state_path, content).map_err(|error| {
                ProfileError::new(format!(
                    "cannot restore command provider state '{}': {error}",
                    self.state_path.display()
                ))
            }),
            None => match fs::remove_file(&self.state_path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(ProfileError::new(format!(
                    "cannot remove uncommitted command provider state '{}': {error}",
                    self.state_path.display()
                ))),
            },
        }
    }
}

impl Drop for ProviderStateTransaction {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.restore_previous();
        }
    }
}

pub(super) fn begin_unavailable(
    data_root: &Path,
    input_revision: &str,
    _data_root_lock: &DataRootLock,
) -> Result<ProviderStateTransaction, ProfileError> {
    let provider_root = ensure_provider_root(data_root)?;
    let locks_root = ensure_regular_child_directory(&provider_root, LOCKS_DIRECTORY_NAME)?;
    let state_lock = acquire_state_lock(&locks_root)?;
    let state_path = provider_root.join(STATE_FILE_NAME);
    validate_regular_file_or_missing(&state_path, "command provider state")?;
    let previous = match fs::read(&state_path) {
        Ok(content) => Some(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(ProfileError::new(format!(
                "cannot read command provider state '{}': {error}",
                state_path.display()
            )));
        }
    };

    let state = UnavailableProviderState {
        schema: STATE_SCHEMA,
        status: "unavailable",
        input_revision,
        token: fresh_token()?,
    };
    let mut content = serde_json::to_vec_pretty(&state).map_err(|error| {
        ProfileError::new(format!("cannot serialize command provider state: {error}"))
    })?;
    content.push(b'\n');
    atomic_file::publish(&state_path, &content).map_err(|error| {
        ProfileError::new(format!(
            "cannot publish command provider state '{}': {error}",
            state_path.display()
        ))
    })?;
    Ok(ProviderStateTransaction {
        state_path,
        previous,
        committed: false,
        _state_lock: state_lock,
    })
}

fn ensure_provider_root(data_root: &Path) -> Result<PathBuf, ProfileError> {
    validate_regular_directory(data_root, "Entry DataRoot")?;
    let mut current = data_root.to_path_buf();
    for component in PROVIDER_DIRECTORY_COMPONENTS {
        current = ensure_regular_child_directory(&current, component)?;
    }
    Ok(current)
}

fn ensure_regular_child_directory(parent: &Path, name: &str) -> Result<PathBuf, ProfileError> {
    let path = parent.join(name);
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(ProfileError::new(format!(
                "cannot create command provider directory '{}': {error}",
                path.display()
            )));
        }
    }
    validate_regular_directory(&path, "command provider directory")?;
    Ok(path)
}

fn validate_regular_directory(path: &Path, subject: &str) -> Result<(), ProfileError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        ProfileError::new(format!(
            "cannot inspect {subject} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ProfileError::new(format!(
            "{subject} must be a regular directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn validate_regular_file_or_missing(path: &Path, subject: &str) -> Result<(), ProfileError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(ProfileError::new(format!(
                "cannot inspect {subject} '{}': {error}",
                path.display()
            )));
        }
    };
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ProfileError::new(format!(
            "{subject} must be a regular file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn acquire_state_lock(locks_root: &Path) -> Result<File, ProfileError> {
    let path = locks_root.join(STATE_LOCK_FILE_NAME);
    validate_regular_file_or_missing(&path, "command provider state lock")?;
    for attempt in 0..LOCK_ATTEMPTS {
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
                    ProfileError::new(format!(
                        "cannot inspect opened command provider state lock '{}': {error}",
                        path.display()
                    ))
                })?;
                if !metadata.is_file()
                    || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                {
                    return Err(ProfileError::new(format!(
                        "command provider state lock must be a regular file: {}",
                        path.display()
                    )));
                }
                return Ok(file);
            }
            Err(_) if attempt + 1 < LOCK_ATTEMPTS => thread::sleep(LOCK_RETRY_DELAY),
            Err(error) => {
                return Err(ProfileError::new(format!(
                    "timed out waiting for command provider state lock '{}': {error}",
                    path.display()
                )));
            }
        }
    }
    unreachable!("at least one state lock attempt is required")
}

fn fresh_token() -> Result<String, ProfileError> {
    let mut guid = GUID::default();
    // SAFETY: `guid` is valid writable storage for one GUID and outlives this call.
    let result = unsafe { CoCreateGuid(&mut guid) };
    if result < 0 {
        return Err(ProfileError::new(format!(
            "cannot create command provider state token: HRESULT 0x{:08x}",
            result as u32
        )));
    }
    Ok(format!(
        "{:08x}{:04x}{:04x}{}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}

#[cfg(test)]
pub(super) fn state_path(data_root: &Path) -> PathBuf {
    PROVIDER_DIRECTORY_COMPONENTS
        .iter()
        .fold(data_root.to_path_buf(), |path, component| {
            path.join(component)
        })
        .join(STATE_FILE_NAME)
}
