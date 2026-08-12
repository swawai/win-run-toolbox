use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::{ArchiveToolError, ArchiveToolErrorKind};

#[path = "lock.rs"]
mod lock;
#[path = "recovery.rs"]
mod recovery;
#[path = "removal.rs"]
mod removal;

pub(super) use lock::ExclusiveFileLock;
pub(super) use recovery::{RecoveryOutcome, recover, recover_with};
pub(super) use removal::{
    remove_controlled, remove_residues, remove_residues_with, with_cleanup_warnings,
};

use removal::{
    move_path_with_retry, path_exists, reject_reparse_or_missing, remove_path_with_retry_with,
    require_regular_directory, target_parent_and_leaf,
};

static NEXT_TOKEN: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkKind {
    Work,
    Partial,
}

impl WorkKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Work => "work",
            Self::Partial => "partial",
        }
    }
}

pub(super) fn work_path(target: &Path, kind: WorkKind) -> Result<PathBuf, ArchiveToolError> {
    let (parent, leaf) = target_parent_and_leaf(target)?;
    Ok(parent.join(format!(
        ".{}.{}-{}",
        leaf.to_string_lossy(),
        kind.as_str(),
        fresh_token()
    )))
}

/// Publishes a fully prepared sibling directory and verifies the formal target.
///
/// A failed rollback never destroys its remaining evidence. The error reports
/// the exact target and backup paths so the next recovery pass can finish it.
pub(super) fn publish<T, F>(
    staged: &Path,
    target: &Path,
    validate: &mut F,
) -> Result<(T, Vec<String>), ArchiveToolError>
where
    F: FnMut(&Path) -> Result<Option<T>, ArchiveToolError>,
{
    publish_with(staged, target, validate, |_| Ok(()))
}

pub(super) fn publish_with<T, F>(
    staged: &Path,
    target: &Path,
    validate: &mut F,
    prepare_removal: fn(&Path) -> Result<(), ArchiveToolError>,
) -> Result<(T, Vec<String>), ArchiveToolError>
where
    F: FnMut(&Path) -> Result<Option<T>, ArchiveToolError>,
{
    require_siblings(staged, target)?;
    match publish_inner(staged, target, validate, prepare_removal) {
        Ok(result) => Ok(result),
        Err(error) => {
            let warnings = remove_residues_with(&[staged.to_path_buf()], prepare_removal);
            Err(with_cleanup_warnings(error, &warnings))
        }
    }
}

fn publish_inner<T, F>(
    staged: &Path,
    target: &Path,
    validate: &mut F,
    prepare_removal: fn(&Path) -> Result<(), ArchiveToolError>,
) -> Result<(T, Vec<String>), ArchiveToolError>
where
    F: FnMut(&Path) -> Result<Option<T>, ArchiveToolError>,
{
    reject_reparse_or_missing(staged, "staged installation")?;
    if validate(staged)?.is_none() {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::InstalledFileInvalid,
            format!(
                "staged installation failed validation: {}",
                staged.display()
            ),
        ));
    }

    reject_reparse_or_missing(target, "installation target")?;
    let backup = backup_path(target)?;
    let had_target = path_exists(target)?;
    let mut published = false;
    if had_target {
        move_path_with_retry(target, &backup, "preserve the previous installation")?;
    }

    let publication = (|| {
        move_path_with_retry(staged, target, "publish the staged installation")?;
        published = true;
        validate(target)?.ok_or_else(|| {
            ArchiveToolError::new(
                ArchiveToolErrorKind::InstalledFileInvalid,
                format!(
                    "published installation failed validation: {}",
                    target.display()
                ),
            )
        })
    })();

    let installation = match publication {
        Ok(installation) => installation,
        Err(publication_error) => {
            let rollback = rollback(target, &backup, had_target, published, prepare_removal);
            if let Err(rollback_error) = rollback {
                let target_remains = path_exists(target).unwrap_or(true);
                let backup_remains = path_exists(&backup).unwrap_or(true);
                let detail = rollback_detail(target, &backup, target_remains, backup_remains);
                return Err(ArchiveToolError::new(
                    ArchiveToolErrorKind::Storage,
                    format!(
                        "installation publication failed and rollback is pending. {detail} \
                         Original error: {publication_error}. Rollback error: \
                         {rollback_error}."
                    ),
                ));
            }
            return Err(publication_error);
        }
    };

    let mut residues = vec![staged.to_path_buf()];
    if had_target {
        residues.push(backup);
    }
    Ok((
        installation,
        remove_residues_with(&residues, prepare_removal),
    ))
}

fn rollback(
    target: &Path,
    backup: &Path,
    had_target: bool,
    published: bool,
    prepare_removal: fn(&Path) -> Result<(), ArchiveToolError>,
) -> Result<(), ArchiveToolError> {
    if published && path_exists(target)? {
        remove_path_with_retry_with(target, "roll back a failed installation", prepare_removal)?;
    }
    if had_target && path_exists(backup)? {
        move_path_with_retry(backup, target, "restore the previous installation")?;
    }
    Ok(())
}

fn rollback_detail(
    target: &Path,
    backup: &Path,
    target_remains: bool,
    backup_remains: bool,
) -> String {
    match (target_remains, backup_remains) {
        (true, true) => format!(
            "The failed target remains at '{}', and the previous installation backup is \
             preserved at '{}'.",
            target.display(),
            backup.display()
        ),
        (false, true) => format!(
            "The previous installation backup is preserved at '{}'.",
            backup.display()
        ),
        (true, false) => format!(
            "No previous installation backup was available; the failed target remains at '{}'.",
            target.display()
        ),
        (false, false) => "No recoverable installation path could be confirmed.".to_owned(),
    }
}

fn backup_path(target: &Path) -> Result<PathBuf, ArchiveToolError> {
    let timestamp = utc_backup_timestamp(SystemTime::now());
    let name = target.file_name().ok_or_else(|| {
        ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!("installation target has no file name: {}", target.display()),
        )
    })?;
    let mut backup_name = name.to_os_string();
    backup_name.push(format!(".backup-{timestamp}-{}", fresh_token()));
    Ok(target.with_file_name(backup_name))
}

fn utc_backup_timestamp(time: SystemTime) -> String {
    let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = duration.as_secs();
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = seconds_of_day % 3_600 / 60;
    let second = seconds_of_day % 60;
    let fraction = duration.subsec_nanos() / 100;
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}{fraction:07}Z")
}

// Howard Hinnant's civil calendar conversion, with day zero at 1970-01-01.
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month as u32, day as u32)
}

fn fresh_token() -> String {
    let sequence = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    format!(
        "{nanos:016x}{:08x}{:08x}",
        std::process::id(),
        sequence as u32
    )
}

fn require_siblings(staged: &Path, target: &Path) -> Result<(), ArchiveToolError> {
    let (staged_parent, _) = target_parent_and_leaf(staged)?;
    let (target_parent, _) = target_parent_and_leaf(target)?;
    if staged_parent != target_parent {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!(
                "staged and target installations must be siblings on the same volume: '{}' and '{}'",
                staged.display(),
                target.display()
            ),
        ));
    }
    require_regular_directory(target_parent, "installation parent")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::os::windows::fs::OpenOptionsExt;
    use std::time::Duration;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "swawkit-archive-transaction-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("create transaction fixture");
            Self { root }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn backup_timestamp_is_power_shell_compatible() {
        let timestamp = utc_backup_timestamp(UNIX_EPOCH + Duration::from_nanos(1_650_000_000));
        assert_eq!(timestamp, "19700101T0000016500000Z");
    }

    #[test]
    fn a_locked_failed_target_preserves_the_recoverable_backup() {
        let fixture = Fixture::new();
        let target = fixture.root.join("bun");
        let backup = fixture.root.join("bun.backup-valid");
        fs::create_dir(&target).expect("create failed target");
        fs::create_dir(&backup).expect("create backup");
        let locked_path = target.join("bun.exe");
        fs::write(&locked_path, b"locked").expect("write locked target");
        let _lock = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&locked_path)
            .expect("lock failed target");

        let error = rollback(&target, &backup, true, true, |_| Ok(())).unwrap_err();

        assert!(error.to_string().contains("after 5 attempts"));
        assert!(target.is_dir());
        assert!(backup.is_dir());
    }
}
