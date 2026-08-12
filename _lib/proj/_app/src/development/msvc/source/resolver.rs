use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::atomic_file;
use crate::development::archive_tool::install::{remove_controlled_data, transfer_archive};
use crate::development::setup::storage::{
    ExclusiveFileLock, ensure_directory_chain, read_replaceable_bounded, regular_file_or_missing,
};

use super::{
    MAX_CHANNEL_BYTES, MAX_MANIFEST_BYTES, MsvcDefinition, MsvcError, MsvcErrorKind, MsvcRecipe,
    error, manifest_payload, resolve_recipe,
};

const LOCK_TIMEOUT: Duration = Duration::from_secs(60);

type DocumentTransfer<'a> =
    dyn FnMut(&OsStr, &Path, &mut dyn FnMut(u64, Option<u64>)) -> Result<u64, MsvcError> + 'a;

pub struct MsvcResolver<'a> {
    cache_data_root: &'a Path,
    definition: &'a MsvcDefinition,
}

impl<'a> MsvcResolver<'a> {
    pub fn new(cache_data_root: &'a Path, definition: &'a MsvcDefinition) -> Self {
        Self {
            cache_data_root,
            definition,
        }
    }

    pub fn resolve(
        &self,
        progress: &mut dyn FnMut(&str, u64, Option<u64>),
    ) -> Result<MsvcRecipe, MsvcError> {
        let mut transfer =
            |source: &OsStr, destination: &Path, report: &mut dyn FnMut(u64, Option<u64>)| {
                transfer_archive(source, destination, report).map_err(MsvcError::from)
            };
        self.resolve_with(&mut transfer, progress)
    }

    fn resolve_with(
        &self,
        transfer: &mut DocumentTransfer<'_>,
        progress: &mut dyn FnMut(&str, u64, Option<u64>),
    ) -> Result<MsvcRecipe, MsvcError> {
        let manifests = ensure_directory_chain(
            self.cache_data_root,
            &["downloads", "msvc", self.definition.channel(), "manifests"],
            "MSVC manifest cache",
        )?;
        let locks =
            ensure_directory_chain(self.cache_data_root, &["_locks"], "MSVC artifact locks")?;
        let channel_path = manifests.join("channel.json");
        let channel = self.refresh_channel(&channel_path, &locks, transfer, progress)?;
        let payload = manifest_payload(&channel)?;
        let manifest = self.product_manifest(&manifests, &locks, &payload, transfer, progress)?;
        resolve_recipe(self.definition, &channel, &manifest)
    }

    fn refresh_channel(
        &self,
        path: &Path,
        locks: &Path,
        transfer: &mut DocumentTransfer<'_>,
        progress: &mut dyn FnMut(&str, u64, Option<u64>),
    ) -> Result<Vec<u8>, MsvcError> {
        let _lock = ExclusiveFileLock::acquire(
            &locks.join(format!("msvc-channel-{}.lock", self.definition.channel())),
            LOCK_TIMEOUT,
        )?;
        let refreshed = transfer_bytes(
            transfer,
            OsStr::new(&self.definition.channel_url()),
            &path.with_extension(format!("{}.refresh", std::process::id())),
            "channel",
            MAX_CHANNEL_BYTES,
            progress,
        )
        .and_then(|content| {
            manifest_payload(&content)?;
            ensure_replaceable(path, "cached Visual Studio channel")?;
            atomic_file::publish(path, &content)
                .map_err(|cause| storage("publish the MSVC channel", path, cause))?;
            Ok(content)
        });
        match refreshed {
            Ok(content) => Ok(content),
            Err(refresh_error) => read_bounded(path, "cached Visual Studio channel", MAX_CHANNEL_BYTES)
                .and_then(|content| {
                    manifest_payload(&content)?;
                    Ok(content)
                })
                .map_err(|cache_error| {
                    error(
                        MsvcErrorKind::InvalidSource,
                        format!(
                            "cannot refresh the Visual Studio channel and no valid cache is available: {refresh_error}; cache: {cache_error}"
                        ),
                    )
                }),
        }
    }

    fn product_manifest(
        &self,
        manifests: &Path,
        locks: &Path,
        payload: &super::MsvcPayload,
        transfer: &mut DocumentTransfer<'_>,
        progress: &mut dyn FnMut(&str, u64, Option<u64>),
    ) -> Result<Vec<u8>, MsvcError> {
        let path = manifests.join(format!("VisualStudio.{}.vsman", &payload.sha256()[..16]));
        let digest_path = sibling_with_suffix(&path, ".actual.sha256")?;
        let _lock = ExclusiveFileLock::acquire(
            &locks.join(format!("msvc-manifest-{}.lock", payload.sha256())),
            LOCK_TIMEOUT,
        )?;
        if let Some(content) = cached_manifest(&path, &digest_path)? {
            return Ok(content);
        }
        let content = transfer_bytes(
            transfer,
            OsStr::new(payload.url()),
            &path.with_extension(format!("{}.download", std::process::id())),
            "manifest",
            MAX_MANIFEST_BYTES,
            progress,
        )?;
        if content.len() > MAX_MANIFEST_BYTES
            || serde_json::from_slice::<serde_json::Value>(&content).is_err()
        {
            return Err(error(
                MsvcErrorKind::InvalidSource,
                "downloaded Visual Studio manifest is invalid",
            ));
        }
        ensure_replaceable(&path, "cached Visual Studio manifest")?;
        ensure_replaceable(&digest_path, "cached Visual Studio manifest digest")?;
        atomic_file::publish(&path, &content)
            .map_err(|cause| storage("publish the Visual Studio manifest", &path, cause))?;
        let digest = format!("{:x}", Sha256::digest(&content));
        atomic_file::publish(&digest_path, format!("{digest}\r\n").as_bytes()).map_err(
            |cause| {
                storage(
                    "publish the Visual Studio manifest digest",
                    &digest_path,
                    cause,
                )
            },
        )?;
        Ok(content)
    }
}

fn transfer_bytes(
    transfer: &mut DocumentTransfer<'_>,
    source: &OsStr,
    temporary: &Path,
    id: &str,
    maximum: usize,
    progress: &mut dyn FnMut(&str, u64, Option<u64>),
) -> Result<Vec<u8>, MsvcError> {
    remove_controlled_data(temporary).map_err(MsvcError::from)?;
    let transferred = transfer(source, temporary, &mut |current, total| {
        progress(id, current, total)
    });
    let result =
        transferred.and_then(|_| read_bounded(temporary, "downloaded Microsoft document", maximum));
    let cleanup = remove_controlled_data(temporary).map_err(MsvcError::from);
    match (result, cleanup) {
        (Ok(content), Ok(())) => Ok(content),
        (Err(failure), Ok(())) => Err(failure),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(failure), Err(cleanup)) => Err(error(
            failure.kind,
            format!("{failure} Cleanup failed: {cleanup}"),
        )),
    }
}

fn cached_manifest(path: &Path, digest_path: &Path) -> Result<Option<Vec<u8>>, MsvcError> {
    if !regular_file_or_missing(path, "cached Visual Studio manifest")?
        || !regular_file_or_missing(digest_path, "cached Visual Studio manifest digest")?
    {
        return Ok(None);
    }
    let Some(content) = read_cache_candidate(
        path,
        "cached Visual Studio manifest",
        MAX_MANIFEST_BYTES as u64,
    )?
    else {
        return Ok(None);
    };
    let Some(recorded) =
        read_cache_candidate(digest_path, "cached Visual Studio manifest digest", 128)?
    else {
        return Ok(None);
    };
    let recorded = String::from_utf8(recorded)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase());
    let actual = format!("{:x}", Sha256::digest(&content));
    if recorded.as_deref() != Some(actual.as_str())
        || serde_json::from_slice::<serde_json::Value>(&content).is_err()
    {
        return Ok(None);
    }
    Ok(Some(content))
}

fn read_cache_candidate(
    path: &Path,
    subject: &str,
    maximum: u64,
) -> Result<Option<Vec<u8>>, MsvcError> {
    match read_replaceable_bounded(path, subject, maximum) {
        Ok(content) => Ok(Some(content)),
        Err(cause)
            if matches!(
                cause.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData
            ) =>
        {
            Ok(None)
        }
        Err(cause) => Err(storage("read the cached Microsoft document", path, cause)),
    }
}

fn ensure_replaceable(path: &Path, subject: &str) -> Result<(), MsvcError> {
    regular_file_or_missing(path, subject)
        .map(|_| ())
        .map_err(|cause| storage("validate the publication target", path, cause))
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> Result<PathBuf, MsvcError> {
    let name = path.file_name().ok_or_else(|| {
        error(
            MsvcErrorKind::Storage,
            format!("cache path has no file name: {}", path.display()),
        )
    })?;
    let mut suffixed = name.to_os_string();
    suffixed.push(suffix);
    Ok(path.with_file_name(suffixed))
}

fn read_bounded(path: &Path, subject: &str, maximum: usize) -> Result<Vec<u8>, MsvcError> {
    if !regular_file_or_missing(path, subject)? {
        return Err(error(
            MsvcErrorKind::MissingStorage,
            format!("{subject} is missing: {}", path.display()),
        ));
    }
    read_replaceable_bounded(path, subject, maximum as u64)
        .map_err(|cause| storage("read the Microsoft document", path, cause))
}

fn storage(action: &str, path: &Path, cause: std::io::Error) -> MsvcError {
    error(
        MsvcErrorKind::Storage,
        format!("cannot {action} '{}': {cause}", path.display()),
    )
}

#[cfg(test)]
mod tests;
