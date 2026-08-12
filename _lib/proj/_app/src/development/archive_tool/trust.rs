use super::{
    ArchiveToolContract, InstallMetadata, ResolvedDefinition, ResolvedVerification,
    SourceVerification, Trust, TrustLevel,
};

pub(super) fn evaluate(
    tool: &ArchiveToolContract,
    resolved: &ResolvedDefinition,
    metadata: Option<&InstallMetadata>,
) -> Trust {
    if !resolved.project_sha256.is_empty() {
        return Trust {
            level: TrustLevel::Pinned,
            message: "project SHA-256",
            warning: None,
        };
    }
    let verification = metadata
        .map(|value| ResolvedVerification::Published(value.source_verification))
        .unwrap_or(resolved.verification);
    if verification == ResolvedVerification::Published(SourceVerification::Github) {
        return Trust {
            level: TrustLevel::Upstream,
            message: "GitHub Release digest",
            warning: Some(format!(
                "{} {} was verified with the GitHub Release digest; {} is not pinned by this project.",
                tool.display_name, resolved.version, tool.hash_variable
            )),
        };
    }
    if verification == ResolvedVerification::Unresolved {
        return Trust {
            level: TrustLevel::Unpinned,
            message: "awaiting GitHub Release resolution",
            warning: Some(format!(
                "{} {} is not pinned by {}; .dev.setup will use the GitHub Release digest when available.",
                tool.display_name, resolved.version, tool.hash_variable
            )),
        };
    }
    Trust {
        level: TrustLevel::Unpinned,
        message: "no comparable release SHA-256",
        warning: Some(format!(
            "{} {} has no comparable GitHub Release digest or project SHA-256; installation is allowed but not content-pinned.",
            tool.display_name, resolved.version
        )),
    }
}
