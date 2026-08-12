use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::development::archive_tool::github;
use crate::development::archive_tool::install::{
    ArchiveSource, InstallOutcome, InstallRequest, ensure_installed_observed,
};
use crate::development::archive_tool::{
    ArchiveToolError, ArchiveToolRequest, ArchiveToolStore, ResolvedDefinition, Trust,
};
use crate::development::{ArchiveToolContract, BUN, PWSH};

use super::PUBLICATION_TOKEN_VARIABLE;
use super::declaration::DeclarationSnapshot;
use super::environment::EnvironmentPlan;
use super::provider::SetupProvider;
use super::storage::{ExclusiveFileLock, ensure_directory_chain};

pub struct ArchiveSetupContext {
    data_root: PathBuf,
    cache_data_root: PathBuf,
    profile_revision: String,
    input_revision: String,
}

impl ArchiveSetupContext {
    pub fn new(
        data_root: impl Into<PathBuf>,
        cache_data_root: impl Into<PathBuf>,
        profile_revision: impl Into<String>,
        input_revision: impl Into<String>,
    ) -> Result<Self, String> {
        let data_root = data_root.into();
        let cache_data_root = cache_data_root.into();
        if !data_root.is_absolute() || !cache_data_root.is_absolute() {
            return Err("native archive setup roots must be absolute".to_owned());
        }
        Ok(Self {
            data_root,
            cache_data_root,
            profile_revision: profile_revision.into(),
            input_revision: input_revision.into(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveToolSetupResult {
    name: &'static str,
    requested: String,
    version: String,
    root: PathBuf,
    outcome: InstallOutcome,
    trust: Trust,
    warnings: Vec<String>,
}

impl ArchiveToolSetupResult {
    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn requested(&self) -> &str {
        &self.requested
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn outcome(&self) -> InstallOutcome {
        self.outcome
    }

    pub fn trust(&self) -> &Trust {
        &self.trust
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveSetupResult {
    tools: Vec<ArchiveToolSetupResult>,
    environment_changed: bool,
}

impl ArchiveSetupResult {
    pub fn tools(&self) -> &[ArchiveToolSetupResult] {
        &self.tools
    }

    pub fn environment_changed(&self) -> bool {
        self.environment_changed
    }
}

pub fn run_archive_only(
    context: &ArchiveSetupContext,
    declarations: &DeclarationSnapshot,
    progress: &mut dyn FnMut(&str, u64, Option<u64>),
) -> Result<ArchiveSetupResult, String> {
    let locks = ensure_directory_chain(
        &context.data_root,
        &["modules", "kernel", ".dev", "setup", "locks"],
        "development setup locks",
    )
    .map_err(|error| error.to_string())?;
    let _setup_lock =
        ExclusiveFileLock::acquire(&locks.join("setup.lock"), Duration::from_secs(600))
            .map_err(|error| format!("cannot acquire development setup lock: {error}"))?;
    let provider = SetupProvider::new(
        &context.data_root,
        &context.profile_revision,
        &context.input_revision,
    )?;
    let publication = provider.start()?;

    declarations
        .require_supported()
        .map_err(|error| error.to_string())?;
    require_archive_only(declarations)?;
    let mut plan = EnvironmentPlan::default();
    let mut tools = Vec::new();
    for tool in [&BUN, &PWSH] {
        if let Some(request) = declarations
            .archive_request(tool)
            .map_err(|error| error.to_string())?
        {
            let installed = setup_tool(context, tool, request, progress)?;
            plan.prepend_path(installed.root.clone())?;
            tools.push(installed);
        }
    }
    plan.set(PUBLICATION_TOKEN_VARIABLE, Some(publication.token()))?;
    let environment_changed = plan.render().publish(&context.data_root)?;
    provider.complete(&publication)?;
    Ok(ArchiveSetupResult {
        tools,
        environment_changed,
    })
}

fn require_archive_only(declarations: &DeclarationSnapshot) -> Result<(), String> {
    let unsupported = declarations
        .enabled_modules()
        .into_iter()
        .filter(|name| !matches!(*name, "bun" | "pwsh"))
        .collect::<Vec<_>>();
    if unsupported.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "native archive setup does not yet handle these enabled declarations: {}.",
            unsupported.join(", ")
        ))
    }
}

fn setup_tool(
    context: &ArchiveSetupContext,
    tool: &'static ArchiveToolContract,
    request: ArchiveToolRequest,
    progress: &mut dyn FnMut(&str, u64, Option<u64>),
) -> Result<ArchiveToolSetupResult, String> {
    let store = ArchiveToolStore::new(&context.data_root, tool);
    let (resolved, pending_source) =
        match store.resolve(&request).map_err(|error| error.to_string())? {
            Some(resolved) => (resolved, None),
            None => {
                let release =
                    github::resolve_latest(tool, &request).map_err(|error| error.to_string())?;
                let (resolved, source) = release.into_parts();
                (resolved, Some(source))
            }
        };
    let install_request = InstallRequest::new(
        &context.data_root,
        &context.cache_data_root,
        tool,
        resolved.clone(),
    )
    .map_err(|error| error.to_string())?;
    let requested = request.requested().to_owned();
    let mut tool_progress = |current, total| progress(tool.name, current, total);
    let result = ensure_installed_observed(
        install_request,
        move |current| resolve_source(tool, current, pending_source),
        &mut tool_progress,
    )
    .map_err(|error| error.to_string())?;
    Ok(ArchiveToolSetupResult {
        name: tool.name,
        requested,
        version: resolved.version().to_owned(),
        root: result.installation().root().to_path_buf(),
        outcome: result.outcome(),
        trust: result.trust().clone(),
        warnings: result.warnings().to_vec(),
    })
}

fn resolve_source(
    tool: &'static ArchiveToolContract,
    resolved: &ResolvedDefinition,
    pending: Option<ArchiveSource>,
) -> Result<ArchiveSource, ArchiveToolError> {
    if let Some(source) = pending {
        return Ok(source);
    }
    if resolved.requested_latest() {
        github::published_source(tool, resolved)
    } else {
        github::resolve_exact(tool, resolved).map(|release| release.into_parts().1)
    }
}

#[cfg(test)]
mod tests;
