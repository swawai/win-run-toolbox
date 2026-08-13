use std::env;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

const REVISION_PREFIX: &str = "sha256-";

pub(super) struct CommandContext {
    pub(super) swawkit_home: PathBuf,
    pub(super) data_root: PathBuf,
    pub(super) export_root: PathBuf,
    pub(super) entry_command: String,
    pub(super) environment_input_revision: String,
}

pub(super) struct SetupCommandContext {
    pub(super) cache_data_root: PathBuf,
    pub(super) profile_revision: String,
}

impl CommandContext {
    pub(super) fn from_environment(handler: &str) -> Result<Self, String> {
        require_exact("SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL", "1")?;
        require_exact("SWAWKIT_PROJ_CORE_COMMAND_PHASE", "run")?;
        let expected_address = match handler {
            "dev.setup" => ".dev.setup",
            "dev.status" => ".dev.status",
            "runtime.cleanup" => ".runtime.cleanup",
            _ => return Err(format!("unsupported Toolchain command handler '{handler}'")),
        };
        require_exact("SWAWKIT_PROJ_CORE_COMMAND_ADDRESS", expected_address)?;

        let swawkit_home = absolute_path(required("SWAWKIT_HOME")?, "Swaw Kit Home")?;
        regular_directory(&swawkit_home, "Swaw Kit Home")?;
        let data_root = absolute_path(required("SWAWKIT_PROJ_DATA_ROOT")?, "Entry DataRoot")?;
        readable_data_root(&data_root)?;
        let entry_command = required("SWAWKIT_PROJ_ENTRY_COMMAND")?;
        let environment_input_revision =
            required("SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION")?;
        if !is_revision(&environment_input_revision) {
            return Err("invalid command environment input revision".to_owned());
        }

        let setup_root = data_root
            .join("modules")
            .join("kernel")
            .join(".dev")
            .join("setup");
        let export_root = setup_root.join("export");
        Ok(Self {
            swawkit_home,
            data_root,
            export_root,
            entry_command,
            environment_input_revision,
        })
    }

    pub(super) fn repair_invocation(&self) -> String {
        format!("{} .dev.setup", self.entry_command)
    }

    pub(super) fn environment(&self, name: &str) -> String {
        env::var(name).unwrap_or_default().trim().to_owned()
    }
}

impl SetupCommandContext {
    pub(super) fn from_environment() -> Result<Self, String> {
        let swawkit_home = absolute_path(required("SWAWKIT_HOME")?, "Swaw Kit Home")?;
        regular_directory(&swawkit_home, "Swaw Kit Home")?;
        let profile_revision = required("SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION")?;
        if !is_revision(&profile_revision) {
            return Err("invalid command Profile revision".to_owned());
        }
        Ok(Self {
            cache_data_root: swawkit_home.join("data").join("proj_cache"),
            profile_revision,
        })
    }
}

fn required(name: &str) -> Result<String, String> {
    let value =
        env::var(name).map_err(|_| format!("required environment variable is missing: {name}"))?;
    if value.is_empty() || value.trim() != value {
        return Err(format!("required environment variable is invalid: {name}"));
    }
    Ok(value)
}

fn require_exact(name: &str, expected: &str) -> Result<(), String> {
    let actual = required(name)?;
    if actual != expected {
        return Err(format!(
            "unsupported {name} value '{actual}'; expected '{expected}'"
        ));
    }
    Ok(())
}

fn absolute_path(value: String, subject: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(format!("{subject} must be absolute: {}", path.display()));
    }
    Ok(path)
}

fn regular_directory(path: &Path, subject: &str) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {subject} '{}': {error}", path.display()))?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "{subject} must be a regular directory: {}",
            path.display()
        ));
    }
    Ok(())
}

fn readable_data_root(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(format!(
                    "Entry DataRoot must be a regular directory: {}",
                    path.display()
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| "Entry DataRoot has no parent directory".to_owned())?;
            regular_directory(parent, "Entry DataRoot parent")
        }
        Err(error) => Err(format!(
            "cannot inspect Entry DataRoot '{}': {error}",
            path.display()
        )),
    }
}

fn is_revision(value: &str) -> bool {
    value.len() == REVISION_PREFIX.len() + 64
        && value.starts_with(REVISION_PREFIX)
        && value[REVISION_PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
