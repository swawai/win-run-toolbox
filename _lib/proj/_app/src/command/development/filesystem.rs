use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

use super::super::{CommandError, CommandResult};

pub(super) fn directory_chain(
    root: &Path,
    components: &[&str],
    subject: &str,
) -> CommandResult<PathBuf> {
    regular_directory(root, subject)?;
    let mut path = root.to_path_buf();
    for component in components {
        if !matches!(
            Path::new(component)
                .components()
                .collect::<Vec<_>>()
                .as_slice(),
            [Component::Normal(_)]
        ) {
            return Err(CommandError::new(format!(
                "unsafe {subject} path segment '{component}'"
            )));
        }
        path.push(component);
        regular_directory(&path, subject)?;
    }
    Ok(path)
}

pub(super) fn read_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    subject: &str,
    max_bytes: u64,
) -> CommandResult<T> {
    let mut file = open_regular_at_most(path, subject, max_bytes)?;
    let mut content = Vec::with_capacity(
        file.metadata()
            .map(|value| value.len() as usize)
            .unwrap_or(0),
    );
    file.read_to_end(&mut content).map_err(|error| {
        CommandError::new(format!(
            "cannot read {subject} '{}': {error}",
            path.display()
        ))
    })?;
    serde_json::from_slice(&content).map_err(|error| {
        CommandError::new(format!(
            "cannot parse {subject} '{}': {error}",
            path.display()
        ))
    })
}

pub(super) fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn regular_directory(path: &Path, subject: &str) -> CommandResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        CommandError::new(format!(
            "cannot inspect {subject} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(CommandError::new(format!(
            "{subject} must be a regular directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn open_regular_at_most(path: &Path, subject: &str, max_bytes: u64) -> CommandResult<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| {
            CommandError::new(format!(
                "cannot open {subject} '{}': {error}",
                path.display()
            ))
        })?;
    let metadata = file.metadata().map_err(|error| {
        CommandError::new(format!(
            "cannot inspect {subject} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() > max_bytes
    {
        return Err(CommandError::new(format!(
            "{subject} must be a bounded regular file: {}",
            path.display()
        )));
    }
    Ok(file)
}
