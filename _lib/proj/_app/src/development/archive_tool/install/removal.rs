use std::fs;
use std::io;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::super::{ArchiveToolError, ArchiveToolErrorKind};

const PATH_ATTEMPTS: usize = 5;
const PATH_RETRY_DELAY: Duration = Duration::from_millis(150);

pub(super) fn move_path_with_retry(
    source: &Path,
    destination: &Path,
    activity: &str,
) -> Result<(), ArchiveToolError> {
    reject_reparse_or_missing(source, activity)?;
    if path_exists(destination)? {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::Storage,
            format!(
                "cannot {activity} because destination exists: {}",
                destination.display()
            ),
        ));
    }
    let mut last_error = None;
    for attempt in 1..=PATH_ATTEMPTS {
        match fs::rename(source, destination) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        if attempt < PATH_ATTEMPTS {
            thread::sleep(PATH_RETRY_DELAY * attempt as u32);
        }
    }
    Err(ArchiveToolError::new(
        ArchiveToolErrorKind::Storage,
        format!(
            "cannot finish {activity} after {PATH_ATTEMPTS} attempts. Release processes that lock \
             '{}', then retry. Last error: {}",
            source.display(),
            last_error.expect("a failed move has an error")
        ),
    ))
}

pub(in super::super) fn remove_residues(paths: &[PathBuf]) -> Vec<String> {
    remove_residues_with(paths, |_| Ok(()))
}

pub(in super::super) fn remove_residues_with(
    paths: &[PathBuf],
    prepare: fn(&Path) -> Result<(), ArchiveToolError>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for path in paths {
        match path_exists(path) {
            Ok(false) => {}
            Ok(true) => {
                if let Err(error) =
                    remove_path_with_retry_with(path, "clean installation recovery data", prepare)
                {
                    warnings.push(error.to_string());
                }
            }
            Err(error) => warnings.push(error.to_string()),
        }
    }
    warnings
}

pub(in super::super) fn with_cleanup_warnings(
    error: ArchiveToolError,
    warnings: &[String],
) -> ArchiveToolError {
    if warnings.is_empty() {
        return error;
    }
    ArchiveToolError::new(
        error.kind(),
        format!("{error} Cleanup warnings: {}.", warnings.join(" | ")),
    )
}

/// Removes one caller-validated controlled child without following reparse points.
/// Every descendant is inspected before it is visited or removed.
pub(in super::super) fn remove_controlled(path: &Path) -> Result<(), ArchiveToolError> {
    target_parent_and_leaf(path)?;
    remove_path_with_retry(path, "remove controlled installation data")
}

pub(super) fn remove_path_with_retry(path: &Path, activity: &str) -> Result<(), ArchiveToolError> {
    remove_path_with_retry_with(path, activity, |_| Ok(()))
}

pub(super) fn remove_path_with_retry_with(
    path: &Path,
    activity: &str,
    prepare: fn(&Path) -> Result<(), ArchiveToolError>,
) -> Result<(), ArchiveToolError> {
    prepare(path)?;
    let mut last_error = None;
    for attempt in 1..=PATH_ATTEMPTS {
        match remove_path_once(path) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        if attempt < PATH_ATTEMPTS {
            thread::sleep(PATH_RETRY_DELAY * attempt as u32);
        }
    }
    Err(ArchiveToolError::new(
        ArchiveToolErrorKind::Storage,
        format!(
            "cannot finish {activity} after {PATH_ATTEMPTS} attempts: {}. Release processes that \
             lock the path, then retry. Last error: {}",
            path.display(),
            last_error.expect("a failed removal has an error")
        ),
    ))
}

fn remove_path_once(path: &Path) -> io::Result<()> {
    validate_removal_tree(path)?;
    remove_validated_tree(path)
}

fn validate_removal_tree(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if is_reparse(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing to remove a reparse point: {}", path.display()),
        ));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            validate_removal_tree(&entry?.path())?;
        }
        Ok(())
    } else if metadata.is_file() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "path is neither a regular directory nor file: {}",
                path.display()
            ),
        ))
    }
}

fn remove_validated_tree(path: &Path) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if is_reparse(&metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing to remove a reparse point: {}", path.display()),
        ));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            remove_validated_tree(&entry?.path())?;
        }
        fs::remove_dir(path)
    } else if metadata.is_file() {
        fs::remove_file(path)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "path is neither a regular directory nor file: {}",
                path.display()
            ),
        ))
    }
}

pub(super) fn path_exists(path: &Path) -> Result<bool, ArchiveToolError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_reparse(&metadata) => {
            Err(unsafe_path("installation transaction path", path))
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(storage(
            "inspect installation transaction path",
            path,
            error,
        )),
    }
}

pub(super) fn reject_reparse_or_missing(
    path: &Path,
    subject: &str,
) -> Result<(), ArchiveToolError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_reparse(&metadata) => Err(unsafe_path(subject, path)),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage(&format!("inspect {subject}"), path, error)),
    }
}

pub(super) fn require_regular_directory(
    path: &Path,
    subject: &str,
) -> Result<(), ArchiveToolError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| storage(&format!("inspect {subject}"), path, error))?;
    if !metadata.is_dir() || is_reparse(&metadata) {
        return Err(unsafe_path(subject, path));
    }
    Ok(())
}

pub(super) fn target_parent_and_leaf(
    target: &Path,
) -> Result<(&Path, &std::ffi::OsStr), ArchiveToolError> {
    let parent = target.parent().ok_or_else(|| {
        ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!("installation target has no parent: {}", target.display()),
        )
    })?;
    let leaf = target.file_name().ok_or_else(|| {
        ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!("installation target has no file name: {}", target.display()),
        )
    })?;
    Ok((parent, leaf))
}

pub(super) fn unsafe_path(subject: &str, path: &Path) -> ArchiveToolError {
    ArchiveToolError::new(
        ArchiveToolErrorKind::UnsafeStorage,
        format!("{subject} must not be a reparse point: {}", path.display()),
    )
}

pub(super) fn storage(activity: &str, path: &Path, error: io::Error) -> ArchiveToolError {
    ArchiveToolError::new(
        ArchiveToolErrorKind::Storage,
        format!("cannot {activity} '{}': {error}", path.display()),
    )
}

pub(super) fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn controlled_removal_preflights_descendant_reparse_points() {
        let root = std::env::temp_dir().join(format!(
            "swawkit-archive-removal-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let controlled = root.join("cache");
        let external = root.join("external");
        fs::create_dir_all(&controlled).expect("create controlled directory");
        fs::create_dir(&external).expect("create external directory");
        let ordinary = controlled.join("ordinary.txt");
        let link = controlled.join("linked");
        fs::write(&ordinary, b"keep until preflight succeeds").expect("write ordinary child");
        if let Err(error) = std::os::windows::fs::symlink_dir(&external, &link) {
            if error.kind() == io::ErrorKind::PermissionDenied {
                let _ = fs::remove_dir_all(root);
                return;
            }
            panic!("create descendant reparse point: {error}");
        }

        let error = remove_controlled(&controlled).unwrap_err();

        assert!(error.to_string().contains("reparse point"));
        assert!(ordinary.is_file());
        assert!(external.is_dir());
        let _ = fs::remove_dir_all(root);
    }
}
