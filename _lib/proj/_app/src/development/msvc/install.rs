use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::development::archive_tool::install::{
    InstallationTransaction, cleanup_installation_data,
};
use crate::development::archive_tool::{ArchiveToolError, ArchiveToolErrorKind};
use crate::development::setup::storage::ensure_directory_chain;

use super::{
    MsvcDefinition, MsvcError, MsvcErrorKind, MsvcInstallation, MsvcPayloadCache, MsvcResolver,
    MsvcStager, MsvcStore, error,
};

mod assembly;
mod metadata;
mod msi;

pub struct MsvcInstallContext<'a> {
    data_root: &'a Path,
    cache_data_root: &'a Path,
}

impl<'a> MsvcInstallContext<'a> {
    pub fn new(data_root: &'a Path, cache_data_root: &'a Path) -> Result<Self, MsvcError> {
        if !data_root.is_absolute() || !cache_data_root.is_absolute() {
            return Err(error(
                MsvcErrorKind::InstallationFailed,
                "MSVC installation roots must be absolute",
            ));
        }
        Ok(Self {
            data_root,
            cache_data_root,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MsvcInstallOutcome {
    Ready,
    Recovered,
    Installed,
}

pub struct MsvcInstallResult {
    outcome: MsvcInstallOutcome,
    installation: MsvcInstallation,
    warnings: Vec<String>,
}

impl MsvcInstallResult {
    pub fn outcome(&self) -> MsvcInstallOutcome {
        self.outcome
    }

    pub fn installation(&self) -> &MsvcInstallation {
        &self.installation
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

pub fn ensure_installed(
    context: MsvcInstallContext<'_>,
    definition: &MsvcDefinition,
    progress: &mut dyn FnMut(&str, u64, Option<u64>),
) -> Result<MsvcInstallResult, MsvcError> {
    require_windows_x64()?;
    let installs = ensure_directory_chain(
        context.data_root,
        &[
            "modules", "kernel", ".dev", "setup", "export", "msvc", "installs",
        ],
        "MSVC installation parent",
    )?;
    let target = installs.join(definition.channel());
    let store = MsvcStore::new(context.data_root, definition);
    let mut transaction =
        InstallationTransaction::open(context.data_root, "msvc", target, |root| {
            candidate(&store, root)
        })?;
    let (existing, recovered, mut warnings) = transaction.recover()?;
    if let Some(installation) = existing {
        return Ok(MsvcInstallResult {
            outcome: if recovered {
                MsvcInstallOutcome::Recovered
            } else {
                MsvcInstallOutcome::Ready
            },
            installation,
            warnings,
        });
    }

    let recipe = MsvcResolver::new(context.cache_data_root, definition).resolve(progress)?;
    let work = transaction.work_path()?;
    let staged = transaction.staged_path()?;
    let installation = (|| {
        fresh_directory(&work, "MSVC installer source directory")?;
        fresh_directory(&staged, "staged MSVC installation")?;
        let cache = MsvcPayloadCache::new(context.cache_data_root, definition);

        for payload in recipe.tool_payloads() {
            let verified = cache.acquire(payload, &mut |current, total| {
                progress(payload.leaf_name(), current, total)
            })?;
            MsvcStager::expand_vsix(&verified, &staged)?;
        }
        let sources = msi::prepare_sources(&cache, &recipe, &work, progress)?;
        let logs = ensure_directory_chain(
            context.data_root,
            &[
                "modules", "kernel", ".dev", "setup", "export", "msvc", "_logs",
            ],
            "MSVC installation logs",
        )?;
        for source in sources {
            msi::install(&source, &staged, &logs)?;
        }
        let versions = assembly::complete(&staged)?;
        metadata::write(definition, &recipe, &staged, &versions)?;
        candidate(&store, &staged)
            .map_err(MsvcError::from)?
            .ok_or_else(|| {
                error(
                    MsvcErrorKind::InstallationFailed,
                    "staged MSVC installation failed validation",
                )
            })
    })();

    let work_cleanup = cleanup_installation_data(&[work]);
    warnings.extend(work_cleanup.iter().cloned());
    let installation = match installation {
        Ok(_) => match transaction.publish(&staged) {
            Ok((installation, publish_warnings)) => {
                warnings.extend(publish_warnings);
                installation
            }
            Err(failure) => {
                return Err(with_cleanup(MsvcError::from(failure), &[work_cleanup]));
            }
        },
        Err(failure) => {
            let cleanup = cleanup_installation_data(&[staged]);
            warnings.extend(cleanup.iter().cloned());
            return Err(with_cleanup(failure, &[work_cleanup, cleanup]));
        }
    };
    Ok(MsvcInstallResult {
        outcome: MsvcInstallOutcome::Installed,
        installation,
        warnings,
    })
}

fn fresh_directory(path: &Path, subject: &str) -> Result<(), MsvcError> {
    fs::create_dir(path).map_err(|cause| {
        error(
            MsvcErrorKind::InstallationFailed,
            format!("cannot create {subject} '{}': {cause}", path.display()),
        )
    })?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(error(
            MsvcErrorKind::UnsafeStorage,
            format!("{subject} must be a regular directory: {}", path.display()),
        ));
    }
    Ok(())
}

fn candidate(
    store: &MsvcStore<'_>,
    root: &Path,
) -> Result<Option<MsvcInstallation>, ArchiveToolError> {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(cause) => return Err(archive_error(MsvcError::from(cause))),
    };
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!(
                "MSVC installation candidate cannot be a reparse point: {}",
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
                "MSVC installation candidate is not a regular filesystem entry: {}",
                root.display()
            ),
        ));
    }
    match store.read_installation_at(root) {
        Ok(installation) => Ok(Some(installation)),
        Err(failure) if invalid_candidate(failure.kind) => Ok(None),
        Err(failure) => Err(archive_error(failure)),
    }
}

fn invalid_candidate(kind: MsvcErrorKind) -> bool {
    matches!(
        kind,
        MsvcErrorKind::MetadataUnreadable
            | MsvcErrorKind::MetadataStale
            | MsvcErrorKind::DuplicateFileRecords
            | MsvcErrorKind::MissingFileRecord
            | MsvcErrorKind::InvalidFileRecord
            | MsvcErrorKind::FileMismatch
            | MsvcErrorKind::MissingStorage
    )
}

fn archive_error(failure: MsvcError) -> ArchiveToolError {
    let kind = match failure.kind {
        MsvcErrorKind::UnsafeStorage => ArchiveToolErrorKind::UnsafeStorage,
        MsvcErrorKind::MissingStorage => ArchiveToolErrorKind::MissingStorage,
        MsvcErrorKind::LockUnavailable => ArchiveToolErrorKind::LockUnavailable,
        MsvcErrorKind::RecoveryFailed => ArchiveToolErrorKind::RecoveryFailed,
        MsvcErrorKind::FileMismatch => ArchiveToolErrorKind::FileMismatch,
        _ => ArchiveToolErrorKind::Storage,
    };
    ArchiveToolError::new(kind, failure.to_string())
}

fn with_cleanup(failure: MsvcError, groups: &[Vec<String>]) -> MsvcError {
    let warnings = groups.iter().flatten().cloned().collect::<Vec<_>>();
    if warnings.is_empty() {
        failure
    } else {
        error(
            failure.kind,
            format!("{failure} Cleanup warnings: {}.", warnings.join(" | ")),
        )
    }
}

fn require_windows_x64() -> Result<(), MsvcError> {
    if std::env::consts::OS == "windows" && std::env::consts::ARCH == "x86_64" {
        Ok(())
    } else {
        Err(error(
            MsvcErrorKind::InstallationFailed,
            "managed MSVC installation requires Windows x64",
        ))
    }
}

#[cfg(test)]
mod tests;
