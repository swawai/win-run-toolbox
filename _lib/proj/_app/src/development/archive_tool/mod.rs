use std::collections::BTreeMap;
use std::path::Path;

use super::ArchiveToolContract;

pub(crate) mod filesystem;
pub mod github;
pub mod install;
mod selection;
mod trust;
mod types;

pub use types::{
    ArchiveToolError, ArchiveToolErrorKind, ArchiveToolRequest, InstallMetadata, Installation,
    InstalledFile, ResolvedDefinition, ResolvedVerification, Selection, SourceVerification, Trust,
    TrustLevel,
};

use filesystem::{
    MAX_METADATA_BYTES, child_file, directory_chain, is_lower_hex, read_json, regular_directory,
    verify_regular_file, verify_regular_file_length,
};

const INSTALL_SCHEMA: &str = "swawkit.proj-dev.install.v0";

pub struct ArchiveToolStore<'a> {
    data_root: &'a Path,
    tool: &'a ArchiveToolContract,
}

impl ArchiveToolRequest {
    pub fn new(
        tool: &ArchiveToolContract,
        requested: &str,
        project_sha256: &str,
    ) -> Result<Self, ArchiveToolError> {
        if requested != "latest" && !tool.accepts_exact_version(requested) {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::InvalidVersion,
                format!("Invalid {} version '{requested}'.", tool.display_name),
            ));
        }
        let mut project_sha256 = project_sha256.to_ascii_lowercase();
        if let Some(value) = project_sha256.strip_prefix("sha256:") {
            project_sha256 = value.to_owned();
        }
        if !project_sha256.is_empty() && !is_lower_hex(&project_sha256, 64) {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::InvalidProjectSha256,
                format!(
                    "{} must be empty or a 64-character SHA-256 value.",
                    tool.hash_variable
                ),
            ));
        }
        if requested == "latest" && !project_sha256.is_empty() {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::LatestWithProjectSha256,
                format!(
                    "{}=latest cannot be combined with {}.",
                    tool.version_variable, tool.hash_variable
                ),
            ));
        }
        Ok(Self {
            tool_name: tool.name.to_owned(),
            requested: requested.to_owned(),
            project_sha256,
        })
    }
}

impl<'a> ArchiveToolStore<'a> {
    pub fn new(data_root: &'a Path, tool: &'a ArchiveToolContract) -> Self {
        Self { data_root, tool }
    }

    pub fn require_export(&self) -> Result<(), ArchiveToolError> {
        let components = [
            "modules",
            "kernel",
            ".dev",
            "setup",
            "export",
            self.tool.name,
        ];
        directory_chain(self.data_root, &components, "tool export")
            .map(|_| ())
            .map_err(|error| error.with_kind(ArchiveToolErrorKind::ExportUnavailable))
    }

    pub fn resolve(
        &self,
        request: &ArchiveToolRequest,
    ) -> Result<Option<ResolvedDefinition>, ArchiveToolError> {
        self.require_tool(&request.tool_name)?;
        if request.requested != "latest" {
            let pinned = !request.project_sha256.is_empty();
            return Ok(Some(ResolvedDefinition {
                tool_name: self.tool.name.to_owned(),
                requested_latest: false,
                version: request.requested.clone(),
                source_sha256: pinned.then(|| request.project_sha256.clone()),
                verification: if pinned {
                    ResolvedVerification::Published(SourceVerification::Project)
                } else {
                    ResolvedVerification::Unresolved
                },
                project_sha256: request.project_sha256.clone(),
            }));
        }
        let Some(selection) = self.read_selection()? else {
            return Ok(None);
        };
        Ok(Some(ResolvedDefinition {
            tool_name: self.tool.name.to_owned(),
            requested_latest: true,
            version: selection.version,
            source_sha256: Some(selection.source_sha256),
            verification: ResolvedVerification::Published(selection.source_verification),
            project_sha256: request.project_sha256.clone(),
        }))
    }

    /// Reads the published metadata and verifies required membership and file lengths.
    /// Call `verify_hashes` before executing any file from the returned installation.
    pub fn read_installation(
        &self,
        resolved: &ResolvedDefinition,
    ) -> Result<Installation, ArchiveToolError> {
        self.require_tool(&resolved.tool_name)?;
        let components = [
            "modules",
            "kernel",
            ".dev",
            "setup",
            "export",
            self.tool.name,
            "installs",
            resolved.version.as_str(),
        ];
        let root = directory_chain(self.data_root, &components, "tool installation")
            .map_err(|error| content_error(error, ArchiveToolErrorKind::InstallationUnavailable))?;
        self.read_installation_at(resolved, &root)
    }

    pub(super) fn read_installation_at(
        &self,
        resolved: &ResolvedDefinition,
        root: &Path,
    ) -> Result<Installation, ArchiveToolError> {
        self.require_tool(&resolved.tool_name)?;
        regular_directory(root, "tool installation")?;
        let metadata_path = root.join(".swawkit-dev-install.json");
        let metadata: InstallMetadata = read_json(
            &metadata_path,
            "tool installation metadata",
            MAX_METADATA_BYTES,
        )
        .map_err(|error| content_error(error, ArchiveToolErrorKind::MetadataUnreadable))?;
        self.validate_metadata(resolved, &metadata)?;
        self.validate_records_and_lengths(root, &metadata)?;
        Ok(Installation {
            tool_name: self.tool.name.to_owned(),
            executable: root.join(self.tool.executable),
            root: root.to_path_buf(),
            metadata,
        })
    }

    /// Completes installation validation by hashing every required file.
    pub fn verify_hashes(&self, installation: &Installation) -> Result<(), ArchiveToolError> {
        self.require_tool(&installation.tool_name)?;
        for record in &installation.metadata.files {
            let path = child_file(&installation.root, &record.path, "installed file").map_err(
                |error| content_error(error, ArchiveToolErrorKind::InstalledFileInvalid),
            )?;
            verify_regular_file(&path, "installed file", record.length, &record.sha256).map_err(
                |error| content_error(error, ArchiveToolErrorKind::InstalledFileInvalid),
            )?;
        }
        Ok(())
    }

    pub fn trust(
        &self,
        resolved: &ResolvedDefinition,
        installation: Option<&Installation>,
    ) -> Result<Trust, ArchiveToolError> {
        self.require_tool(&resolved.tool_name)?;
        if let Some(installation) = installation {
            self.require_tool(&installation.tool_name)?;
            let belongs_to_resolution = installation.metadata.version == resolved.version
                && installation.metadata.definition_signature
                    == self
                        .tool
                        .definition_signature(&resolved.version, &resolved.project_sha256);
            if !belongs_to_resolution {
                return Err(ArchiveToolError::new(
                    ArchiveToolErrorKind::DefinitionMismatch,
                    "archive tool installation belongs to a different resolution",
                ));
            }
        }
        Ok(trust::evaluate(
            self.tool,
            resolved,
            installation.map(Installation::metadata),
        ))
    }

    fn validate_metadata(
        &self,
        resolved: &ResolvedDefinition,
        metadata: &InstallMetadata,
    ) -> Result<(), ArchiveToolError> {
        let verification_matches = match resolved.verification {
            ResolvedVerification::Published(expected) => metadata.source_verification == expected,
            ResolvedVerification::Unresolved => matches!(
                metadata.source_verification,
                SourceVerification::Github | SourceVerification::Unverified
            ),
        };
        let valid = metadata.schema == INSTALL_SCHEMA
            && metadata.name == self.tool.name
            && metadata.version == resolved.version
            && !metadata.source_url.is_empty()
            && metadata.source_url.trim() == metadata.source_url
            && is_lower_hex(&metadata.source_sha256, 64)
            && verification_matches
            && resolved
                .source_sha256
                .as_ref()
                .is_none_or(|expected| expected == &metadata.source_sha256)
            && metadata.recipe_version == self.tool.recipe_version
            && metadata.definition_signature
                == self
                    .tool
                    .definition_signature(&resolved.version, &resolved.project_sha256)
            && metadata.files.len() == self.tool.required_paths.len();
        if !valid {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::MetadataStale,
                "tool installation metadata is stale",
            ));
        }
        Ok(())
    }

    fn validate_records_and_lengths(
        &self,
        root: &Path,
        metadata: &InstallMetadata,
    ) -> Result<(), ArchiveToolError> {
        let records: BTreeMap<_, _> = metadata
            .files
            .iter()
            .map(|record| (record.path.as_str(), record))
            .collect();
        if records.len() != metadata.files.len() {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::DuplicateFileRecords,
                "tool installation metadata has duplicate file records",
            ));
        }
        for relative in self.tool.required_paths {
            let record = records.get(relative).ok_or_else(|| {
                ArchiveToolError::new(
                    ArchiveToolErrorKind::MissingFileRecord,
                    "tool installation metadata is missing a required file record",
                )
            })?;
            if record.length == 0 || !is_lower_hex(&record.sha256, 64) {
                return Err(ArchiveToolError::new(
                    ArchiveToolErrorKind::InvalidFileRecord,
                    "tool installation metadata has an invalid file record",
                ));
            }
            let path = child_file(root, relative, "installed file").map_err(|error| {
                content_error(error, ArchiveToolErrorKind::InstalledFileInvalid)
            })?;
            verify_regular_file_length(&path, "installed file", record.length).map_err(
                |error| content_error(error, ArchiveToolErrorKind::InstalledFileInvalid),
            )?;
        }
        Ok(())
    }

    fn require_tool(&self, actual: &str) -> Result<(), ArchiveToolError> {
        if actual == self.tool.name {
            return Ok(());
        }
        Err(ArchiveToolError::new(
            ArchiveToolErrorKind::DefinitionMismatch,
            format!(
                "archive tool definition belongs to '{actual}', not '{}'",
                self.tool.name
            ),
        ))
    }
}

fn content_error(error: ArchiveToolError, kind: ArchiveToolErrorKind) -> ArchiveToolError {
    match error.kind() {
        ArchiveToolErrorKind::UnsafeStorage | ArchiveToolErrorKind::Storage => error,
        _ => error.with_kind(kind),
    }
}

#[cfg(test)]
mod tests;
