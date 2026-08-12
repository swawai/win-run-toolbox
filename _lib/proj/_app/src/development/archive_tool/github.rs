use std::io::Read;
use std::time::Duration;

use serde::Deserialize;
use ureq::{
    Agent,
    tls::{RootCerts, TlsConfig},
};

use crate::development::{ArchiveToolContract, GithubReleaseCoordinates, is_semantic_version};

use super::install::ArchiveSource;
use super::{
    ArchiveToolError, ArchiveToolErrorKind, ArchiveToolRequest, ResolvedDefinition,
    ResolvedVerification, SourceVerification,
};

const GITHUB_API_ROOT: &str = "https://api.github.com";
const MAX_RELEASE_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;
const USER_AGENT: &str = "swawkit-proj-v0";

#[derive(Debug)]
pub struct ResolvedRelease {
    definition: ResolvedDefinition,
    source: ArchiveSource,
}

impl ResolvedRelease {
    pub fn definition(&self) -> &ResolvedDefinition {
        &self.definition
    }

    pub fn source(&self) -> &ArchiveSource {
        &self.source
    }

    pub fn into_parts(self) -> (ResolvedDefinition, ArchiveSource) {
        (self.definition, self.source)
    }
}

/// Resolves an exact declaration against its matching GitHub Release.
///
/// Already-published definitions should use [`published_source`] so normal
/// startup and recovery never acquire a network dependency.
pub fn resolve_exact(
    tool: &'static ArchiveToolContract,
    resolved: &ResolvedDefinition,
) -> Result<ResolvedRelease, ArchiveToolError> {
    require_definition(tool, resolved)?;
    if resolved.requested_latest() {
        return Err(invalid_release(
            "an exact GitHub resolution cannot consume a latest definition",
        ));
    }
    let coordinates = tool.release_coordinates(resolved.version());
    let endpoint = release_endpoint(tool, &coordinates.tag);
    let document = request_release(tool, &endpoint)?;
    resolve_document(tool, resolved, &coordinates, &document)
}

/// Resolves `latest` to an immutable exact definition and archive source.
pub fn resolve_latest(
    tool: &'static ArchiveToolContract,
    request: &ArchiveToolRequest,
) -> Result<ResolvedRelease, ArchiveToolError> {
    if request.tool_name != tool.name || request.requested() != "latest" {
        return Err(invalid_release(
            "GitHub latest resolution requires a matching latest request",
        ));
    }
    let endpoint = latest_endpoint(tool);
    let document = request_release(tool, &endpoint)?;
    resolve_latest_document(tool, request, &document)
}

/// Reconstructs the only valid direct URL for a published definition.
/// No GitHub API request is performed.
pub fn published_source(
    tool: &'static ArchiveToolContract,
    resolved: &ResolvedDefinition,
) -> Result<ArchiveSource, ArchiveToolError> {
    require_definition(tool, resolved)?;
    if !resolved.requested_latest() {
        return Err(invalid_release(
            "only a published latest selection can bypass GitHub resolution",
        ));
    }
    let ResolvedVerification::Published(verification) = resolved.verification() else {
        return Err(invalid_release(
            "an unresolved definition has no published archive source",
        ));
    };
    if !matches!(
        verification,
        SourceVerification::Github | SourceVerification::Unverified
    ) {
        return Err(invalid_release(
            "a latest selection must be verified by GitHub or recorded as unverified",
        ));
    }
    let digest = resolved.source_sha256().ok_or_else(|| {
        invalid_release("a published archive source must declare its exact SHA-256")
    })?;
    let coordinates = tool.release_coordinates(resolved.version());
    ArchiveSource::from_release(
        resolved,
        coordinates.download_url,
        Some(digest),
        verification,
    )
}

fn resolve_latest_document(
    tool: &'static ArchiveToolContract,
    request: &ArchiveToolRequest,
    document: &[u8],
) -> Result<ResolvedRelease, ArchiveToolError> {
    let release = parse_document(document)?;
    let version = tool
        .release_version_from_tag(&release.tag_name)
        .filter(|value| is_semantic_version(value))
        .ok_or_else(|| {
            invalid_release(format!(
                "GitHub returned an invalid latest {} tag '{}'",
                tool.display_name, release.tag_name
            ))
        })?;
    let coordinates = tool.release_coordinates(version);
    let unresolved = ResolvedDefinition {
        tool_name: tool.name.to_owned(),
        requested_latest: true,
        version: version.to_owned(),
        source_sha256: None,
        verification: ResolvedVerification::Unresolved,
        project_sha256: request.project_sha256().to_owned(),
    };
    resolve_release(tool, unresolved, &coordinates, release)
}

fn resolve_document(
    tool: &'static ArchiveToolContract,
    resolved: &ResolvedDefinition,
    coordinates: &GithubReleaseCoordinates,
    document: &[u8],
) -> Result<ResolvedRelease, ArchiveToolError> {
    let release = parse_document(document)?;
    resolve_release(tool, resolved.clone(), coordinates, release)
}

fn resolve_release(
    tool: &'static ArchiveToolContract,
    mut definition: ResolvedDefinition,
    coordinates: &GithubReleaseCoordinates,
    release: ReleaseDocument,
) -> Result<ResolvedRelease, ArchiveToolError> {
    if release.tag_name != coordinates.tag {
        return Err(invalid_release(format!(
            "GitHub returned {} release tag '{}'; expected '{}'",
            tool.display_name, release.tag_name, coordinates.tag
        )));
    }
    let matching: Vec<_> = release
        .assets
        .into_iter()
        .filter(|asset| asset.name == coordinates.asset)
        .collect();
    if matching.len() != 1 {
        return Err(invalid_release(format!(
            "GitHub release '{}' must contain exactly one '{}' asset; found {}",
            coordinates.tag,
            coordinates.asset,
            matching.len()
        )));
    }
    let asset = &matching[0];
    if asset.browser_download_url != coordinates.download_url {
        return Err(invalid_release(format!(
            "GitHub returned a non-canonical {} asset URL",
            tool.display_name
        )));
    }
    let github_sha256 = asset
        .digest
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(parse_digest)
        .transpose()?;
    let project_sha256 =
        (!definition.project_sha256().is_empty()).then(|| definition.project_sha256().to_owned());
    if let (Some(project), Some(github)) = (&project_sha256, &github_sha256)
        && project != github
    {
        return Err(invalid_release(format!(
            "{} does not match the GitHub Release digest for {} {}",
            tool.hash_variable,
            tool.display_name,
            definition.version()
        )));
    }
    let (digest, verification) = if let Some(project) = project_sha256 {
        (Some(project), SourceVerification::Project)
    } else if let Some(github) = github_sha256 {
        (Some(github), SourceVerification::Github)
    } else {
        (None, SourceVerification::Unverified)
    };
    definition.source_sha256 = digest.clone();
    definition.verification = digest
        .as_ref()
        .map_or(ResolvedVerification::Unresolved, |_| {
            ResolvedVerification::Published(verification)
        });
    let source = ArchiveSource::from_release(
        &definition,
        coordinates.download_url.clone(),
        digest.as_deref(),
        verification,
    )?;
    Ok(ResolvedRelease { definition, source })
}

#[derive(Deserialize)]
struct ReleaseDocument {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

fn parse_document(document: &[u8]) -> Result<ReleaseDocument, ArchiveToolError> {
    serde_json::from_slice(document).map_err(|cause| {
        invalid_release(format!(
            "GitHub returned an invalid release document: {cause}"
        ))
    })
}

fn parse_digest(value: &str) -> Result<String, ArchiveToolError> {
    let Some(digest) = value.strip_prefix("sha256:") else {
        return Err(invalid_release("GitHub returned an invalid release digest"));
    };
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid_release("GitHub returned an invalid release digest"));
    }
    Ok(digest.to_ascii_lowercase())
}

fn request_release(
    tool: &ArchiveToolContract,
    endpoint: &str,
) -> Result<Vec<u8>, ArchiveToolError> {
    let agent = github_agent();
    request_release_with_agent(tool, endpoint, &agent)
}

fn request_release_with_agent(
    tool: &ArchiveToolContract,
    endpoint: &str,
    agent: &Agent,
) -> Result<Vec<u8>, ArchiveToolError> {
    let mut request = agent.get(endpoint);
    for (name, value) in request_headers(tool) {
        request = request.header(name, value);
    }
    let mut response = request.call().map_err(|cause| {
        ArchiveToolError::new(
            ArchiveToolErrorKind::GithubUnavailable,
            format!(
                "cannot resolve {} from GitHub Releases: {cause}",
                tool.display_name
            ),
        )
    })?;
    if response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|length| length > MAX_RELEASE_DOCUMENT_BYTES)
    {
        return Err(invalid_release(
            "GitHub release document exceeds the 4 MiB safety limit",
        ));
    }
    let mut document = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_RELEASE_DOCUMENT_BYTES + 1)
        .read_to_end(&mut document)
        .map_err(|cause| {
            ArchiveToolError::new(
                ArchiveToolErrorKind::GithubUnavailable,
                format!("cannot read the GitHub release document: {cause}"),
            )
        })?;
    if document.len() as u64 > MAX_RELEASE_DOCUMENT_BYTES {
        return Err(invalid_release(
            "GitHub release document exceeds the 4 MiB safety limit",
        ));
    }
    Ok(document)
}

fn github_agent() -> Agent {
    Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .timeout_connect(Some(Duration::from_secs(30)))
        .max_redirects(0)
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into()
}

fn latest_endpoint(tool: &ArchiveToolContract) -> String {
    format!(
        "{GITHUB_API_ROOT}/repos/{}/releases/latest",
        tool.release_repository()
    )
}

fn release_endpoint(tool: &ArchiveToolContract, tag: &str) -> String {
    format!(
        "{GITHUB_API_ROOT}/repos/{}/releases/tags/{tag}",
        tool.release_repository()
    )
}

fn request_headers<'a>(tool: &'a ArchiveToolContract) -> [(&'static str, &'a str); 3] {
    [
        ("Accept", "application/vnd.github+json"),
        ("X-GitHub-Api-Version", tool.release_api_version()),
        ("User-Agent", USER_AGENT),
    ]
}

fn require_definition(
    tool: &ArchiveToolContract,
    resolved: &ResolvedDefinition,
) -> Result<(), ArchiveToolError> {
    if resolved.tool_name() != tool.name || !tool.accepts_exact_version(resolved.version()) {
        return Err(invalid_release(
            "the resolved definition does not match its GitHub Release contract",
        ));
    }
    Ok(())
}

fn invalid_release(message: impl Into<String>) -> ArchiveToolError {
    ArchiveToolError::new(ArchiveToolErrorKind::GithubReleaseInvalid, message)
}

#[cfg(test)]
mod tests;
