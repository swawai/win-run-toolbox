use std::fs::{self, File, OpenOptions};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

pub(super) fn directory_chain(
    root: &Path,
    components: &[&str],
    subject: &str,
) -> Result<PathBuf, String> {
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
            return Err(format!("unsafe {subject} path segment '{component}'"));
        }
        path.push(component);
        regular_directory(&path, subject)?;
    }
    Ok(path)
}

pub(super) fn regular_directory(path: &Path, subject: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {subject} '{}': {error}", path.display()))?;
    if !metadata.is_dir() || is_reparse(&metadata) {
        return Err(format!(
            "{subject} must be a regular directory: {}",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn regular_file_length(path: &Path, subject: &str) -> Result<u64, String> {
    let file = open_regular(path, subject)?;
    file.metadata()
        .map(|metadata| metadata.len())
        .map_err(|error| format!("cannot inspect {subject} '{}': {error}", path.display()))
}

fn open_regular(path: &Path, subject: &str) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| format!("cannot open {subject} '{}': {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect {subject} '{}': {error}", path.display()))?;
    if !metadata.is_file() || is_reparse(&metadata) {
        return Err(format!(
            "{subject} must be a regular file: {}",
            path.display()
        ));
    }
    Ok(file)
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}
