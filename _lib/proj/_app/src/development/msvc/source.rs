use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use super::{
    MsvcDefinition, MsvcError, MsvcErrorKind, SDK_MSI_NAMES, TOOL_PACKAGE_TEMPLATES, error,
};

mod model;
pub(super) mod resolver;
pub use model::{MsvcPayload, MsvcRecipe};
pub use resolver::MsvcResolver;

const MANIFEST_ID: &str = "Microsoft.VisualStudio.Manifests.VisualStudio";
const RESOURCE_LANGUAGE: &str = "en-US";
const MAX_CHANNEL_BYTES: usize = 4 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 64 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 12 * 1024 * 1024 * 1024;

pub fn resolve_recipe(
    definition: &MsvcDefinition,
    channel_document: &[u8],
    manifest_document: &[u8],
) -> Result<MsvcRecipe, MsvcError> {
    if channel_document.len() > MAX_CHANNEL_BYTES || manifest_document.len() > MAX_MANIFEST_BYTES {
        return Err(invalid(
            "Microsoft source document exceeds its safety limit",
        ));
    }
    let manifest_payload = manifest_payload(channel_document)?;
    let manifest: ManifestDocument = parse(manifest_document, "Visual Studio manifest")?;
    resolve_manifest(
        definition,
        manifest_payload,
        format!("{:x}", Sha256::digest(manifest_document)),
        manifest,
    )
}

fn manifest_payload(channel_document: &[u8]) -> Result<MsvcPayload, MsvcError> {
    if channel_document.len() > MAX_CHANNEL_BYTES {
        return Err(invalid(
            "Visual Studio channel exceeds its 4 MiB safety limit",
        ));
    }
    let channel: ChannelDocument = parse(channel_document, "Visual Studio channel")?;
    let items = channel
        .channel_items
        .iter()
        .filter(|item| item.id == MANIFEST_ID)
        .collect::<Vec<_>>();
    if items.len() != 1 || items[0].payloads.len() != 1 {
        return Err(invalid(
            "the Visual Studio channel must declare exactly one product manifest",
        ));
    }
    convert_payload(&items[0].payloads[0], "Visual Studio manifest")
}

fn resolve_manifest(
    definition: &MsvcDefinition,
    manifest_payload: MsvcPayload,
    manifest_sha256: String,
    manifest: ManifestDocument,
) -> Result<MsvcRecipe, MsvcError> {
    if manifest.packages.is_empty() {
        return Err(invalid("the Visual Studio manifest contains no packages"));
    }
    let tool_package_version = select_tool_version(&manifest.packages)?;
    let mut tool_payloads = Vec::new();
    for template in TOOL_PACKAGE_TEMPLATES {
        let id = template.replace("{tool}", &tool_package_version);
        let language = id
            .to_ascii_lowercase()
            .ends_with(".res.base")
            .then_some(RESOURCE_LANGUAGE);
        let package = unique_package(&manifest.packages, &id, language)?;
        tool_payloads.extend(package_payloads(package, &id)?);
    }

    let sdk_component = select_sdk_component(&manifest.packages)?;
    let dependencies = sdk_component.dependencies.as_ref().ok_or_else(|| {
        invalid(format!(
            "SDK component '{}' has no dependencies",
            sdk_component.id
        ))
    })?;
    let sdk_dependencies = dependencies
        .keys()
        .filter(|name| sdk_dependency(name))
        .collect::<Vec<_>>();
    if sdk_dependencies.len() != 1 {
        return Err(invalid(format!(
            "SDK component '{}' must identify exactly one Windows SDK package",
            sdk_component.id
        )));
    }
    let sdk_package = (*sdk_dependencies[0]).clone();
    let sdk = unique_package(&manifest.packages, &sdk_package, None)?;
    let sdk_payloads = package_payloads(sdk, &sdk_package)?;
    let mut msi_payloads = Vec::new();
    for name in SDK_MSI_NAMES {
        let expected = format!("Installers\\{name}");
        let matches = sdk_payloads
            .iter()
            .filter(|payload| payload.file_name().eq_ignore_ascii_case(&expected))
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(invalid(format!(
                "Windows SDK package must contain exactly one '{expected}' payload"
            )));
        }
        msi_payloads.push(matches[0].clone());
    }
    Ok(MsvcRecipe {
        channel: definition.channel().to_owned(),
        definition_signature: definition.definition_signature(),
        manifest: manifest_payload,
        manifest_sha256,
        tool_package_version,
        tool_payloads,
        sdk_package,
        sdk_payloads,
        msi_payloads,
    })
}

fn select_tool_version(packages: &[Package]) -> Result<String, MsvcError> {
    packages
        .iter()
        .filter_map(|package| tool_version(&package.id))
        .max_by(|left, right| compare_version(left, right))
        .map(|version| version.0)
        .ok_or_else(|| invalid("no x64 MSVC tool package was found in the Visual Studio manifest"))
}

fn select_sdk_component(packages: &[Package]) -> Result<&Package, MsvcError> {
    let candidates = packages
        .iter()
        .filter_map(|package| sdk_component_number(&package.id).map(|number| (number, package)))
        .collect::<Vec<_>>();
    let maximum = candidates
        .iter()
        .map(|(number, _)| *number)
        .max()
        .ok_or_else(|| invalid("no Windows 10/11 SDK component was found in the manifest"))?;
    let selected = candidates
        .into_iter()
        .filter(|(number, _)| *number == maximum)
        .collect::<Vec<_>>();
    if selected.len() != 1 {
        return Err(invalid("the newest Windows SDK component is ambiguous"));
    }
    Ok(selected[0].1)
}

fn unique_package<'a>(
    packages: &'a [Package],
    id: &str,
    language: Option<&str>,
) -> Result<&'a Package, MsvcError> {
    let matches = packages
        .iter()
        .filter(|package| {
            package.id.eq_ignore_ascii_case(id)
                && match language {
                    Some(expected) => package
                        .language
                        .as_deref()
                        .is_some_and(|actual| actual.eq_ignore_ascii_case(expected)),
                    None => package
                        .language
                        .as_deref()
                        .is_none_or(|actual| actual.trim().is_empty()),
                }
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(invalid(format!(
            "expected exactly one Microsoft package '{id}'"
        )));
    }
    Ok(matches[0])
}

fn package_payloads(package: &Package, description: &str) -> Result<Vec<MsvcPayload>, MsvcError> {
    if package.payloads.is_empty() {
        return Err(invalid(format!(
            "Microsoft package has no payloads: {description}"
        )));
    }
    package
        .payloads
        .iter()
        .map(|payload| convert_payload(payload, description))
        .collect()
}

fn convert_payload(raw: &RawPayload, description: &str) -> Result<MsvcPayload, MsvcError> {
    let file_name = raw.file_name.as_str();
    let sha256 = raw.sha256.trim().to_ascii_lowercase();
    let url = Url::parse(raw.url.trim())
        .ok()
        .filter(|url| {
            url.scheme() == "https"
                && url.host_str() == Some("download.visualstudio.microsoft.com")
                && url.username().is_empty()
                && url.password().is_none()
        })
        .ok_or_else(|| invalid(format!("invalid Microsoft payload for {description}")))?;
    let leaf_name = Path::new(file_name)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| valid_leaf_name(name))
        .ok_or_else(|| invalid(format!("invalid Microsoft payload for {description}")))?;
    if file_name.is_empty()
        || file_name.trim() != file_name
        || raw.url.trim() != raw.url
        || sha256.len() != 64
        || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || raw
            .size
            .is_some_and(|size| size == 0 || size > MAX_PAYLOAD_BYTES)
    {
        return Err(invalid(format!(
            "invalid Microsoft payload for {description}"
        )));
    }
    Ok(MsvcPayload::new(
        file_name.to_owned(),
        leaf_name.to_owned(),
        sha256,
        raw.size,
        url.to_string(),
    ))
}

fn valid_leaf_name(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && !value.ends_with(['.', ' '])
        && !value.bytes().any(|byte| {
            byte < 32
                || matches!(
                    byte,
                    b'<' | b'>' | b':' | b'"' | b'/' | b'\\' | b'|' | b'?' | b'*'
                )
        })
        && !is_windows_device_name(value)
}

fn is_windows_device_name(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn tool_version(id: &str) -> Option<(String, [u64; 4])> {
    let lower = id.to_ascii_lowercase();
    let version = lower
        .strip_prefix("microsoft.vc.")?
        .strip_suffix(".tools.hostx64.targetx64.base")?;
    parse_quad(version).map(|parsed| (version.to_owned(), parsed))
}

fn compare_version(left: &(String, [u64; 4]), right: &(String, [u64; 4])) -> Ordering {
    left.1.cmp(&right.1)
}

fn parse_quad(value: &str) -> Option<[u64; 4]> {
    let fields = value
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    fields.try_into().ok()
}

fn sdk_component_number(id: &str) -> Option<u64> {
    let lower = id.to_ascii_lowercase();
    [
        "microsoft.visualstudio.component.windows10sdk.",
        "microsoft.visualstudio.component.windows11sdk.",
    ]
    .iter()
    .find_map(|prefix| lower.strip_prefix(prefix)?.parse().ok())
}

fn sdk_dependency(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("win10sdk_") || lower.starts_with("win11sdk_")
}

fn parse<T: for<'de> Deserialize<'de>>(document: &[u8], name: &str) -> Result<T, MsvcError> {
    serde_json::from_slice(document)
        .map_err(|cause| invalid(format!("cannot parse {name} JSON: {cause}")))
}

fn invalid(message: impl Into<String>) -> MsvcError {
    error(MsvcErrorKind::InvalidSource, message)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelDocument {
    channel_items: Vec<ChannelItem>,
}

#[derive(Deserialize)]
struct ChannelItem {
    id: String,
    #[serde(default)]
    payloads: Vec<RawPayload>,
}

#[derive(Deserialize)]
struct ManifestDocument {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    language: Option<String>,
    #[serde(default)]
    payloads: Vec<RawPayload>,
    dependencies: Option<BTreeMap<String, serde_json::Value>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPayload {
    file_name: String,
    sha256: String,
    size: Option<u64>,
    url: String,
}

#[cfg(test)]
pub(crate) mod tests;
