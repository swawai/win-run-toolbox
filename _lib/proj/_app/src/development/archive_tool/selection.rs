use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

use super::super::is_semantic_version;
use super::filesystem::{
    MAX_SELECTION_BYTES, directory_chain, is_lower_hex, optional_directory_chain,
    optional_regular_file, read_json,
};
use super::{
    ArchiveToolError, ArchiveToolErrorKind, ArchiveToolStore, Installation, ResolvedDefinition,
    Selection, SourceVerification,
};

const SELECTION_FILE: &str = ".swawkit-dev-selection.json";
static NEXT_SELECTION_PUBLICATION: AtomicU64 = AtomicU64::new(0);

impl ArchiveToolStore<'_> {
    pub fn read_selection(&self) -> Result<Option<Selection>, ArchiveToolError> {
        let Some(root) = optional_directory_chain(
            self.data_root,
            &selection_components(self.tool.name),
            "tool version selection directory",
        )
        .map_err(selection_read_error)?
        else {
            return Ok(None);
        };
        let path = root.join(SELECTION_FILE);
        if !optional_regular_file(&path, "tool version selection").map_err(selection_read_error)? {
            return Ok(None);
        }
        let selection: Selection = read_json(&path, "tool version selection", MAX_SELECTION_BYTES)
            .map_err(selection_read_error)?;
        if !valid_selection(self, &selection) {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::SelectionInvalid,
                format!(
                    "the {} version selection is invalid: {}",
                    self.tool.display_name,
                    path.display()
                ),
            ));
        }
        Ok(Some(selection))
    }

    /// Publishes the immutable result of a successful `latest` installation.
    ///
    /// A different existing selection is never replaced implicitly. Advancing
    /// `latest` requires an explicit update operation so ordinary setup cannot
    /// silently change a project's selected tool version.
    pub(super) fn publish_latest_selection(
        &self,
        resolved: &ResolvedDefinition,
        installation: &Installation,
    ) -> Result<(), ArchiveToolError> {
        if !resolved.requested_latest() {
            return Ok(());
        }
        self.require_tool(resolved.tool_name())?;
        self.require_tool(&installation.tool_name)?;
        self.validate_metadata(resolved, installation.metadata())?;
        let verification = installation.metadata().source_verification();
        if !matches!(
            verification,
            SourceVerification::Github | SourceVerification::Unverified
        ) {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::InvalidInstallRequest,
                "a latest selection requires github or unverified installation metadata",
            ));
        }
        let proposed = Selection {
            schema: self.tool.selection_schema.to_owned(),
            selector: "latest".to_owned(),
            version: installation.metadata.version.clone(),
            source_sha256: installation.metadata.source_sha256().to_owned(),
            source_verification: verification,
        };
        match self.read_selection()? {
            Some(existing) if existing == proposed => return Ok(()),
            Some(existing) => return Err(selection_conflict(self, &existing, &proposed)),
            None => {}
        }

        let root = directory_chain(
            self.data_root,
            &selection_components(self.tool.name),
            "tool version selection directory",
        )
        .map_err(selection_write_error)?;
        let path = root.join(SELECTION_FILE);
        // Revalidate the leaf immediately before publication. This catches a
        // reparse point or non-file introduced after the preceding read.
        optional_regular_file(&path, "tool version selection").map_err(selection_write_error)?;
        let pretty = serde_json::to_vec_pretty(&proposed).map_err(|error| {
            ArchiveToolError::new(
                ArchiveToolErrorKind::Storage,
                format!("cannot serialize the tool version selection: {error}"),
            )
        })?;
        // PowerShell and Rust exchange semantic JSON. Keep the established
        // UTF-8/no-BOM and CRLF convention without depending on indentation.
        let mut content = Vec::with_capacity(pretty.len() + 16);
        for byte in pretty {
            if byte == b'\n' {
                content.push(b'\r');
            }
            content.push(byte);
        }
        content.extend_from_slice(b"\r\n");
        if let Err(error) = publish_immutable(&path, &content) {
            // A concurrent publisher may have won the create race. Accept the
            // same immutable value, but never overwrite a different one.
            return match self.read_selection() {
                Ok(Some(existing)) if existing == proposed => Ok(()),
                Ok(Some(existing)) => Err(selection_conflict(self, &existing, &proposed)),
                Err(read_error) => Err(read_error),
                Ok(None) => Err(ArchiveToolError::new(
                    ArchiveToolErrorKind::Storage,
                    format!(
                        "cannot publish the {} latest selection '{}': {error}",
                        self.tool.display_name,
                        path.display()
                    ),
                )),
            };
        }
        Ok(())
    }
}

/// Atomically creates `path` but never replaces an existing winner.
///
/// The shared atomic-file publisher intentionally supports replacement, while
/// latest selection is a compare-and-set record. Keeping this primitive local
/// makes the no-implicit-upgrade invariant explicit.
fn publish_immutable(path: &Path, content: &[u8]) -> io::Result<()> {
    let directory = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "selection publication path has no parent",
        )
    })?;
    let temporary = unique_sibling(directory);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    if let Err(error) = file.write_all(content).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);

    let existing = match canonical_sibling(&temporary) {
        Ok(path) => path,
        Err(error) => return cleanup_temporary(&temporary, error),
    };
    let destination = match canonical_sibling(path) {
        Ok(path) => path,
        Err(error) => return cleanup_temporary(&temporary, error),
    };
    let existing = null_terminated(existing.as_os_str());
    let destination = null_terminated(destination.as_os_str());
    let succeeded = unsafe {
        MoveFileExW(
            existing.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_WRITE_THROUGH,
        )
    };
    if succeeded == 0 {
        let error = io::Error::last_os_error();
        return cleanup_temporary(&temporary, error);
    }
    Ok(())
}

fn cleanup_temporary<T>(temporary: &Path, error: io::Error) -> io::Result<T> {
    match fs::remove_file(temporary) {
        Ok(()) => Err(error),
        Err(cleanup_error) if cleanup_error.kind() == io::ErrorKind::NotFound => Err(error),
        Err(cleanup_error) => Err(io::Error::new(
            error.kind(),
            format!(
                "{error}; selection publication temporary '{}' could not be removed: {cleanup_error}",
                temporary.display()
            ),
        )),
    }
}

fn canonical_sibling(path: &Path) -> io::Result<PathBuf> {
    let directory = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    Ok(fs::canonicalize(directory)?.join(name))
}

fn unique_sibling(directory: &Path) -> PathBuf {
    let sequence = NEXT_SELECTION_PUBLICATION.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    directory.join(format!(
        ".swawkit-selection.{}.{timestamp}.{sequence}.tmp",
        std::process::id()
    ))
}

fn null_terminated(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn selection_components<'a>(tool: &'a str) -> [&'a str; 6] {
    ["modules", "kernel", ".dev", "setup", "export", tool]
}

fn valid_selection(store: &ArchiveToolStore<'_>, selection: &Selection) -> bool {
    selection.schema == store.tool.selection_schema
        && selection.selector == "latest"
        && is_semantic_version(&selection.version)
        && is_lower_hex(&selection.source_sha256, 64)
        && matches!(
            selection.source_verification,
            SourceVerification::Github | SourceVerification::Unverified
        )
}

fn selection_conflict(
    store: &ArchiveToolStore<'_>,
    existing: &Selection,
    proposed: &Selection,
) -> ArchiveToolError {
    ArchiveToolError::new(
        ArchiveToolErrorKind::SelectionConflict,
        format!(
            "the {} latest selection is already {} ({}) and cannot be changed implicitly to {} ({})",
            store.tool.display_name,
            existing.version,
            existing.source_sha256,
            proposed.version,
            proposed.source_sha256
        ),
    )
}

fn selection_read_error(error: ArchiveToolError) -> ArchiveToolError {
    match error.kind() {
        ArchiveToolErrorKind::UnsafeStorage | ArchiveToolErrorKind::Storage => error,
        _ => error.with_kind(ArchiveToolErrorKind::SelectionUnreadable),
    }
}

fn selection_write_error(error: ArchiveToolError) -> ArchiveToolError {
    match error.kind() {
        ArchiveToolErrorKind::UnsafeStorage => error,
        _ => error.with_kind(ArchiveToolErrorKind::Storage),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use super::*;

    #[test]
    fn concurrent_different_publications_never_replace_the_winner() {
        let root = std::env::temp_dir().join(format!(
            "swawkit-selection-race-{}-{}",
            std::process::id(),
            NEXT_SELECTION_PUBLICATION.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join(SELECTION_FILE);
        let barrier = Arc::new(Barrier::new(3));
        let workers: Vec<_> = [b"first".as_slice(), b"second".as_slice()]
            .into_iter()
            .map(|content| {
                let path = path.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    publish_immutable(&path, content)
                })
            })
            .collect();
        barrier.wait();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let winner = fs::read(&path).unwrap();
        assert!(winner == b"first" || winner == b"second");
        assert!(fs::read_dir(&root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
        fs::remove_dir_all(root).unwrap();
    }
}
