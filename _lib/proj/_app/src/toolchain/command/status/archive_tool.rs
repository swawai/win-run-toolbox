use swawkit_proj::development::ArchiveToolContract as ArchiveTool;
use swawkit_proj::development::archive_tool::{ArchiveToolRequest, ArchiveToolStore, Trust};

use super::CommandContext;

pub(super) enum ArchiveReport {
    Off,
    LatestUnresolved {
        repair: String,
    },
    Resolved {
        version_label: String,
        ready: bool,
        trust: Trust,
    },
}

impl ArchiveReport {
    pub(super) fn render(&self, tool: &ArchiveTool) {
        match self {
            Self::Off => println!("[OFF] {} is disabled.", tool.name),
            Self::LatestUnresolved { repair } => {
                println!("[MISSING] {} latest unresolved; run '{repair}'", tool.name)
            }
            Self::Resolved {
                version_label,
                ready,
                trust,
            } => {
                let state = if *ready { "READY" } else { "MISSING" };
                println!(
                    "[{state}] {} {version_label}  {}  {}",
                    tool.name,
                    trust.level().as_str(),
                    trust.message()
                );
                if let Some(warning) = trust.warning() {
                    println!("WARNING: {warning}");
                }
            }
        }
    }
}

pub(super) fn inspect(
    context: &CommandContext,
    tool: &ArchiveTool,
) -> Result<ArchiveReport, String> {
    let mode = context.environment(tool.mode_variable).to_ascii_lowercase();
    if mode.is_empty() || mode == "disabled" {
        return Ok(ArchiveReport::Off);
    }
    if mode != "managed" {
        return Err(format!(
            "Unsupported {} value '{mode}'. Expected 'managed' or 'disabled'.",
            tool.mode_variable
        ));
    }

    let requested = context.environment(tool.version_variable);
    if requested.is_empty() {
        return Err(format!(
            "Enabled {} must declare {}.",
            tool.display_name, tool.version_variable
        ));
    }
    let request =
        ArchiveToolRequest::new(tool, &requested, &context.environment(tool.hash_variable))
            .map_err(|error| error.to_string())?;
    let store = ArchiveToolStore::new(&context.data_root, tool);
    let Some(resolved) = store.resolve(&request).map_err(|error| error.to_string())? else {
        return Ok(ArchiveReport::LatestUnresolved {
            repair: context.repair_invocation(),
        });
    };

    let installation = store.read_installation(&resolved).ok();
    let ready = installation
        .as_ref()
        .is_some_and(|value| store.verify_hashes(value).is_ok());
    let trust = store
        .trust(&resolved, installation.as_ref())
        .map_err(|error| error.to_string())?;
    let version_label = if resolved.requested_latest() {
        format!("latest -> {}", resolved.version())
    } else {
        resolved.version().to_owned()
    };
    Ok(ArchiveReport::Resolved {
        version_label,
        ready,
        trust,
    })
}
