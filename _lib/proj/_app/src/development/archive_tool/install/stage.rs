use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::development::ArchiveToolContract;

use super::super::filesystem::{is_reparse, regular_directory, regular_file_digest};
use super::super::{
    ArchiveToolError, ArchiveToolErrorKind, InstallMetadata, InstalledFile, ResolvedDefinition,
};
use super::ArchiveSource;
use super::archive::extract_file as extract_archive_file;
use super::recipe::Recipe;

pub(super) fn payload(
    tool: &ArchiveToolContract,
    resolved: &ResolvedDefinition,
    source: &ArchiveSource,
    archive: &File,
    source_sha256: &str,
    work: &Path,
    staged: &Path,
    recipe: &dyn Recipe,
) -> Result<(), ArchiveToolError> {
    let extract = work.join("extract");
    create_fresh_directory(&extract, "archive extraction directory")?;
    extract_archive_file(archive, &extract)?;
    let source_root = archive_source_root(&extract, tool.archive_subdir)?;
    copy_tree(&source_root, staged)?;
    recipe.prepare(tool, staged)?;
    recipe.validate(tool, resolved, staged)?;
    let metadata = install_metadata(tool, resolved, source, source_sha256, staged)?;
    write_metadata(staged, &metadata)
}

pub(super) fn create_fresh_directory(path: &Path, subject: &str) -> Result<(), ArchiveToolError> {
    fs::create_dir(path).map_err(|error| {
        ArchiveToolError::new(
            ArchiveToolErrorKind::InstallationFailed,
            format!("cannot create {subject} '{}': {error}", path.display()),
        )
    })?;
    regular_directory(path, subject)
}

fn install_metadata(
    tool: &ArchiveToolContract,
    resolved: &ResolvedDefinition,
    source: &ArchiveSource,
    source_sha256: &str,
    staged: &Path,
) -> Result<InstallMetadata, ArchiveToolError> {
    let mut files = Vec::with_capacity(tool.required_paths.len());
    for relative in tool.required_paths {
        let path = safe_relative(staged, relative, "required installed file")?;
        let (length, sha256) =
            regular_file_digest(&path, "required installed file", 4 * 1024 * 1024 * 1024)?;
        files.push(InstalledFile::new((*relative).to_owned(), length, sha256));
    }
    Ok(InstallMetadata::new(
        tool.name.to_owned(),
        resolved.version().to_owned(),
        source.url().to_owned(),
        source_sha256.to_owned(),
        source.verification(),
        tool.recipe_version.to_owned(),
        tool.definition_signature(resolved.version(), resolved.project_sha256()),
        files,
    ))
}

fn write_metadata(root: &Path, metadata: &InstallMetadata) -> Result<(), ArchiveToolError> {
    let path = root.join(".swawkit-dev-install.json");
    let content = serde_json::to_string_pretty(metadata)
        .map_err(|error| {
            ArchiveToolError::new(
                ArchiveToolErrorKind::InstallationFailed,
                format!("cannot serialize archive tool install metadata: {error}"),
            )
        })?
        .replace('\n', "\r\n")
        + "\r\n";
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| install_io("create install metadata", &path, error))?;
    file.write_all(content.as_bytes())
        .map_err(|error| install_io("write install metadata", &path, error))?;
    file.sync_all()
        .map_err(|error| install_io("flush install metadata", &path, error))
}

fn archive_source_root(extract: &Path, relative: &str) -> Result<PathBuf, ArchiveToolError> {
    if relative.is_empty() {
        regular_directory(extract, "archive extraction root")?;
        return Ok(extract.to_path_buf());
    }
    let path = safe_relative(extract, relative, "archive subdirectory")?;
    regular_directory(&path, "archive subdirectory")?;
    Ok(path)
}

fn safe_relative(root: &Path, relative: &str, subject: &str) -> Result<PathBuf, ArchiveToolError> {
    let components: Vec<_> = Path::new(relative).components().collect();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::InvalidInstallRequest,
            format!("unsafe {subject} path '{relative}'"),
        ));
    }
    let mut path = root.to_path_buf();
    for component in components {
        let Component::Normal(name) = component else {
            unreachable!()
        };
        path.push(name);
    }
    Ok(path)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), ArchiveToolError> {
    regular_directory(source, "archive payload")?;
    regular_directory(destination, "staged installation")?;
    for entry in
        fs::read_dir(source).map_err(|error| install_io("inspect payload", source, error))?
    {
        let entry = entry.map_err(|error| install_io("inspect payload", source, error))?;
        let source_path = entry.path();
        let target_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| install_io("inspect payload", &source_path, error))?;
        if is_reparse(&metadata) {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::UnsafeStorage,
                format!(
                    "archive payload cannot contain a reparse point: {}",
                    source_path.display()
                ),
            ));
        }
        if metadata.is_dir() {
            fs::create_dir(&target_path)
                .map_err(|error| install_io("create staged directory", &target_path, error))?;
            copy_tree(&source_path, &target_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &target_path)
                .map_err(|error| install_io("copy staged file", &target_path, error))?;
        } else {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::ArchiveInvalid,
                format!(
                    "archive payload contains an unsupported entry: {}",
                    source_path.display()
                ),
            ));
        }
    }
    Ok(())
}

fn install_io(action: &str, path: &Path, error: std::io::Error) -> ArchiveToolError {
    ArchiveToolError::new(
        ArchiveToolErrorKind::InstallationFailed,
        format!("cannot {action} '{}': {error}", path.display()),
    )
}
