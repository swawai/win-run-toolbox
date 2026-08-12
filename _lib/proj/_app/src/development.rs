use sha2::{Digest, Sha256};

pub mod archive_tool;

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
    archive_name: fn(&str) -> String,
    source_identity: fn(&str) -> String,
    exact_version: fn(&str) -> bool,
}

impl ArchiveToolContract {
    pub fn accepts_exact_version(&self, value: &str) -> bool {
        (self.exact_version)(value)
    }

    pub fn source_identity(&self, version: &str) -> String {
        (self.source_identity)(version)
    }

    pub fn archive_name(&self, version: &str) -> String {
        (self.archive_name)(version)
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
    archive_name: bun_archive_name,
    source_identity: bun_source_identity,
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
    archive_name: pwsh_archive_name,
    source_identity: pwsh_source_identity,
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

fn bun_source_identity(version: &str) -> String {
    format!("github:oven-sh/bun@bun-v{version}#bun-windows-x64.zip")
}

fn bun_archive_name(_version: &str) -> String {
    "bun-windows-x64.zip".to_owned()
}

fn pwsh_source_identity(version: &str) -> String {
    format!("github:PowerShell/PowerShell@v{version}#PowerShell-{version}-win-x64.zip")
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
        assert_eq!(BUN.definition_signature("1.2.3", "").len(), 64);
    }
}
