use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ureq::{
    Agent,
    tls::{RootCerts, TlsConfig},
};

use super::{event, path};

const MAX_DOWNLOAD_BYTES: u64 = 12 * 1024 * 1024 * 1024;

pub(crate) fn run(
    controlled_root: &Path,
    source: &OsStr,
    destination: &Path,
    progress_id: &str,
) -> Result<(), String> {
    let destination =
        path::controlled_destination(controlled_root, destination, "download destination")?;
    if destination.exists() {
        return Err(format!(
            "download destination already exists: {}",
            destination.display()
        ));
    }
    let file_name = destination
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("artifact");
    let message = format!("Downloading {file_name}");
    event::progress(progress_id, "running", Some(0), None, &message);

    let result = if let Some(source_path) = local_source(source) {
        copy_local(&source_path, &destination, progress_id, &message)
    } else {
        download_http(source, &destination, progress_id, &message)
    };
    match result {
        Ok(bytes) => {
            event::progress(
                progress_id,
                "completed",
                Some(bytes),
                Some(bytes),
                &format!("Downloaded {file_name}"),
            );
            Ok(())
        }
        Err(error) => {
            event::progress(
                progress_id,
                "failed",
                None,
                None,
                &format!("Download failed: {file_name}"),
            );
            Err(error)
        }
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
    progress_id: &str,
    message: &str,
) -> Result<u64, String> {
    let source = path::regular_file(source, "download source")?;
    let total = fs::metadata(&source)
        .map_err(|error| error.to_string())?
        .len();
    if total == 0 || total > MAX_DOWNLOAD_BYTES {
        return Err(format!(
            "download source has an invalid size: {}",
            source.display()
        ));
    }
    publish_stream(
        File::open(&source)
            .map_err(|error| format!("cannot open '{}': {error}", source.display()))?,
        destination,
        progress_id,
        message,
        Some(total),
    )
}

fn download_http(
    source: &OsStr,
    destination: &Path,
    progress_id: &str,
    message: &str,
) -> Result<u64, String> {
    let source = source
        .to_str()
        .ok_or_else(|| "download URL must be valid Unicode".to_owned())?;
    if !(source.starts_with("https://") || source.starts_with("http://")) {
        return Err("download source must be an absolute file or HTTP(S) URL".to_owned());
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
                if total.is_some_and(|value| value == 0 || value > MAX_DOWNLOAD_BYTES) {
                    return Err("HTTP download declares an invalid size".to_owned());
                }
                match publish_stream(
                    response.body_mut().as_reader(),
                    destination,
                    progress_id,
                    message,
                    total,
                ) {
                    Ok(bytes) => return Ok(bytes),
                    Err(error) => failures.push(format!("attempt {attempt}: {error}")),
                }
            }
            Err(error) => failures.push(format!("attempt {attempt}: {error}")),
        }
    }
    Err(format!(
        "download failed for '{source}': {}",
        failures.join("; ")
    ))
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
    progress_id: &str,
    message: &str,
    total: Option<u64>,
) -> Result<u64, String> {
    let stage = temporary_sibling(destination)?;
    let result = (|| {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage)
            .map_err(|error| {
                format!(
                    "cannot create download stage '{}': {error}",
                    stage.display()
                )
            })?;
        let mut buffer = [0u8; 64 * 1024];
        let mut bytes = 0u64;
        let mut reported = Instant::now();
        loop {
            let count = reader
                .read(&mut buffer)
                .map_err(|error| format!("download read failed: {error}"))?;
            if count == 0 {
                break;
            }
            bytes = bytes
                .checked_add(count as u64)
                .filter(|value| *value <= MAX_DOWNLOAD_BYTES)
                .ok_or_else(|| "download exceeds the 12 GB safety limit".to_owned())?;
            if total.is_some_and(|value| bytes > value) {
                return Err("download exceeds its declared Content-Length".to_owned());
            }
            output
                .write_all(&buffer[..count])
                .map_err(|error| format!("download write failed: {error}"))?;
            if reported.elapsed() >= Duration::from_millis(100) {
                event::progress(progress_id, "running", Some(bytes), total, message);
                reported = Instant::now();
            }
        }
        if bytes == 0 || total.is_some_and(|value| value != bytes) {
            return Err("download is empty or incomplete".to_owned());
        }
        output
            .sync_all()
            .map_err(|error| format!("cannot flush download: {error}"))?;
        drop(output);
        fs::rename(&stage, destination).map_err(|error| {
            format!(
                "cannot publish download '{}': {error}",
                destination.display()
            )
        })?;
        Ok(bytes)
    })();
    if stage.exists() {
        let _ = fs::remove_file(&stage);
    }
    result
}

fn temporary_sibling(destination: &Path) -> Result<PathBuf, String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "download destination has no parent".to_owned())?;
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
    Err("cannot allocate a unique download staging path".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn local_download_is_published_complete() {
        let root =
            env::temp_dir().join(format!("swawkit-toolchain-download-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("out")).unwrap();
        let source = root.join("source.bin");
        let destination = root.join("out/artifact.bin");
        fs::write(&source, b"fixture").unwrap();

        run(&root, source.as_os_str(), &destination, "download:test").unwrap();

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
