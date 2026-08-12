use std::fs;
use std::path::{Path, PathBuf};

use super::removal::{
    is_reparse, move_path_with_retry, path_exists, reject_reparse_or_missing,
    remove_path_with_retry, remove_residues, require_regular_directory, storage,
    target_parent_and_leaf, unsafe_path,
};
use super::validated_candidate;
use crate::development::archive_tool::{
    ArchiveToolError, ArchiveToolErrorKind, ArchiveToolStore, Installation, ResolvedDefinition,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in super::super) enum RecoveryOutcome {
    Ready(Installation),
    Recovered(Installation),
    Missing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in super::super) struct RecoveryReport {
    pub(in super::super) outcome: RecoveryOutcome,
    pub(in super::super) warnings: Vec<String>,
}

pub(in super::super) fn recover(
    store: &ArchiveToolStore<'_>,
    resolved: &ResolvedDefinition,
    target: &Path,
) -> Result<RecoveryReport, ArchiveToolError> {
    let (parent, _) = target_parent_and_leaf(target)?;
    require_regular_directory(parent, "installation parent")?;
    reject_reparse_or_missing(target, "installation target")?;

    let mut paths = scan_recovery_paths(target)?;
    if let Some(installation) = validated_candidate(store, resolved, target)? {
        let warnings = remove_residues(&paths.all());
        return Ok(RecoveryReport {
            outcome: RecoveryOutcome::Ready(installation),
            warnings,
        });
    }

    let mut valid_backups = Vec::new();
    for path in &paths.backups {
        if validated_candidate(store, resolved, path)?.is_some() {
            valid_backups.push(path.clone());
        }
    }

    if path_exists(target)? {
        remove_path_with_retry(target, "remove an invalid installation")?;
    }

    if let Some(selected) = select_backup(&valid_backups)? {
        move_path_with_retry(&selected, target, "restore the last valid installation")?;
        let installation = validated_candidate(store, resolved, target)?.ok_or_else(|| {
            ArchiveToolError::new(
                ArchiveToolErrorKind::InstalledFileInvalid,
                format!(
                    "the restored installation failed validation: {}",
                    target.display()
                ),
            )
        })?;
        paths = scan_recovery_paths(target)?;
        return Ok(RecoveryReport {
            outcome: RecoveryOutcome::Recovered(installation),
            warnings: remove_residues(&paths.all()),
        });
    }

    Ok(RecoveryReport {
        outcome: RecoveryOutcome::Missing,
        warnings: remove_residues(&paths.all()),
    })
}

#[derive(Debug)]
struct RecoveryPaths {
    backups: Vec<PathBuf>,
    work: Vec<PathBuf>,
}

impl RecoveryPaths {
    fn all(&self) -> Vec<PathBuf> {
        self.backups.iter().chain(&self.work).cloned().collect()
    }
}

fn scan_recovery_paths(target: &Path) -> Result<RecoveryPaths, ArchiveToolError> {
    let (parent, leaf) = target_parent_and_leaf(target)?;
    let leaf = leaf.to_string_lossy();
    let backup_prefix = format!("{leaf}.backup-").to_ascii_lowercase();
    let mut backups = Vec::new();
    let mut work = Vec::new();

    for entry in fs::read_dir(parent)
        .map_err(|error| storage("scan installation recovery directory", parent, error))?
    {
        let entry =
            entry.map_err(|error| storage("read installation recovery entry", parent, error))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        let is_backup = lower.starts_with(&backup_prefix);
        let is_work = is_strict_work_name(&name)
            || lower.starts_with(".work-")
            || lower.starts_with(".partial-");
        if !is_backup && !is_work {
            continue;
        }

        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| storage("inspect installation recovery path", &path, error))?;
        if is_reparse(&metadata) {
            return Err(unsafe_path("installation recovery path", &path));
        }
        if is_backup {
            backups.push(path);
        } else {
            work.push(path);
        }
    }
    Ok(RecoveryPaths { backups, work })
}

fn select_backup(backups: &[PathBuf]) -> Result<Option<PathBuf>, ArchiveToolError> {
    let mut timestamped = backups
        .iter()
        .filter_map(|path| backup_order(path).map(|order| (order, path.clone())))
        .collect::<Vec<_>>();
    timestamped.sort_by(|left, right| left.cmp(right));
    if let Some((_, path)) = timestamped.pop() {
        return Ok(Some(path));
    }
    match backups {
        [] => Ok(None),
        [only] => Ok(Some(only.clone())),
        _ => Err(ArchiveToolError::new(
            ArchiveToolErrorKind::RecoveryFailed,
            format!(
                "multiple valid legacy installation backups have unknowable creation order; \
                 manual repair is required: {}",
                backups
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

fn backup_order(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    let marker = ".backup-";
    let offset = name.to_ascii_lowercase().rfind(marker)? + marker.len();
    let suffix = &name[offset..];
    if suffix.len() != 23 + 1 + 32 {
        return None;
    }
    let (timestamp, token) = suffix.split_once('-')?;
    if !is_backup_timestamp(timestamp) || !is_hex(token, 32) {
        return None;
    }
    Some(timestamp.to_ascii_uppercase())
}

fn is_backup_timestamp(value: &str) -> bool {
    value.len() == 23
        && value.as_bytes()[8] == b'T'
        && value.as_bytes()[22].eq_ignore_ascii_case(&b'Z')
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 22) || byte.is_ascii_digit())
}

fn is_strict_work_name(name: &str) -> bool {
    let Some((stem, token)) = name.rsplit_once('-') else {
        return false;
    };
    if !is_hex(token, 32) || !stem.starts_with('.') {
        return false;
    }
    let lower = stem.to_ascii_lowercase();
    let leaf = lower
        .strip_suffix(".partial")
        .or_else(|| lower.strip_suffix(".work"))
        .and_then(|value| value.strip_prefix('.'));
    leaf.is_some_and(|leaf| {
        let mut bytes = leaf.bytes();
        bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && bytes.all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-')
            })
    })
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "swawkit-archive-recovery-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("create recovery fixture");
            Self { root }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn newest_timestamped_backup_wins_over_legacy_and_older_backups() {
        let backups = [
            PathBuf::from("bun.backup-valid"),
            PathBuf::from("bun.backup-20260801T0101010000000Z-11111111111111111111111111111111"),
            PathBuf::from("bun.backup-20260801T0101020000000Z-22222222222222222222222222222222"),
        ];

        assert_eq!(select_backup(&backups).unwrap(), Some(backups[2].clone()));
    }

    #[test]
    fn multiple_valid_legacy_backups_are_not_guessed() {
        let backups = [
            PathBuf::from("bun.backup-first"),
            PathBuf::from("bun.backup-second"),
        ];

        let error = select_backup(&backups).unwrap_err();
        assert!(error.to_string().contains("unknowable creation order"));
    }

    #[test]
    fn strict_cross_leaf_and_legacy_work_names_are_discovered() {
        assert!(is_strict_work_name(
            ".1.2.15.partial-11111111111111111111111111111111"
        ));
        assert!(is_strict_work_name(
            ".nightly-2026-07-31.work-22222222222222222222222222222222"
        ));
        assert!(!is_strict_work_name(
            ".partial-11111111111111111111111111111111"
        ));
        assert!(!is_strict_work_name(".bun.work-not-a-token"));
    }

    #[test]
    fn recognized_reparse_recovery_paths_fail_closed() {
        let fixture = Fixture::new();
        let external = fixture.root.join("external");
        let link = fixture.root.join("bun.backup-valid");
        fs::create_dir(&external).expect("create external directory");
        if let Err(error) = std::os::windows::fs::symlink_dir(&external, &link) {
            if error.kind() == io::ErrorKind::PermissionDenied {
                return;
            }
            panic!("create recovery reparse point: {error}");
        }

        let error = scan_recovery_paths(&fixture.root.join("bun")).unwrap_err();
        assert!(error.to_string().contains("must not be a reparse point"));
        assert!(external.is_dir());
    }
}
