use super::{ArchiveTool, InstallMetadata, ResolvedDefinition};

pub(in crate::command::status) struct Trust {
    pub(super) level: &'static str,
    pub(super) message: &'static str,
    pub(super) warning: Option<String>,
}

pub(super) fn trust(
    tool: &ArchiveTool,
    resolved: &ResolvedDefinition,
    project_sha256: &str,
    metadata: Option<&InstallMetadata>,
) -> Trust {
    if !project_sha256.is_empty() {
        return Trust {
            level: "pinned",
            message: "project SHA-256",
            warning: None,
        };
    }
    let verification = metadata
        .map(|value| value.source_verification.as_str())
        .unwrap_or(&resolved.verification);
    if verification == "github" {
        return Trust {
            level: "upstream",
            message: "GitHub Release digest",
            warning: Some(format!(
                "{} {} was verified with the GitHub Release digest; {} is not pinned by this project.",
                tool.display_name, resolved.version, tool.hash_variable
            )),
        };
    }
    if metadata.is_none() {
        return Trust {
            level: "unpinned",
            message: "awaiting GitHub Release resolution",
            warning: Some(format!(
                "{} {} is not pinned by {}; .dev.setup will use the GitHub Release digest when available.",
                tool.display_name, resolved.version, tool.hash_variable
            )),
        };
    }
    Trust {
        level: "unpinned",
        message: "no comparable release SHA-256",
        warning: Some(format!(
            "{} {} has no comparable GitHub Release digest or project SHA-256; installation is allowed but not content-pinned.",
            tool.display_name, resolved.version
        )),
    }
}
