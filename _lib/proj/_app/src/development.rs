use sha2::{Digest, Sha256};

pub mod archive_tool;
pub mod setup;

#[derive(Clone, Copy)]
struct GithubReleaseContract {
    repository: &'static str,
    api_version: &'static str,
    tag_prefix: &'static str,
    asset_name: fn(&str) -> String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GithubReleaseCoordinates {
    pub(crate) tag: String,
    pub(crate) asset: String,
    pub(crate) download_url: String,
    pub(crate) source_identity: String,
}

impl GithubReleaseContract {
    fn coordinates(&self, version: &str) -> GithubReleaseCoordinates {
        let tag = format!("{}{version}", self.tag_prefix);
        let asset = (self.asset_name)(version);
        GithubReleaseCoordinates {
            download_url: format!(
                "https://github.com/{}/releases/download/{tag}/{asset}",
                self.repository
            ),
            source_identity: format!("github:{}@{tag}#{asset}", self.repository),
            tag,
            asset,
        }
    }

    fn version_from_tag<'a>(&self, tag: &'a str) -> Option<&'a str> {
        tag.strip_prefix(self.tag_prefix)
            .filter(|value| !value.is_empty())
    }
}

pub struct ArchiveToolContract {
    pub name: &'static str,
    pub display_name: &'static str,
    pub mode_variable: &'static str,
    pub version_variable: &'static str,
    pub hash_variable: &'static str,
    pub selection_schema: &'static str,
    pub recipe_version: &'static str,
    pub executable: &'static str,
    pub required_paths: &'static [&'static str],
    pub archive_subdir: &'static str,
    github_release: GithubReleaseContract,
    exact_version: fn(&str) -> bool,
}

impl ArchiveToolContract {
    pub fn accepts_exact_version(&self, value: &str) -> bool {
        (self.exact_version)(value)
    }

    pub fn source_identity(&self, version: &str) -> String {
        self.github_release.coordinates(version).source_identity
    }

    pub fn archive_name(&self, version: &str) -> String {
        self.github_release.coordinates(version).asset
    }

    pub(crate) fn release_coordinates(&self, version: &str) -> GithubReleaseCoordinates {
        self.github_release.coordinates(version)
    }

    pub(crate) fn release_repository(&self) -> &'static str {
        self.github_release.repository
    }

    pub(crate) fn release_api_version(&self) -> &'static str {
        self.github_release.api_version
    }

    pub(crate) fn release_version_from_tag<'a>(&self, tag: &'a str) -> Option<&'a str> {
        self.github_release.version_from_tag(tag)
    }

    pub fn definition_signature(&self, version: &str, project_sha256: &str) -> String {
        let identity = [
            "swawkit.proj-dev.definition.v1".to_owned(),
            self.name.to_owned(),
            "managed".to_owned(),
            version.to_owned(),
            self.source_identity(version),
            project_sha256.to_owned(),
            self.archive_subdir.to_owned(),
            self.recipe_version.to_owned(),
            self.executable.to_owned(),
            self.required_paths.join("|"),
        ]
        .join("\n");
        format!("{:x}", Sha256::digest(identity.as_bytes()))
    }
}

pub const BUN: ArchiveToolContract = ArchiveToolContract {
    name: "bun",
    display_name: "Bun",
    mode_variable: "SWAWKIT_PROJ_BUN_MODE",
    version_variable: "SWAWKIT_PROJ_BUN_VERSION",
    hash_variable: "SWAWKIT_PROJ_BUN_SHA256",
    selection_schema: "swawkit.proj-dev.bun-selection.v0",
    recipe_version: "2",
    executable: "bun.exe",
    required_paths: &["bun.exe", "bunx.cmd"],
    archive_subdir: "bun-windows-x64",
    github_release: GithubReleaseContract {
        repository: "oven-sh/bun",
        api_version: "2026-03-10",
        tag_prefix: "bun-v",
        asset_name: bun_archive_name,
    },
    // Exact declarations preserve Bun's safe non-semver channels. GitHub
    // `latest` is deliberately narrowed to an immutable semantic version.
    exact_version: is_safe_segment,
};

pub const PWSH: ArchiveToolContract = ArchiveToolContract {
    name: "pwsh",
    display_name: "PowerShell",
    mode_variable: "SWAWKIT_PROJ_PWSH_MODE",
    version_variable: "SWAWKIT_PROJ_PWSH_VERSION",
    hash_variable: "SWAWKIT_PROJ_PWSH_SHA256",
    selection_schema: "swawkit.proj-dev.pwsh-selection.v0",
    recipe_version: "pwsh-win-x64-zip-v0",
    executable: "pwsh.exe",
    required_paths: &["pwsh.exe"],
    archive_subdir: "",
    github_release: GithubReleaseContract {
        repository: "PowerShell/PowerShell",
        api_version: "2026-03-10",
        tag_prefix: "v",
        asset_name: pwsh_archive_name,
    },
    exact_version: is_semantic_version,
};

pub fn is_safe_segment(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && bytes
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
}

pub fn is_semantic_version(value: &str) -> bool {
    let (version, suffix) = value.split_once('-').unwrap_or((value, ""));
    if value.contains('-')
        && (suffix.is_empty()
            || !suffix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')))
    {
        return false;
    }
    let fields: Vec<_> = version.split('.').collect();
    fields.len() == 3
        && fields
            .iter()
            .all(|field| !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()))
}

fn bun_archive_name(_version: &str) -> String {
    "bun-windows-x64.zip".to_owned()
}

fn pwsh_archive_name(version: &str) -> String {
    format!("PowerShell-{version}-win-x64.zip")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bun_contract_matches_the_published_v2_identity() {
        assert_eq!(BUN.recipe_version, "2");
        assert_eq!(BUN.required_paths, ["bun.exe", "bunx.cmd"]);
        assert!(BUN.accepts_exact_version("1.2.3-canary.1"));
        assert!(!BUN.accepts_exact_version("../1.2.3"));
        assert_eq!(
            BUN.release_coordinates("1.2.3").download_url,
            "https://github.com/oven-sh/bun/releases/download/bun-v1.2.3/bun-windows-x64.zip"
        );
        assert_eq!(
            BUN.source_identity("1.2.3"),
            "github:oven-sh/bun@bun-v1.2.3#bun-windows-x64.zip"
        );
        assert_eq!(BUN.definition_signature("1.2.3", "").len(), 64);
    }

    #[test]
    fn powershell_coordinates_share_one_release_contract() {
        let coordinates = PWSH.release_coordinates("7.6.4");
        assert_eq!(coordinates.tag, "v7.6.4");
        assert_eq!(coordinates.asset, "PowerShell-7.6.4-win-x64.zip");
        assert_eq!(
            PWSH.github_release.version_from_tag(&coordinates.tag),
            Some("7.6.4")
        );
    }
}
