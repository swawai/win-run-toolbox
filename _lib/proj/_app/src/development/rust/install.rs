use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::{
    RustDefinition, RustError, RustErrorKind, RustInstallation, RustStore, RustupCache, error,
};
use crate::development::archive_tool::install::{
    InstallationTransaction, cleanup_installation_data_with,
};
use crate::development::archive_tool::{ArchiveToolError, ArchiveToolErrorKind};
use crate::development::setup::storage::ensure_directory_chain;

pub struct RustInstallContext<'a> {
    data_root: &'a Path,
    cache_data_root: &'a Path,
}

impl<'a> RustInstallContext<'a> {
    pub fn new(data_root: &'a Path, cache_data_root: &'a Path) -> Result<Self, RustError> {
        if !data_root.is_absolute() || !cache_data_root.is_absolute() {
            return Err(error(
                RustErrorKind::InstallationFailed,
                "Rust installation roots must be absolute",
            ));
        }
        Ok(Self {
            data_root,
            cache_data_root,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RustInstallOutcome {
    Ready,
    Recovered,
    Installed,
}

pub struct RustInstallResult {
    outcome: RustInstallOutcome,
    installation: RustInstallation,
    warnings: Vec<String>,
}

impl RustInstallResult {
    pub fn outcome(&self) -> RustInstallOutcome {
        self.outcome
    }

    pub fn installation(&self) -> &RustInstallation {
        &self.installation
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

pub fn ensure_installed(
    context: RustInstallContext<'_>,
    definition: &RustDefinition,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<RustInstallResult, RustError> {
    let installs = ensure_directory_chain(
        context.data_root,
        &[
            "modules", "kernel", ".dev", "setup", "export", "rust", "installs",
        ],
        "Rust installation parent",
    )?;
    let target = installs.join(definition.toolchain());
    let store = RustStore::new(context.data_root, definition);
    let mut transaction = InstallationTransaction::open_with_removal(
        context.data_root,
        "rust",
        target,
        |root| candidate(&store, root),
        prepare_rust_removal,
    )?;
    let (existing, recovered, mut warnings) = transaction.recover()?;
    if let Some(installation) = existing {
        return Ok(RustInstallResult {
            outcome: if recovered {
                RustInstallOutcome::Recovered
            } else {
                RustInstallOutcome::Ready
            },
            installation,
            warnings,
        });
    }
    let installer = RustupCache::new(context.cache_data_root, definition).acquire(progress)?;
    let staged = transaction.staged_path()?;
    let installation = (|| {
        fresh_directory(&staged, "staged Rust installation")?;
        fs::create_dir(staged.join("cargo"))?;
        fs::create_dir(staged.join("rustup"))?;
        super::process::run_installer(definition, &installer, &staged)?;
        let probe = super::probe::inspect(definition, &staged)?;
        super::metadata::write(definition, &probe, &staged, installer.sha256())?;
        super::store::read_installation_at(definition, &staged)
    })();
    match installation {
        Ok(_) => match transaction.publish(&staged) {
            Ok((installation, publish_warnings)) => {
                warnings.extend(publish_warnings);
                Ok(RustInstallResult {
                    outcome: RustInstallOutcome::Installed,
                    installation,
                    warnings,
                })
            }
            Err(failure) => Err(with_cleanup(
                RustError::from(failure),
                cleanup_installation_data_with(&[staged], prepare_rust_removal),
            )),
        },
        Err(failure) => Err(with_cleanup(
            failure,
            cleanup_installation_data_with(&[staged], prepare_rust_removal),
        )),
    }
}

fn prepare_rust_removal(root: &Path) -> Result<(), ArchiveToolError> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(cause) => return Err(archive_error(RustError::from(cause))),
    };
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!(
                "Rust installation root cannot be a reparse point: {}",
                root.display()
            ),
        ));
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    let mut pending = vec![root.to_path_buf()];
    let mut proxies = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|cause| {
            ArchiveToolError::new(
                ArchiveToolErrorKind::Storage,
                format!(
                    "cannot inspect Rust installation data '{}': {cause}",
                    directory.display()
                ),
            )
        })? {
            let path = entry
                .map_err(|cause| {
                    ArchiveToolError::new(ArchiveToolErrorKind::Storage, cause.to_string())
                })?
                .path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|cause| archive_error(RustError::from(cause)))?;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                if !owned_rustup_proxy(root, &path)? {
                    return Err(ArchiveToolError::new(
                        ArchiveToolErrorKind::UnsafeStorage,
                        format!(
                            "Rust installation contains an unowned reparse point: {}",
                            path.display()
                        ),
                    ));
                }
                proxies.push(path);
            } else if metadata.is_dir() {
                pending.push(path);
            }
        }
    }
    for proxy in proxies {
        fs::remove_file(&proxy).map_err(|cause| {
            ArchiveToolError::new(
                ArchiveToolErrorKind::Storage,
                format!(
                    "cannot remove owned rustup proxy '{}': {cause}",
                    proxy.display()
                ),
            )
        })?;
    }
    Ok(())
}

fn owned_rustup_proxy(root: &Path, path: &Path) -> Result<bool, ArchiveToolError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!("Rust removal path escaped its root: {}", path.display()),
        )
    })?;
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if components.len() != 3
        || !components[0].eq_ignore_ascii_case("cargo")
        || !components[1].eq_ignore_ascii_case("bin")
        || !Path::new(&components[2])
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Ok(false);
    }
    fs::read_link(path)
        .map(|target| target == Path::new("rustup.exe"))
        .map_err(|cause| {
            ArchiveToolError::new(
                ArchiveToolErrorKind::Storage,
                format!("cannot inspect rustup proxy '{}': {cause}", path.display()),
            )
        })
}

fn fresh_directory(path: &Path, subject: &str) -> Result<(), RustError> {
    fs::create_dir(path).map_err(RustError::from)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(error(
            RustErrorKind::UnsafeStorage,
            format!("{subject} must be a regular directory: {}", path.display()),
        ));
    }
    Ok(())
}

fn candidate(
    store: &RustStore<'_>,
    root: &Path,
) -> Result<Option<RustInstallation>, ArchiveToolError> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(cause) => return Err(archive_error(RustError::from(cause))),
    };
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!(
                "Rust installation candidate cannot be a reparse point: {}",
                root.display()
            ),
        ));
    }
    if metadata.is_file() {
        return Ok(None);
    }
    if !metadata.is_dir() {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!(
                "Rust installation candidate is not a regular filesystem entry: {}",
                root.display()
            ),
        ));
    }
    match super::store::read_installation_at(store.definition, root) {
        Ok(installation) => Ok(Some(installation)),
        Err(failure) if invalid_candidate(failure.kind()) => Ok(None),
        Err(failure) => Err(archive_error(failure)),
    }
}

fn invalid_candidate(kind: RustErrorKind) -> bool {
    matches!(
        kind,
        RustErrorKind::MetadataUnreadable
            | RustErrorKind::MetadataStale
            | RustErrorKind::InvalidInventory
            | RustErrorKind::FileMismatch
            | RustErrorKind::MissingStorage
    )
}

fn archive_error(failure: RustError) -> ArchiveToolError {
    let kind = match failure.kind() {
        RustErrorKind::UnsafeStorage => ArchiveToolErrorKind::UnsafeStorage,
        RustErrorKind::MissingStorage => ArchiveToolErrorKind::MissingStorage,
        RustErrorKind::FileMismatch => ArchiveToolErrorKind::FileMismatch,
        _ => ArchiveToolErrorKind::Storage,
    };
    ArchiveToolError::new(kind, failure.to_string())
}

fn with_cleanup(failure: RustError, warnings: Vec<String>) -> RustError {
    if warnings.is_empty() {
        failure
    } else {
        error(
            failure.kind(),
            format!("{failure} Cleanup warnings: {}.", warnings.join(" | ")),
        )
    }
}

#[cfg(test)]
#[path = "install/tests.rs"]
mod tests;
