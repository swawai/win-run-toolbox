use std::collections::BTreeSet;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::{ContextRecord, ContextResult, ContextStoreError, MAX_CONTEXT_BYTES, not_found};
use crate::atomic_file;

const RESOURCE_DOCUMENT: &str = "_resource.json";
static NEXT_STAGING_DIRECTORY: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
pub(super) fn context_directory(module_data_root: &Path) -> PathBuf {
    module_data_root.to_path_buf()
}

pub(super) fn ensure_context_directory(
    data_root: &Path,
    module_data_root: &Path,
) -> ContextResult<PathBuf> {
    validate_directory(data_root, "Context DataRoot")?;
    let relative = safe_module_relative(data_root, module_data_root)?;
    let mut current = data_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(ContextStoreError::new(format!(
                "Context module DataRoot is not a safe child of '{}': {}",
                data_root.display(),
                module_data_root.display()
            )));
        };
        current.push(segment);
        match fs::create_dir(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(ContextStoreError::new(format!(
                    "cannot create Context directory '{}': {error}",
                    current.display()
                )));
            }
        }
        validate_directory(&current, "Context directory")?;
    }
    Ok(current)
}

pub(super) fn existing_context_directory(
    data_root: &Path,
    module_data_root: &Path,
) -> ContextResult<Option<PathBuf>> {
    validate_directory(data_root, "Context DataRoot")?;
    let relative = safe_module_relative(data_root, module_data_root)?;
    let mut current = data_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(segment) = component else {
            return Err(ContextStoreError::new(format!(
                "Context module DataRoot is not a safe child of '{}': {}",
                data_root.display(),
                module_data_root.display()
            )));
        };
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(_) => validate_directory(&current, "Context directory")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(ContextStoreError::new(format!(
                    "cannot inspect Context directory '{}': {error}",
                    current.display()
                )));
            }
        }
    }
    Ok(Some(current))
}

pub(super) fn resource_directories(
    directory: &Path,
    reserved_ids: &BTreeSet<String>,
) -> ContextResult<Vec<(String, PathBuf)>> {
    let mut resources = Vec::new();
    let entries = fs::read_dir(directory).map_err(|error| {
        ContextStoreError::new(format!(
            "cannot list Context directory '{}': {error}",
            directory.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            ContextStoreError::new(format!(
                "cannot read Context directory '{}': {error}",
                directory.display()
            ))
        })?;
        let name = entry.file_name().into_string().map_err(|_| {
            ContextStoreError::new(format!(
                "Context resource directory name is not valid Unicode: {}",
                entry.path().display()
            ))
        })?;
        if name.starts_with('_') {
            continue;
        }
        if reserved_ids.contains(&name) {
            validate_directory(&entry.path(), "Context static command data")?;
            continue;
        }
        let path = entry.path();
        validate_directory(&path, "Context resource")?;
        super::validate_id(&name)?;
        resources.push((name, path));
    }
    resources.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(resources)
}

pub(super) fn context_path(directory: &Path, id: &str) -> PathBuf {
    directory.join(id).join(RESOURCE_DOCUMENT)
}

pub(super) fn read_optional_record(
    directory: &Path,
    id: &str,
) -> ContextResult<Option<ContextRecord>> {
    let resource_directory = directory.join(id);
    match fs::symlink_metadata(&resource_directory) {
        Ok(_) => validate_directory(&resource_directory, "Context resource")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ContextStoreError::new(format!(
                "cannot inspect Context resource '{}': {error}",
                resource_directory.display()
            )));
        }
    }
    read_record(&resource_directory.join(RESOURCE_DOCUMENT), id).map(Some)
}

pub(super) fn read_record(path: &Path, expected_id: &str) -> ContextResult<ContextRecord> {
    let metadata = publication_metadata(path)?.ok_or_else(|| not_found(expected_id))?;
    if metadata.len() > MAX_CONTEXT_BYTES as u64 {
        return Err(ContextStoreError::new(format!(
            "Context file exceeds {MAX_CONTEXT_BYTES} bytes: {}",
            path.display()
        )));
    }
    let content = fs::read(path).map_err(|error| {
        ContextStoreError::new(format!("cannot read Context '{}': {error}", path.display()))
    })?;
    let record: ContextRecord = serde_json::from_slice(&content).map_err(|error| {
        ContextStoreError::new(format!(
            "invalid Context JSON '{}': {error}",
            path.display()
        ))
    })?;
    record.validate()?;
    if record.id != expected_id {
        return Err(ContextStoreError::new(format!(
            "Context resource '{}' declares mismatched ID '{}'",
            path.display(),
            record.id
        )));
    }
    Ok(record)
}

pub(super) fn publish_new_record(directory: &Path, record: &ContextRecord) -> ContextResult<()> {
    let target = directory.join(&record.id);
    if fs::symlink_metadata(&target).is_ok() {
        return Err(ContextStoreError::new(format!(
            "Context already exists: {}",
            record.id
        )));
    }
    let sequence = NEXT_STAGING_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let staging = directory.join(format!("_partial-{}-{sequence}", std::process::id()));
    fs::create_dir(&staging).map_err(|error| {
        ContextStoreError::new(format!(
            "cannot create staged Context resource '{}': {error}",
            staging.display()
        ))
    })?;
    let result = publish_record(&staging.join(RESOURCE_DOCUMENT), record).and_then(|_| {
        fs::rename(&staging, &target).map_err(|error| {
            ContextStoreError::new(format!(
                "cannot publish Context resource '{}': {error}",
                target.display()
            ))
        })
    });
    if result.is_err() {
        let _ = fs::remove_file(staging.join(RESOURCE_DOCUMENT));
        let _ = fs::remove_dir(&staging);
    }
    result
}

pub(super) fn publish_record(path: &Path, record: &ContextRecord) -> ContextResult<()> {
    record.validate()?;
    let mut content = serde_json::to_vec_pretty(record).map_err(|error| {
        ContextStoreError::new(format!("cannot serialize Context '{}': {error}", record.id))
    })?;
    content.push(b'\n');
    if content.len() > MAX_CONTEXT_BYTES {
        return Err(ContextStoreError::new(format!(
            "serialized Context accepts at most {MAX_CONTEXT_BYTES} bytes"
        )));
    }
    publication_metadata(path)?;
    atomic_file::publish(path, &content).map_err(|error| {
        ContextStoreError::new(format!(
            "cannot publish Context '{}': {error}",
            path.display()
        ))
    })
}

pub(super) fn delete_record(directory: &Path, id: &str) -> ContextResult<()> {
    let resource_directory = directory.join(id);
    validate_directory(&resource_directory, "Context resource")?;
    let entries = fs::read_dir(&resource_directory)
        .map_err(|error| ContextStoreError::new(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| ContextStoreError::new(error.to_string()))?;
    if entries.len() != 1 || entries[0].file_name() != RESOURCE_DOCUMENT {
        return Err(ContextStoreError::new(format!(
            "Context resource contains unexpected files and cannot be deleted safely: {}",
            resource_directory.display()
        )));
    }
    let document = resource_directory.join(RESOURCE_DOCUMENT);
    publication_metadata(&document)?.ok_or_else(|| not_found(id))?;
    fs::remove_file(&document).map_err(|error| {
        ContextStoreError::new(format!(
            "cannot delete Context document '{}': {error}",
            document.display()
        ))
    })?;
    fs::remove_dir(&resource_directory).map_err(|error| {
        ContextStoreError::new(format!(
            "cannot delete Context resource '{}': {error}",
            resource_directory.display()
        ))
    })
}

fn publication_metadata(path: &Path) -> ContextResult<Option<fs::Metadata>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ContextStoreError::new(format!(
                "cannot inspect Context file '{}': {error}",
                path.display()
            )));
        }
    };
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ContextStoreError::new(format!(
            "Context publication must be a regular file: {}",
            path.display()
        )));
    }
    Ok(Some(metadata))
}

fn safe_module_relative<'a>(
    data_root: &'a Path,
    module_data_root: &'a Path,
) -> ContextResult<&'a Path> {
    let relative = module_data_root.strip_prefix(data_root).map_err(|_| {
        ContextStoreError::new(format!(
            "Context module DataRoot is outside '{}': {}",
            data_root.display(),
            module_data_root.display()
        ))
    })?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ContextStoreError::new(format!(
            "Context module DataRoot is not a safe child of '{}': {}",
            data_root.display(),
            module_data_root.display()
        )));
    }
    Ok(relative)
}

fn validate_directory(path: &Path, label: &str) -> ContextResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        ContextStoreError::new(format!(
            "cannot inspect {label} '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ContextStoreError::new(format!(
            "{label} must be a regular directory: {}",
            path.display()
        )));
    }
    Ok(())
}
