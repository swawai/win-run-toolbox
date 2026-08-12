use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveToolErrorKind {
    InvalidVersion,
    InvalidProjectSha256,
    LatestWithProjectSha256,
    DefinitionMismatch,
    ExportUnavailable,
    SelectionUnreadable,
    SelectionInvalid,
    InstallationUnavailable,
    MetadataUnreadable,
    MetadataStale,
    DuplicateFileRecords,
    MissingFileRecord,
    InvalidFileRecord,
    InstalledFileInvalid,
    MissingStorage,
    UnsafeStorage,
    Storage,
    InvalidDocument,
    FileMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveToolError {
    kind: ArchiveToolErrorKind,
    message: String,
}

impl ArchiveToolError {
    pub(super) fn new(kind: ArchiveToolErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(super) fn with_kind(mut self, kind: ArchiveToolErrorKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn kind(&self) -> ArchiveToolErrorKind {
        self.kind
    }
}

impl fmt::Display for ArchiveToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ArchiveToolError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveToolRequest {
    pub(super) tool_name: String,
    pub(super) requested: String,
    pub(super) project_sha256: String,
}

impl ArchiveToolRequest {
    pub fn requested(&self) -> &str {
        &self.requested
    }

    pub fn project_sha256(&self) -> &str {
        &self.project_sha256
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SourceVerification {
    Github,
    Project,
    Unverified,
}

impl SourceVerification {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Project => "project",
            Self::Unverified => "unverified",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolvedVerification {
    Published(SourceVerification),
    Unresolved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub(super) schema: String,
    pub(super) selector: String,
    pub(super) version: String,
    pub(super) source_sha256: String,
    pub(super) source_verification: SourceVerification,
}

impl Selection {
    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    pub fn source_verification(&self) -> SourceVerification {
        self.source_verification
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedDefinition {
    pub(super) tool_name: String,
    pub(super) requested_latest: bool,
    pub(super) version: String,
    pub(super) source_sha256: Option<String>,
    pub(super) verification: ResolvedVerification,
    pub(super) project_sha256: String,
}

impl ResolvedDefinition {
    pub fn requested_latest(&self) -> bool {
        self.requested_latest
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn source_sha256(&self) -> Option<&str> {
        self.source_sha256.as_deref()
    }

    pub fn verification(&self) -> ResolvedVerification {
        self.verification
    }

    pub fn project_sha256(&self) -> &str {
        &self.project_sha256
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallMetadata {
    pub(super) schema: String,
    pub(super) name: String,
    pub(super) version: String,
    pub(super) source_url: String,
    pub(super) source_sha256: String,
    pub(super) source_verification: SourceVerification,
    pub(super) recipe_version: String,
    pub(super) definition_signature: String,
    pub(super) files: Vec<InstalledFile>,
}

impl InstallMetadata {
    pub fn source_url(&self) -> &str {
        &self.source_url
    }

    pub fn source_sha256(&self) -> &str {
        &self.source_sha256
    }

    pub fn source_verification(&self) -> SourceVerification {
        self.source_verification
    }

    pub fn files(&self) -> &[InstalledFile] {
        &self.files
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InstalledFile {
    pub(super) path: String,
    pub(super) length: u64,
    pub(super) sha256: String,
}

impl InstalledFile {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Installation {
    pub(super) tool_name: String,
    pub(super) root: PathBuf,
    pub(super) executable: PathBuf,
    pub(super) metadata: InstallMetadata,
}

impl Installation {
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the declared executable path. The caller must first complete
    /// `ArchiveToolStore::verify_hashes` for this installation.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn metadata(&self) -> &InstallMetadata {
        &self.metadata
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustLevel {
    Pinned,
    Upstream,
    Unpinned,
}

impl TrustLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pinned => "pinned",
            Self::Upstream => "upstream",
            Self::Unpinned => "unpinned",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trust {
    pub(super) level: TrustLevel,
    pub(super) message: &'static str,
    pub(super) warning: Option<String>,
}

impl Trust {
    pub fn level(&self) -> TrustLevel {
        self.level
    }

    pub fn message(&self) -> &'static str {
        self.message
    }

    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }
}
