use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::atomic_file;
use crate::entry::{EntryIdentity, is_valid_file_id, is_valid_volume_id};

pub const ENTRY_RECORD_SCHEMA: &str = "swawkit.proj-entry.v0";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryRecord {
    pub schema: String,
    pub entry_name: String,
    pub entry_file: Option<String>,
    pub volume_id: String,
    pub file_id: String,
}

impl EntryRecord {
    pub fn matches_identity(&self, identity: &EntryIdentity) -> bool {
        self.volume_id == identity.volume_id() && self.file_id == identity.file_id()
    }

    fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("schema", self.schema.as_str()),
            ("entryName", self.entry_name.as_str()),
            ("volumeId", self.volume_id.as_str()),
            ("fileId", self.file_id.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(format!("required property '{name}' is missing"));
            }
        }
        if self.schema != ENTRY_RECORD_SCHEMA {
            return Err(format!("unsupported schema '{}'", self.schema));
        }
        if !is_valid_volume_id(&self.volume_id) {
            return Err("volumeId is invalid".to_owned());
        }
        if !is_valid_file_id(&self.file_id) {
            return Err("fileId is invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryRecordState {
    Missing { path: PathBuf },
    Invalid { path: PathBuf, error: String },
    Valid { path: PathBuf, record: EntryRecord },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EntryRecordFingerprint {
    Missing,
    PresentSha256(String),
    Unreadable,
}

impl EntryRecordFingerprint {
    fn present(content: &[u8]) -> Self {
        Self::PresentSha256(format!("{:x}", Sha256::digest(content)))
    }

    pub(crate) fn revision(&self) -> String {
        match self {
            Self::Missing => "missing".to_owned(),
            Self::PresentSha256(digest) => format!("sha256-{digest}"),
            Self::Unreadable => "unreadable".to_owned(),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_state(state: &EntryRecordState) -> Self {
        match state {
            EntryRecordState::Missing { .. } => Self::Missing,
            EntryRecordState::Invalid { error, .. } => Self::present(error.as_bytes()),
            EntryRecordState::Valid { record, .. } => {
                Self::present(&serde_json::to_vec(record).expect("serialize fixture entry record"))
            }
        }
    }
}

pub(crate) struct EntryRecordRead {
    pub(crate) state: EntryRecordState,
    pub(crate) fingerprint: EntryRecordFingerprint,
}

impl EntryRecordState {
    pub fn valid_record(&self) -> Option<&EntryRecord> {
        match self {
            Self::Valid { record, .. } => Some(record),
            Self::Missing { .. } | Self::Invalid { .. } => None,
        }
    }

    pub fn invalid_reason(&self) -> Option<&str> {
        match self {
            Self::Missing { .. } => Some("identity record is missing"),
            Self::Invalid { error, .. } => Some(error),
            Self::Valid { .. } => None,
        }
    }
}

pub fn read_entry_record(data_root: &Path) -> EntryRecordState {
    read_entry_record_with_fingerprint(data_root).state
}

pub(crate) fn read_entry_record_with_fingerprint(data_root: &Path) -> EntryRecordRead {
    let path = data_root.join("_entry.json");
    let content = match fs::read(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return EntryRecordRead {
                state: EntryRecordState::Missing { path },
                fingerprint: EntryRecordFingerprint::Missing,
            };
        }
        Err(error) => {
            return EntryRecordRead {
                state: EntryRecordState::Invalid {
                    path,
                    error: error.to_string(),
                },
                fingerprint: EntryRecordFingerprint::Unreadable,
            };
        }
    };
    let fingerprint = EntryRecordFingerprint::present(&content);
    let result = parse_entry_record(&content);
    let state = match result {
        Ok(record) => EntryRecordState::Valid { path, record },
        Err(error) => EntryRecordState::Invalid { path, error },
    };
    EntryRecordRead { state, fingerprint }
}

pub(crate) fn parse_entry_record(content: &[u8]) -> Result<EntryRecord, String> {
    serde_json::from_slice::<EntryRecord>(content)
        .map_err(|error| error.to_string())
        .and_then(|record| {
            record.validate()?;
            Ok(record)
        })
}

pub(crate) fn publish_entry_record(
    data_root: &Path,
    entry_name: &str,
    entry_file: &Path,
    identity: &EntryIdentity,
) -> Result<(), EntryRecordWriteError> {
    let data_root_metadata = fs::symlink_metadata(data_root).map_err(|error| {
        EntryRecordWriteError::new(format!(
            "cannot inspect DataRoot '{}': {error}",
            data_root.display()
        ))
    })?;
    if data_root_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(EntryRecordWriteError::new(format!(
            "project DataRoot cannot be a reparse point: {}",
            data_root.display()
        )));
    }
    if !data_root_metadata.is_dir() {
        return Err(EntryRecordWriteError::new(format!(
            "cannot publish identity for a missing DataRoot: {}",
            data_root.display()
        )));
    }
    let entry_file_name = entry_file
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| {
            EntryRecordWriteError::new(format!(
                "project entry file has no usable Unicode name: {}",
                entry_file.display()
            ))
        })?;
    let record = EntryRecord {
        schema: ENTRY_RECORD_SCHEMA.to_owned(),
        entry_name: entry_name.to_owned(),
        entry_file: Some(entry_file_name.to_owned()),
        volume_id: identity.volume_id().to_owned(),
        file_id: identity.file_id().to_owned(),
    };
    let mut content = serde_json::to_string_pretty(&record).map_err(|error| {
        EntryRecordWriteError::new(format!("cannot serialize project entry identity: {error}"))
    })?;
    content.push('\n');

    let record_path = data_root.join("_entry.json");
    atomic_file::publish(&record_path, content.as_bytes()).map_err(|error| {
        EntryRecordWriteError::new(format!(
            "cannot publish project entry identity '{}': {error}",
            record_path.display()
        ))
    })
}

#[derive(Debug)]
pub(crate) struct EntryRecordWriteError {
    message: String,
}

impl EntryRecordWriteError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for EntryRecordWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for EntryRecordWriteError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "swawkit-entry-record-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("create fixture");
            Self(path)
        }

        fn write(&self, content: &str) {
            fs::write(self.0.join("_entry.json"), content).expect("write identity record");
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn distinguishes_missing_invalid_and_valid_records() {
        let fixture = Fixture::new();
        assert!(matches!(
            read_entry_record(&fixture.0),
            EntryRecordState::Missing { .. }
        ));

        fixture.write(r#"{"schema":"wrong"}"#);
        assert!(matches!(
            read_entry_record(&fixture.0),
            EntryRecordState::Invalid { .. }
        ));

        fixture.write(
            r#"{
                "schema":"swawkit.proj-entry.v0",
                "entryName":"project-one",
                "entryFile":"project-one.exe",
                "volumeId":"\\\\?\\volume{91cf565a-694f-4232-be2d-368578d28629}",
                "fileId":"0000000000000000001400000000685d"
            }"#,
        );
        assert!(matches!(
            read_entry_record(&fixture.0),
            EntryRecordState::Valid { .. }
        ));
    }

    #[test]
    fn fingerprints_missing_present_and_unreadable_records() {
        let fixture = Fixture::new();
        let missing = read_entry_record_with_fingerprint(&fixture.0);
        assert_eq!(missing.fingerprint.revision(), "missing");

        fixture.write("first invalid record");
        let first = read_entry_record_with_fingerprint(&fixture.0);
        fixture.write("second invalid record");
        let second = read_entry_record_with_fingerprint(&fixture.0);
        assert!(first.fingerprint.revision().starts_with("sha256-"));
        assert_ne!(first.fingerprint, second.fingerprint);

        fs::remove_file(fixture.0.join("_entry.json")).expect("remove readable record");
        fs::create_dir(fixture.0.join("_entry.json")).expect("create unreadable record path");
        let unreadable = read_entry_record_with_fingerprint(&fixture.0);
        assert_eq!(unreadable.fingerprint.revision(), "unreadable");
        assert!(matches!(unreadable.state, EntryRecordState::Invalid { .. }));
    }

    #[test]
    fn keeps_case_significant_for_persisted_identity_matching() {
        let identity = EntryIdentity::from_parts(
            r"\\?\volume{91cf565a-694f-4232-be2d-368578d28629}",
            "0000000000000000001400000000685d",
        )
        .expect("identity");
        let record = EntryRecord {
            schema: ENTRY_RECORD_SCHEMA.to_owned(),
            entry_name: "project-one".to_owned(),
            entry_file: None,
            volume_id: identity.volume_id().to_uppercase(),
            file_id: identity.file_id().to_owned(),
        };
        assert!(!record.matches_identity(&identity));
    }
}
