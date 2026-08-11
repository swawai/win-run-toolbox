use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

pub(crate) fn regular_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    let path = absolute(path, label)?;
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("cannot inspect {label} '{}': {error}", path.display()))?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!("{label} is not a regular file: {}", path.display()));
    }
    Ok(path)
}

pub(crate) fn controlled_destination(
    root: &Path,
    destination: &Path,
    label: &str,
) -> Result<PathBuf, String> {
    let root = absolute(root, "controlled root")?;
    let destination = absolute(destination, label)?;
    reject_parent_components(&root, "controlled root")?;
    reject_parent_components(&destination, label)?;
    if !starts_with_windows(&destination, &root) || destination == root {
        return Err(format!(
            "{label} escapes the controlled root: {}",
            destination.display()
        ));
    }
    validate_directory(&root, "controlled root")?;
    let parent = destination
        .parent()
        .ok_or_else(|| format!("{label} has no parent directory"))?;
    validate_existing_chain(&root, parent, label)?;
    let canonical_root = fs::canonicalize(&root).map_err(|error| {
        format!(
            "cannot resolve controlled root '{}': {error}",
            root.display()
        )
    })?;
    let canonical_parent = fs::canonicalize(parent).map_err(|error| {
        format!(
            "cannot resolve {label} parent '{}': {error}",
            parent.display()
        )
    })?;
    if !starts_with_windows(&canonical_parent, &canonical_root) {
        return Err(format!("{label} parent escapes through a reparse point"));
    }
    if let Ok(metadata) = fs::symlink_metadata(&destination)
        && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(format!(
            "{label} cannot be a reparse point: {}",
            destination.display()
        ));
    }
    Ok(destination)
}

pub(crate) fn validate_directory(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {label} '{}': {error}", path.display()))?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(format!(
            "{label} is not a regular directory: {}",
            path.display()
        ));
    }
    Ok(())
}

fn validate_existing_chain(root: &Path, target: &Path, label: &str) -> Result<(), String> {
    if !starts_with_windows(target, root) {
        return Err(format!("{label} escapes the controlled root"));
    }
    let mut current = root.to_path_buf();
    for component in target.components().skip(root.components().count()) {
        current.push(component.as_os_str());
        validate_directory(&current, label)?;
    }
    Ok(())
}

fn starts_with_windows(path: &Path, root: &Path) -> bool {
    let mut path_components = path.components();
    for root_component in root.components() {
        let Some(path_component) = path_components.next() else {
            return false;
        };
        if !component_equal(path_component, root_component) {
            return false;
        }
    }
    true
}

fn component_equal(left: Component<'_>, right: Component<'_>) -> bool {
    left.as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

fn reject_parent_components(path: &Path, label: &str) -> Result<(), String> {
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(format!("{label} contains an unresolved parent component"));
    }
    Ok(())
}

fn absolute(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!("{label} must be absolute: {}", path.display()));
    }
    std::path::absolute(path)
        .map_err(|error| format!("cannot resolve {label} '{}': {error}", path.display()))
}
