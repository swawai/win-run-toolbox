use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

use super::CommandContext;
use super::filesystem::{
    MAX_METADATA_BYTES, child_file, directory_chain, is_lower_hex, read_json, regular_file_length,
    sha256_regular, sha256_text,
};

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

pub(super) enum MsvcReport {
    Off,
    Managed {
        channel: String,
        metadata: Option<MsvcMetadata>,
        ready: bool,
    },
}

impl MsvcReport {
    pub(super) fn render(&self) {
        match self {
            Self::Off => println!("[OFF] msvc is disabled."),
            Self::Managed {
                channel,
                metadata,
                ready,
            } => {
                let state = if *ready { "READY" } else { "MISSING" };
                let version = if *ready {
                    let metadata = metadata.as_ref().expect("ready MSVC metadata");
                    format!(
                        "tool {}, SDK {}",
                        metadata.tool_version, metadata.sdk_version
                    )
                } else {
                    "not installed".to_owned()
                };
                println!("[{state}] msvc channel {channel}  microsoft-manifest  {version}");
            }
        }
    }
}

pub(super) fn inspect(context: &CommandContext) -> Result<MsvcReport, String> {
    let mode = context
        .environment("SWAWKIT_PROJ_MSVC_MODE")
        .to_ascii_lowercase();
    if mode.is_empty() || mode == "disabled" {
        return Ok(MsvcReport::Off);
    }
    if mode != "managed" {
        return Err(format!(
            "Unsupported SWAWKIT_PROJ_MSVC_MODE value '{mode}'. Expected 'managed' or 'disabled'."
        ));
    }
    let channel = context.environment("SWAWKIT_PROJ_MSVC_CHANNEL");
    if channel.is_empty() || !channel.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("SWAWKIT_PROJ_MSVC_CHANNEL must be a numeric VS channel.".to_owned());
    }
    let metadata = validate_metadata(context, &channel).ok();
    let ready = metadata
        .as_ref()
        .is_some_and(|metadata| hashes_match(context, &channel, metadata));
    Ok(MsvcReport::Managed {
        channel,
        metadata,
        ready,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MsvcMetadata {
    schema: String,
    name: String,
    channel: String,
    recipe_version: String,
    definition_signature: String,
    manifest_sha256: String,
    pub(super) tool_version: String,
    pub(super) sdk_version: String,
    source_verification: String,
    files: Vec<InstalledFile>,
}

#[derive(Deserialize)]
struct InstalledFile {
    path: String,
    length: u64,
    sha256: String,
}

fn validate_metadata(context: &CommandContext, channel: &str) -> Result<MsvcMetadata, String> {
    let root = install_root(context, channel)?;
    let metadata: MsvcMetadata = read_json(
        &root.join(".swawkit-dev-msvc.json"),
        "MSVC installation metadata",
        MAX_METADATA_BYTES,
    )?;
    if metadata.schema != "swawkit.proj-dev.msvc-install.v0"
        || metadata.name != "msvc"
        || metadata.channel != channel
        || metadata.recipe_version != "1"
        || metadata.definition_signature != definition_signature(channel)
        || !is_lower_hex(&metadata.manifest_sha256, 64)
        || !is_numeric_dotted(&metadata.tool_version)
        || !is_numeric_dotted(&metadata.sdk_version)
        || metadata.source_verification != "microsoft-manifest"
    {
        return Err("MSVC installation metadata is stale".to_owned());
    }
    let expected = required_paths(&metadata.tool_version, &metadata.sdk_version);
    if metadata.files.len() != expected.len() {
        return Err("MSVC installation inventory is incomplete".to_owned());
    }
    let records: BTreeMap<_, _> = metadata
        .files
        .iter()
        .map(|record| (record.path.as_str(), record))
        .collect();
    if records.len() != metadata.files.len() {
        return Err("MSVC installation inventory has duplicate paths".to_owned());
    }
    for relative in &expected {
        let record = records
            .get(relative.as_str())
            .ok_or("MSVC required file record is missing")?;
        if !is_lower_hex(&record.sha256, 64) {
            return Err("MSVC required file hash is invalid".to_owned());
        }
        let path = child_file(&root, relative, "MSVC installed file")?;
        if regular_file_length(&path, "MSVC installed file")? != record.length {
            return Err("MSVC required file length changed".to_owned());
        }
    }
    Ok(metadata)
}

fn hashes_match(context: &CommandContext, channel: &str, metadata: &MsvcMetadata) -> bool {
    let Ok(root) = install_root(context, channel) else {
        return false;
    };
    metadata.files.iter().all(|record| {
        child_file(&root, &record.path, "MSVC installed file")
            .and_then(|path| sha256_regular(&path, "MSVC installed file"))
            .is_ok_and(|actual| actual == record.sha256)
    })
}

fn install_root(context: &CommandContext, channel: &str) -> Result<PathBuf, String> {
    directory_chain(
        &context.data_root,
        &[
            "modules", "kernel", ".dev", "setup", "export", "msvc", "installs", channel,
        ],
        "MSVC installation",
    )
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

fn definition_signature(channel: &str) -> String {
    let identity = [
        "swawkit.proj-dev.msvc-definition.v0".to_owned(),
        "managed".to_owned(),
        channel.to_owned(),
        format!("https://aka.ms/vs/{channel}/release/channel"),
        "1".to_owned(),
        "en-US".to_owned(),
        TOOL_PACKAGE_TEMPLATES.join("|"),
        SDK_MSI_NAMES.join("|"),
    ]
    .join("\n");
    sha256_text(&identity)
}

fn is_numeric_dotted(value: &str) -> bool {
    let fields: Vec<_> = value.split('.').collect();
    fields.len() >= 2
        && fields
            .iter()
            .all(|field| !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()))
}
