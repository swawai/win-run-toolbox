use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ureq::{
    Agent,
    tls::{RootCerts, TlsConfig},
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

use super::super::{ArchiveToolError, ArchiveToolErrorKind};

const MAX_TRANSFER_BYTES: u64 = 12 * 1024 * 1024 * 1024;

pub(super) fn transfer(
    source: &OsStr,
    destination: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<u64, ArchiveToolError> {
    if destination.exists() {
        return Err(storage_error(format!(
            "download destination already exists: {}",
            destination.display()
        )));
    }
    if let Some(source_path) = local_source(source) {
        copy_local(&source_path, destination, progress)
    } else {
        download_http(source, destination, progress)
    }
}

fn local_source(source: &OsStr) -> Option<PathBuf> {
    let path = PathBuf::from(source);
    path.is_absolute()
        .then_some(path)
        .filter(|path| path.exists())
}

fn copy_local(
    source: &Path,
    destination: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<u64, ArchiveToolError> {
    let input = open_regular(source, "download source")?;
    let total = input
        .metadata()
        .map_err(|cause| io_error("inspect download source", source, cause))?
        .len();
    if total == 0 || total > MAX_TRANSFER_BYTES {
        return Err(download_error(format!(
            "download source has an invalid size: {}",
            source.display()
        )));
    }
    publish_stream(input, destination, Some(total), progress)
}

fn download_http(
    source: &OsStr,
    destination: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<u64, ArchiveToolError> {
    let source = source
        .to_str()
        .ok_or_else(|| download_error("download URL must be valid Unicode"))?;
    if !(source.starts_with("https://") || source.starts_with("http://")) {
        return Err(download_error(
            "download source must be an absolute file or HTTP(S) URL",
        ));
    }
    let agent = download_agent();
    let mut failures = Vec::new();
    for attempt in 1..=3 {
        match agent.get(source).call() {
            Ok(mut response) => {
                let total = response
                    .headers()
                    .get("content-length")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok());
                if total.is_some_and(|value| value == 0 || value > MAX_TRANSFER_BYTES) {
                    return Err(download_error("HTTP download declares an invalid size"));
                }
                match publish_stream(
                    response.body_mut().as_reader(),
                    destination,
                    total,
                    progress,
                ) {
                    Ok(bytes) => return Ok(bytes),
                    Err(cause) => failures.push(format!("attempt {attempt}: {cause}")),
                }
            }
            Err(cause) => failures.push(format!("attempt {attempt}: {cause}")),
        }
    }
    Err(download_error(format!(
        "download failed for '{source}': {}",
        failures.join("; ")
    )))
}

fn download_agent() -> Agent {
    Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(900)))
        .timeout_connect(Some(Duration::from_secs(30)))
        .max_redirects(10)
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into()
}

fn publish_stream(
    mut reader: impl Read,
    destination: &Path,
    total: Option<u64>,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<u64, ArchiveToolError> {
    let stage = temporary_sibling(destination)?;
    let mut stage_created = false;
    let result = (|| {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)
            .map_err(|cause| {
                storage_error(format!(
                    "cannot create download stage '{}': {cause}",
                    stage.display()
                ))
            })?;
        stage_created = true;
        let mut buffer = [0u8; 64 * 1024];
        let mut bytes = 0u64;
        let mut reported = Instant::now();
        loop {
            let count = reader
                .read(&mut buffer)
                .map_err(|cause| download_error(format!("download read failed: {cause}")))?;
            if count == 0 {
                break;
            }
            bytes = bytes
                .checked_add(count as u64)
                .filter(|value| *value <= MAX_TRANSFER_BYTES)
                .ok_or_else(|| download_error("download exceeds the 12 GB safety limit"))?;
            if total.is_some_and(|value| bytes > value) {
                return Err(download_error(
                    "download exceeds its declared Content-Length",
                ));
            }
            output
                .write_all(&buffer[..count])
                .map_err(|cause| storage_error(format!("download write failed: {cause}")))?;
            if reported.elapsed() >= Duration::from_millis(100) {
                progress(bytes, total);
                reported = Instant::now();
            }
        }
        if bytes == 0 || total.is_some_and(|value| value != bytes) {
            return Err(download_error("download is empty or incomplete"));
        }
        output
            .sync_all()
            .map_err(|cause| storage_error(format!("cannot flush download: {cause}")))?;
        drop(output);
        fs::rename(&stage, destination).map_err(|cause| {
            storage_error(format!(
                "cannot publish download '{}': {cause}",
                destination.display()
            ))
        })?;
        stage_created = false;
        Ok(bytes)
    })();
    if stage_created {
        let _ = fs::remove_file(&stage);
    }
    result
}

fn temporary_sibling(destination: &Path) -> Result<PathBuf, ArchiveToolError> {
    let parent = destination
        .parent()
        .ok_or_else(|| storage_error("download destination has no parent"))?;
    for sequence in 0..1000u32 {
        let candidate = parent.join(format!(
            ".{}.{}.{sequence}.tmp",
            destination
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("artifact"),
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(storage_error(
        "cannot allocate a unique download staging path",
    ))
}

fn open_regular(path: &Path, subject: &str) -> Result<File, ArchiveToolError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|cause| io_error(&format!("open {subject}"), path, cause))?;
    let metadata = file
        .metadata()
        .map_err(|cause| io_error(&format!("inspect {subject}"), path, cause))?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!("{subject} is not a regular file: {}", path.display()),
        ));
    }
    Ok(file)
}

fn io_error(action: &str, path: &Path, cause: std::io::Error) -> ArchiveToolError {
    ArchiveToolError::new(
        ArchiveToolErrorKind::Storage,
        format!("cannot {action} '{}': {cause}", path.display()),
    )
}

fn storage_error(message: impl Into<String>) -> ArchiveToolError {
    ArchiveToolError::new(ArchiveToolErrorKind::Storage, message)
}

fn download_error(message: impl Into<String>) -> ArchiveToolError {
    ArchiveToolError::new(ArchiveToolErrorKind::DownloadFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn local_transfer_is_published_complete() {
        let root = env::temp_dir().join(format!("swawkit-archive-transfer-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("out")).unwrap();
        let source = root.join("source.bin");
        let destination = root.join("out/artifact.bin");
        fs::write(&source, b"fixture").unwrap();

        let bytes = transfer(source.as_os_str(), &destination, &mut |_, _| {}).unwrap();

        assert_eq!(bytes, 7);
        assert_eq!(fs::read(destination).unwrap(), b"fixture");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn http_downloads_use_the_windows_certificate_verifier() {
        let agent = download_agent();
        assert!(matches!(
            agent.config().tls_config().root_certs(),
            RootCerts::PlatformVerifier
        ));
    }
}
