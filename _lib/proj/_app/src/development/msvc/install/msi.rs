use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::development::archive_tool::install::remove_controlled_data;
use crate::development::msvc::{
    MsvcError, MsvcErrorKind, MsvcPayload, MsvcPayloadCache, MsvcRecipe, VerifiedMsvcPayload, error,
};

const MAX_MSI_BYTES: u64 = 512 * 1024 * 1024;
const MSI_TIMEOUT: Duration = Duration::from_secs(10 * 60);

pub(super) fn prepare_sources(
    cache: &MsvcPayloadCache<'_>,
    recipe: &MsvcRecipe,
    root: &Path,
    progress: &mut dyn FnMut(&str, u64, Option<u64>),
) -> Result<Vec<PathBuf>, MsvcError> {
    require_directory(root, "MSVC installer source directory")?;
    let cab_candidates = unique_cab_candidates(recipe.sdk_payloads())?;
    let mut referenced = BTreeMap::<String, String>::new();
    let mut msi_paths = Vec::new();
    for payload in recipe.msi_payloads() {
        let verified = cache.acquire(payload, &mut |current, total| {
            progress(payload.leaf_name(), current, total)
        })?;
        let path = copy_verified(&verified, payload, root)?;
        for name in referenced_cabs(&path, &cab_candidates)? {
            referenced.insert(name.to_ascii_lowercase(), name);
        }
        msi_paths.push(path);
    }
    for name in referenced.into_values() {
        let payload = recipe
            .sdk_payloads()
            .iter()
            .find(|payload| payload.leaf_name().eq_ignore_ascii_case(&name))
            .expect("unique CAB candidates come from the recipe");
        let verified = cache.acquire(payload, &mut |current, total| {
            progress(payload.leaf_name(), current, total)
        })?;
        copy_verified(&verified, payload, root)?;
    }
    Ok(msi_paths)
}

fn unique_cab_candidates(payloads: &[MsvcPayload]) -> Result<Vec<String>, MsvcError> {
    let candidates = payloads
        .iter()
        .filter(|payload| {
            Path::new(payload.leaf_name())
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("cab"))
        })
        .map(|payload| payload.leaf_name().to_owned())
        .collect::<Vec<_>>();
    let mut unique = BTreeMap::<String, String>::new();
    for candidate in candidates {
        if unique
            .insert(candidate.to_ascii_lowercase(), candidate.clone())
            .is_some()
        {
            return Err(error(
                MsvcErrorKind::InvalidSource,
                format!("Windows SDK manifest contains a duplicate CAB payload leaf: {candidate}"),
            ));
        }
    }
    Ok(unique.into_values().collect())
}

pub(super) fn install(msi: &Path, destination: &Path, logs: &Path) -> Result<(), MsvcError> {
    require_file(msi, "MSVC MSI source")?;
    require_directory(destination, "staged MSVC installation")?;
    require_directory(logs, "MSVC installation log directory")?;
    let system_root = std::env::var_os("SystemRoot").ok_or_else(|| {
        error(
            MsvcErrorKind::InstallationFailed,
            "SystemRoot is unavailable; Windows Installer cannot be located",
        )
    })?;
    let executable = PathBuf::from(system_root).join(r"System32\msiexec.exe");
    require_file(&executable, "Windows Installer")?;
    let leaf = msi.file_name().ok_or_else(|| {
        error(
            MsvcErrorKind::InstallationFailed,
            format!("MSVC MSI source has no file name: {}", msi.display()),
        )
    })?;
    let log = logs.join(format!("{}.install.log", leaf.to_string_lossy()));
    remove_controlled_data(&log)?;
    let mut child = Command::new(&executable)
        .arg("/a")
        .arg(msi)
        .arg("/quiet")
        .arg("/qn")
        .arg(format!("TARGETDIR={}", destination.display()))
        .arg("/l*v")
        .arg(&log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|cause| {
            error(
                MsvcErrorKind::InstallationFailed,
                format!(
                    "cannot start Windows Installer for '{}': {cause}",
                    msi.display()
                ),
            )
        })?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(status)) => {
                return Err(error(
                    MsvcErrorKind::InstallationFailed,
                    format!(
                        "Windows Installer exited with code {} for '{}'. Diagnostic log: {}",
                        status.code().unwrap_or(-1),
                        msi.display(),
                        log.display()
                    ),
                ));
            }
            Ok(None) if started.elapsed() < MSI_TIMEOUT => {
                thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error(
                    MsvcErrorKind::InstallationFailed,
                    format!(
                        "Windows Installer timed out for '{}': {}",
                        msi.display(),
                        log.display()
                    ),
                ));
            }
            Err(cause) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error(
                    MsvcErrorKind::InstallationFailed,
                    format!(
                        "cannot wait for Windows Installer '{}': {cause}",
                        msi.display()
                    ),
                ));
            }
        }
    }
    remove_controlled_data(&log).map_err(MsvcError::from)
}

fn copy_verified(
    verified: &VerifiedMsvcPayload,
    payload: &MsvcPayload,
    root: &Path,
) -> Result<PathBuf, MsvcError> {
    let destination = root.join(payload.leaf_name());
    let mut input = verified.try_clone()?;
    input
        .seek(SeekFrom::Start(0))
        .map_err(|cause| storage("seek", verified.path(), cause))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|cause| storage("create", &destination, cause))?;
    let copied = std::io::copy(&mut BufReader::new(input), &mut output)
        .map_err(|cause| storage("copy", &destination, cause))?;
    output
        .sync_all()
        .map_err(|cause| storage("flush", &destination, cause))?;
    if copied != verified.length() {
        return Err(error(
            MsvcErrorKind::FileMismatch,
            format!(
                "MSVC installer source copy length mismatch: {}",
                destination.display()
            ),
        ));
    }
    let mut digest = Sha256::new();
    let mut file =
        fs::File::open(&destination).map_err(|cause| storage("open", &destination, cause))?;
    std::io::copy(&mut file, &mut digest).map_err(|cause| storage("hash", &destination, cause))?;
    if format!("{:x}", digest.finalize()) != payload.sha256() {
        return Err(error(
            MsvcErrorKind::FileMismatch,
            format!(
                "MSVC installer source copy SHA-256 mismatch: {}",
                destination.display()
            ),
        ));
    }
    Ok(destination)
}

fn referenced_cabs(msi: &Path, candidates: &[String]) -> Result<Vec<String>, MsvcError> {
    require_file(msi, "Windows SDK MSI")?;
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(msi)
        .map_err(|cause| storage("open", msi, cause))?;
    let metadata = file
        .metadata()
        .map_err(|cause| storage("inspect", msi, cause))?;
    if !metadata.is_file() || is_reparse(&metadata) {
        return Err(error(
            MsvcErrorKind::UnsafeStorage,
            format!("Windows SDK MSI must be a regular file: {}", msi.display()),
        ));
    }
    let length = metadata.len();
    if length > MAX_MSI_BYTES {
        return Err(error(
            MsvcErrorKind::InvalidSource,
            format!(
                "Windows SDK MSI exceeds its 512 MiB inspection limit: {}",
                msi.display()
            ),
        ));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.read_to_end(&mut bytes)
        .map_err(|cause| storage("read", msi, cause))?;
    Ok(candidates
        .iter()
        .filter(|candidate| contains_ascii_case_insensitive(&bytes, candidate.as_bytes()))
        .cloned()
        .collect())
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack.windows(needle.len()).any(|window| {
            window
                .iter()
                .zip(needle)
                .all(|(left, right)| left.eq_ignore_ascii_case(right))
        })
}

fn require_directory(path: &Path, subject: &str) -> Result<(), MsvcError> {
    let metadata = fs::symlink_metadata(path).map_err(|cause| storage("inspect", path, cause))?;
    if metadata.is_dir() && !is_reparse(&metadata) {
        Ok(())
    } else {
        Err(error(
            MsvcErrorKind::UnsafeStorage,
            format!("{subject} must be a regular directory: {}", path.display()),
        ))
    }
}

fn require_file(path: &Path, subject: &str) -> Result<(), MsvcError> {
    let metadata = fs::symlink_metadata(path).map_err(|cause| storage("inspect", path, cause))?;
    if metadata.is_file() && !is_reparse(&metadata) {
        Ok(())
    } else {
        Err(error(
            MsvcErrorKind::UnsafeStorage,
            format!("{subject} must be a regular file: {}", path.display()),
        ))
    }
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn storage(action: &str, path: &Path, cause: std::io::Error) -> MsvcError {
    error(
        MsvcErrorKind::InstallationFailed,
        format!(
            "cannot {action} MSVC installer source '{}': {cause}",
            path.display()
        ),
    )
}

#[cfg(test)]
mod tests;
