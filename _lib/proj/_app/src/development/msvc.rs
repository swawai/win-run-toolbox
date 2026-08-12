use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

mod install;
mod payload_cache;
mod source;
mod stage;

pub use install::{MsvcInstallContext, MsvcInstallOutcome, MsvcInstallResult, ensure_installed};
pub use payload_cache::{MsvcPayloadCache, VerifiedMsvcPayload};
pub use source::{MsvcPayload, MsvcRecipe, MsvcResolver, resolve_recipe};
pub use stage::MsvcStager;

use super::archive_tool::filesystem::{
    MAX_METADATA_BYTES, child_file, directory_chain, is_lower_hex, read_json, verify_regular_file,
    verify_regular_file_length,
};
use super::archive_tool::{ArchiveToolError, ArchiveToolErrorKind};

const INSTALL_SCHEMA: &str = "swawkit.proj-dev.msvc-install.v0";
const RECIPE_VERSION: &str = "1";
const CHANNEL_URL_PREFIX: &str = "https://aka.ms/vs/";
const CHANNEL_URL_SUFFIX: &str = "/release/channel";
const TOOL_PACKAGE_TEMPLATES: [&str; 7] = [
    "microsoft.vc.{tool}.crt.headers.base",
    "microsoft.vc.{tool}.crt.source.base",
    "microsoft.vc.{tool}.tools.hostx64.targetx64.base",
    "microsoft.vc.{tool}.tools.hostx64.targetx64.res.base",
    "microsoft.vc.{tool}.crt.x64.desktop.base",
    "microsoft.vc.{tool}.crt.x64.store.base",
    "microsoft.visualcpp.dia.sdk",
];
const SDK_MSI_NAMES: [&str; 8] = [
    "Windows SDK for Windows Store Apps Tools-x86_en-us.msi",
    "Windows SDK for Windows Store Apps Headers-x86_en-us.msi",
    "Windows SDK for Windows Store Apps Headers OnecoreUap-x86_en-us.msi",
    "Windows SDK for Windows Store Apps Libs-x86_en-us.msi",
    "Universal CRT Headers Libraries and Sources-x86_en-us.msi",
    "Windows SDK Desktop Headers x64-x86_en-us.msi",
    "Windows SDK OnecoreUap Headers x64-x86_en-us.msi",
    "Windows SDK Desktop Libs x64-x86_en-us.msi",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MsvcErrorKind {
    InvalidChannel,
    InvalidSource,
    DownloadFailed,
    MetadataUnreadable,
    MetadataStale,
    DuplicateFileRecords,
    MissingFileRecord,
    InvalidFileRecord,
    FileMismatch,
    MissingStorage,
    UnsafeStorage,
    Storage,
    InstallationFailed,
    RecoveryFailed,
    LockUnavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MsvcError {
    kind: MsvcErrorKind,
    message: String,
}

impl MsvcError {
    fn new(kind: MsvcErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    #[cfg(test)]
    fn kind(&self) -> MsvcErrorKind {
        self.kind
    }
}

impl fmt::Display for MsvcError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for MsvcError {}

impl From<ArchiveToolError> for MsvcError {
    fn from(source: ArchiveToolError) -> Self {
        let kind = match source.kind() {
            ArchiveToolErrorKind::DownloadFailed => MsvcErrorKind::DownloadFailed,
            ArchiveToolErrorKind::ArchiveInvalid | ArchiveToolErrorKind::InstallationFailed => {
                MsvcErrorKind::InstallationFailed
            }
            ArchiveToolErrorKind::RecoveryFailed => MsvcErrorKind::RecoveryFailed,
            ArchiveToolErrorKind::LockUnavailable => MsvcErrorKind::LockUnavailable,
            ArchiveToolErrorKind::InvalidDocument => MsvcErrorKind::MetadataUnreadable,
            ArchiveToolErrorKind::MissingStorage => MsvcErrorKind::MissingStorage,
            ArchiveToolErrorKind::UnsafeStorage => MsvcErrorKind::UnsafeStorage,
            ArchiveToolErrorKind::FileMismatch => MsvcErrorKind::FileMismatch,
            _ => MsvcErrorKind::Storage,
        };
        Self::new(kind, source.to_string())
    }
}

impl From<std::io::Error> for MsvcError {
    fn from(cause: std::io::Error) -> Self {
        error(MsvcErrorKind::Storage, cause.to_string())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MsvcDefinition {
    channel: String,
}

impl MsvcDefinition {
    pub fn new(channel: &str) -> Result<Self, MsvcError> {
        let channel = channel.trim();
        if channel.is_empty() || !channel.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(error(
                MsvcErrorKind::InvalidChannel,
                "SWAWKIT_PROJ_MSVC_CHANNEL must be a numeric VS channel.",
            ));
        }
        Ok(Self {
            channel: channel.to_owned(),
        })
    }

    pub fn channel(&self) -> &str {
        &self.channel
    }

    fn channel_url(&self) -> String {
        format!("{CHANNEL_URL_PREFIX}{}{CHANNEL_URL_SUFFIX}", self.channel)
    }

    fn definition_signature(&self) -> String {
        let identity = [
            "swawkit.proj-dev.msvc-definition.v0".to_owned(),
            "managed".to_owned(),
            self.channel.clone(),
            self.channel_url(),
            RECIPE_VERSION.to_owned(),
            "en-US".to_owned(),
            TOOL_PACKAGE_TEMPLATES.join("|"),
            SDK_MSI_NAMES.join("|"),
        ]
        .join("\n");
        format!("{:x}", Sha256::digest(identity.as_bytes()))
    }
}

pub struct MsvcStore<'a> {
    data_root: &'a Path,
    definition: &'a MsvcDefinition,
}

impl<'a> MsvcStore<'a> {
    pub fn new(data_root: &'a Path, definition: &'a MsvcDefinition) -> Self {
        Self {
            data_root,
            definition,
        }
    }

    pub fn read_installation(&self) -> Result<MsvcInstallation, MsvcError> {
        let root = self.install_root()?;
        self.read_installation_at(&root)
    }

    pub(crate) fn read_installation_at(&self, root: &Path) -> Result<MsvcInstallation, MsvcError> {
        let metadata: MsvcMetadata = read_json(
            &root.join(".swawkit-dev-msvc.json"),
            "MSVC installation metadata",
            MAX_METADATA_BYTES,
        )?;
        self.validate_metadata(&metadata)?;
        self.validate_records_and_lengths(&root, &metadata)?;
        for record in &metadata.files {
            let path = child_file(&root, &record.path, "MSVC installed file")?;
            verify_regular_file(&path, "MSVC installed file", record.length, &record.sha256)?;
        }
        Ok(MsvcInstallation {
            root: root.to_path_buf(),
            metadata,
        })
    }

    fn install_root(&self) -> Result<PathBuf, MsvcError> {
        Ok(directory_chain(
            self.data_root,
            &[
                "modules",
                "kernel",
                ".dev",
                "setup",
                "export",
                "msvc",
                "installs",
                self.definition.channel(),
            ],
            "MSVC installation",
        )?)
    }

    fn validate_metadata(&self, metadata: &MsvcMetadata) -> Result<(), MsvcError> {
        let valid = metadata.schema == INSTALL_SCHEMA
            && metadata.name == "msvc"
            && metadata.channel == self.definition.channel
            && metadata.channel_url == self.definition.channel_url()
            && metadata.recipe_version == RECIPE_VERSION
            && metadata.definition_signature == self.definition.definition_signature()
            && metadata
                .manifest_url
                .starts_with("https://download.visualstudio.microsoft.com/")
            && metadata.manifest_url.trim() == metadata.manifest_url
            && is_lower_hex(&metadata.manifest_sha256, 64)
            && is_numeric_dotted(&metadata.tool_package_version)
            && is_numeric_dotted(&metadata.tool_version)
            && !metadata.sdk_package.is_empty()
            && metadata.sdk_package.trim() == metadata.sdk_package
            && is_numeric_dotted(&metadata.sdk_version)
            && metadata.source_verification == "microsoft-manifest";
        if valid {
            Ok(())
        } else {
            Err(error(
                MsvcErrorKind::MetadataStale,
                "MSVC installation metadata is stale",
            ))
        }
    }

    fn validate_records_and_lengths(
        &self,
        root: &Path,
        metadata: &MsvcMetadata,
    ) -> Result<(), MsvcError> {
        let expected = required_paths(&metadata.tool_version, &metadata.sdk_version);
        if metadata.files.len() != expected.len() {
            return Err(error(
                MsvcErrorKind::MissingFileRecord,
                "MSVC installation inventory is incomplete",
            ));
        }
        let records = metadata
            .files
            .iter()
            .map(|record| (record.path.as_str(), record))
            .collect::<BTreeMap<_, _>>();
        if records.len() != metadata.files.len() {
            return Err(error(
                MsvcErrorKind::DuplicateFileRecords,
                "MSVC installation inventory has duplicate paths",
            ));
        }
        for relative in expected {
            let record = records.get(relative.as_str()).ok_or_else(|| {
                error(
                    MsvcErrorKind::MissingFileRecord,
                    "MSVC required file record is missing",
                )
            })?;
            if record.length == 0 || !is_lower_hex(&record.sha256, 64) {
                return Err(error(
                    MsvcErrorKind::InvalidFileRecord,
                    "MSVC required file record is invalid",
                ));
            }
            let path = child_file(root, relative.as_str(), "MSVC installed file")?;
            verify_regular_file_length(&path, "MSVC installed file", record.length)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MsvcInstallation {
    root: PathBuf,
    metadata: MsvcMetadata,
}

impl MsvcInstallation {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn tool_version(&self) -> &str {
        &self.metadata.tool_version
    }

    pub fn sdk_version(&self) -> &str {
        &self.metadata.sdk_version
    }

    pub fn add_environment(
        &self,
        plan: &mut crate::development::setup::environment::EnvironmentPlan,
    ) -> Result<(), String> {
        let vc_root = self.root.join("VC");
        let tool_root = vc_root.join(r"Tools\MSVC").join(self.tool_version());
        let sdk_root = self.root.join(r"Windows Kits\10");
        let tool_bin = tool_root.join(r"bin\Hostx64\x64");
        let sdk_bin = sdk_root.join("bin").join(self.sdk_version()).join("x64");
        for (name, value) in [
            ("VSCMD_ARG_HOST_ARCH", "x64".to_owned()),
            ("VSCMD_ARG_TGT_ARCH", "x64".to_owned()),
            ("VCToolsVersion", self.tool_version().to_owned()),
            ("WindowsSDKVersion", format!("{}\\", self.sdk_version())),
            ("VCToolsInstallDir", trailing_slash(&tool_root)),
            ("VCINSTALLDIR", trailing_slash(&vc_root)),
            ("WindowsSdkDir", trailing_slash(&sdk_root)),
            ("WindowsSdkBinPath", trailing_slash(&sdk_root.join("bin"))),
            ("WindowsSdkVerBinPath", trailing_slash(&sdk_bin)),
            ("UniversalCRTSdkDir", trailing_slash(&sdk_root)),
            ("UCRTVersion", self.sdk_version().to_owned()),
            (
                "INCLUDE",
                join_paths([
                    tool_root.join("include"),
                    sdk_root
                        .join("Include")
                        .join(self.sdk_version())
                        .join("ucrt"),
                    sdk_root
                        .join("Include")
                        .join(self.sdk_version())
                        .join("shared"),
                    sdk_root.join("Include").join(self.sdk_version()).join("um"),
                    sdk_root
                        .join("Include")
                        .join(self.sdk_version())
                        .join("winrt"),
                    sdk_root
                        .join("Include")
                        .join(self.sdk_version())
                        .join("cppwinrt"),
                ]),
            ),
            (
                "LIB",
                join_paths([
                    tool_root.join(r"lib\x64"),
                    sdk_root
                        .join("Lib")
                        .join(self.sdk_version())
                        .join(r"ucrt\x64"),
                    sdk_root
                        .join("Lib")
                        .join(self.sdk_version())
                        .join(r"um\x64"),
                ]),
            ),
        ] {
            plan.set(name, Some(value))?;
        }
        plan.prepend_path(tool_bin)?;
        plan.prepend_path(&sdk_bin)?;
        plan.prepend_path(sdk_bin.join("ucrt"))?;
        Ok(())
    }
}

fn trailing_slash(path: &Path) -> String {
    format!("{}\\", path.to_string_lossy().trim_end_matches(['\\', '/']))
}

fn join_paths<const N: usize>(paths: [PathBuf; N]) -> String {
    paths
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join(";")
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MsvcMetadata {
    schema: String,
    name: String,
    channel: String,
    channel_url: String,
    recipe_version: String,
    definition_signature: String,
    manifest_url: String,
    manifest_sha256: String,
    tool_package_version: String,
    tool_version: String,
    sdk_package: String,
    sdk_version: String,
    source_verification: String,
    files: Vec<InstalledFile>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct InstalledFile {
    path: String,
    length: u64,
    sha256: String,
}

fn required_paths(tool_version: &str, sdk_version: &str) -> Vec<String> {
    [
        "setup_x64.bat".to_owned(),
        format!("VC\\Tools\\MSVC\\{tool_version}\\bin\\Hostx64\\x64\\cl.exe"),
        format!("VC\\Tools\\MSVC\\{tool_version}\\bin\\Hostx64\\x64\\link.exe"),
        format!("VC\\Tools\\MSVC\\{tool_version}\\bin\\Hostx64\\x64\\lib.exe"),
        format!("VC\\Tools\\MSVC\\{tool_version}\\bin\\Hostx64\\x64\\msdia140.dll"),
        format!("VC\\Tools\\MSVC\\{tool_version}\\include\\yvals_core.h"),
        format!("Windows Kits\\10\\bin\\{sdk_version}\\x64\\rc.exe"),
        format!("Windows Kits\\10\\Include\\{sdk_version}\\ucrt\\stdio.h"),
        format!("Windows Kits\\10\\Include\\{sdk_version}\\um\\windows.h"),
        format!("Windows Kits\\10\\Lib\\{sdk_version}\\ucrt\\x64\\ucrt.lib"),
        format!("Windows Kits\\10\\Lib\\{sdk_version}\\um\\x64\\kernel32.lib"),
    ]
    .into_iter()
    .collect()
}

fn is_numeric_dotted(value: &str) -> bool {
    let fields = value.split('.').collect::<Vec<_>>();
    fields.len() >= 2
        && fields
            .iter()
            .all(|field| !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()))
}

fn error(kind: MsvcErrorKind, message: impl Into<String>) -> MsvcError {
    MsvcError::new(kind, message)
}

#[cfg(test)]
mod tests;
