use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

pub(super) const MAX_STATE_BYTES: u64 = 16 * 1024;
pub(super) const MAX_METADATA_BYTES: u64 = 4 * 1024 * 1024;

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

pub(super) fn child_file(root: &Path, relative: &str, subject: &str) -> Result<PathBuf, String> {
    regular_directory(root, subject)?;
    let relative_path = Path::new(relative);
    let components: Vec<_> = relative_path.components().collect();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("unsafe {subject} relative path '{relative}'"));
    }
    let mut path = root.to_path_buf();
    for component in &components[..components.len() - 1] {
        let Component::Normal(name) = component else {
            unreachable!()
        };
        path.push(name);
        regular_directory(&path, subject)?;
    }
    let Component::Normal(name) = components[components.len() - 1] else {
        unreachable!()
    };
    path.push(name);
    Ok(path)
}

pub(super) fn read_json<T: for<'de> Deserialize<'de>>(
    path: &Path,
    subject: &str,
    max_bytes: u64,
) -> Result<T, String> {
    let mut file = open_regular_at_most(path, subject, max_bytes)?;
    let mut content = Vec::with_capacity(
        file.metadata()
            .map(|value| value.len() as usize)
            .unwrap_or(0),
    );
    file.read_to_end(&mut content)
        .map_err(|error| format!("cannot read {subject} '{}': {error}", path.display()))?;
    serde_json::from_slice(&content)
        .map_err(|error| format!("cannot parse {subject} '{}': {error}", path.display()))
}

pub(super) fn regular_file_length(path: &Path, subject: &str) -> Result<u64, String> {
    let file = open_regular_at_most(path, subject, u64::MAX)?;
    file.metadata()
        .map(|metadata| metadata.len())
        .map_err(|error| format!("cannot inspect {subject} '{}': {error}", path.display()))
}

pub(super) fn sha256_regular(path: &Path, subject: &str) -> Result<String, String> {
    let file = open_regular_at_most(path, subject, u64::MAX)?;
    sha256_reader(file, path, subject)
}

pub(super) fn sha256_follow(path: &Path, subject: &str) -> Result<String, String> {
    let file = File::open(path)
        .map_err(|error| format!("cannot open {subject} '{}': {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect {subject} '{}': {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!(
            "{subject} target is not a non-empty file: {}",
            path.display()
        ));
    }
    sha256_reader(file, path, subject)
}

fn sha256_reader(file: File, path: &Path, subject: &str) -> Result<String, String> {
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    std::io::copy(&mut reader, &mut digest)
        .map_err(|error| format!("cannot hash {subject} '{}': {error}", path.display()))?;
    Ok(format!("{:x}", digest.finalize()))
}

fn open_regular_at_most(path: &Path, subject: &str, max_bytes: u64) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|error| format!("cannot open {subject} '{}': {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("cannot inspect {subject} '{}': {error}", path.display()))?;
    if !metadata.is_file() || is_reparse(&metadata) || metadata.len() > max_bytes {
        return Err(format!(
            "{subject} must be a bounded regular file: {}",
            path.display()
        ));
    }
    Ok(file)
}

pub(super) fn collect_regular_files(
    root: &Path,
    subject: &str,
) -> Result<BTreeSet<String>, String> {
    regular_directory(root, subject)?;
    let mut pending = vec![root.to_path_buf()];
    let mut files = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|error| {
            format!(
                "cannot enumerate {subject} '{}': {error}",
                directory.display()
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("cannot enumerate {subject}: {error}"))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                format!("cannot inspect {subject} '{}': {error}", path.display())
            })?;
            if metadata.is_dir() {
                if is_reparse(&metadata) {
                    return Err(format!(
                        "{subject} directory cannot be a reparse point: {}",
                        path.display()
                    ));
                }
                pending.push(path);
            } else if metadata.is_file() || is_reparse(&metadata) {
                let relative = path.strip_prefix(root).map_err(|_| {
                    format!("{subject} escaped its controlled root: {}", path.display())
                })?;
                files.insert(relative.to_string_lossy().into_owned());
            } else {
                return Err(format!("unsupported {subject} entry: {}", path.display()));
            }
        }
    }
    Ok(files)
}

pub(super) fn sha256_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub(super) fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}
