use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::development::archive_tool::github;
use crate::development::archive_tool::install::{
    ArchiveSource, InstallOutcome, InstallRequest, ensure_installed_observed,
};
use crate::development::archive_tool::{
    ArchiveToolError, ArchiveToolRequest, ArchiveToolStore, ResolvedDefinition, Trust,
};
use crate::development::msvc::{
    MsvcDefinition, MsvcInstallContext, MsvcInstallOutcome,
    ensure_installed as ensure_msvc_installed,
};
use crate::development::rust::{
    RustDefinition, RustInstallContext, RustInstallOutcome,
    ensure_installed as ensure_rust_installed,
};
use crate::development::{ArchiveToolContract, BUN, PWSH};

use super::PUBLICATION_TOKEN_VARIABLE;
use super::declaration::DeclarationSnapshot;
use super::environment::EnvironmentPlan;
use super::provider::SetupProvider;
use super::storage::{ExclusiveFileLock, ensure_directory_chain};

pub struct NativeSetupContext {
    data_root: PathBuf,
    cache_data_root: PathBuf,
    profile_revision: String,
    input_revision: String,
}

impl NativeSetupContext {
    pub fn new(
        data_root: impl Into<PathBuf>,
        cache_data_root: impl Into<PathBuf>,
        profile_revision: impl Into<String>,
        input_revision: impl Into<String>,
    ) -> Result<Self, String> {
        let data_root = data_root.into();
        let cache_data_root = cache_data_root.into();
        if !data_root.is_absolute() || !cache_data_root.is_absolute() {
            return Err("native development setup roots must be absolute".to_owned());
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
pub struct MsvcSetupResult {
    channel: String,
    tool_version: String,
    sdk_version: String,
    root: PathBuf,
    outcome: MsvcInstallOutcome,
    warnings: Vec<String>,
}

impl MsvcSetupResult {
    pub fn channel(&self) -> &str {
        &self.channel
    }

    pub fn tool_version(&self) -> &str {
        &self.tool_version
    }

    pub fn sdk_version(&self) -> &str {
        &self.sdk_version
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn outcome(&self) -> MsvcInstallOutcome {
        self.outcome
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeSetupResult {
    archive_tools: Vec<ArchiveToolSetupResult>,
    msvc: Option<MsvcSetupResult>,
    rust: Option<RustSetupResult>,
    environment_changed: bool,
}

impl NativeSetupResult {
    pub fn archive_tools(&self) -> &[ArchiveToolSetupResult] {
        &self.archive_tools
    }

    pub fn msvc(&self) -> Option<&MsvcSetupResult> {
        self.msvc.as_ref()
    }

    pub fn rust(&self) -> Option<&RustSetupResult> {
        self.rust.as_ref()
    }

    pub fn environment_changed(&self) -> bool {
        self.environment_changed
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustSetupResult {
    toolchain: String,
    root: PathBuf,
    outcome: RustInstallOutcome,
    warnings: Vec<String>,
}

impl RustSetupResult {
    pub fn toolchain(&self) -> &str {
        &self.toolchain
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn outcome(&self) -> RustInstallOutcome {
        self.outcome
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

pub fn run_native(
    context: &NativeSetupContext,
    declarations: &DeclarationSnapshot,
    progress: &mut dyn FnMut(&str, u64, Option<u64>),
) -> Result<NativeSetupResult, String> {
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
    require_native_domains(declarations)?;
    let archive_requests = [&BUN, &PWSH]
        .into_iter()
        .filter_map(|tool| {
            declarations
                .archive_request(tool)
                .map(|request| request.map(|request| (tool, request)))
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let msvc_definition = declarations
        .msvc_definition()
        .map_err(|error| error.to_string())?;
    let rust_definition = declarations
        .rust_definition()
        .map_err(|error| error.to_string())?;
    let mut plan = EnvironmentPlan::default();
    let mut archive_tools = Vec::new();
    for (tool, request) in archive_requests {
        let installed = setup_archive_tool(context, tool, request, progress)?;
        plan.prepend_path(installed.root.clone())?;
        archive_tools.push(installed);
    }
    let msvc = setup_msvc(context, msvc_definition.as_ref(), &mut plan, progress)?;
    let rust = setup_rust(context, rust_definition.as_ref(), &mut plan, progress)?;
    plan.set(PUBLICATION_TOKEN_VARIABLE, Some(publication.token()))?;
    let environment_changed = plan.render().publish(&context.data_root)?;
    provider.complete(&publication)?;
    Ok(NativeSetupResult {
        archive_tools,
        msvc,
        rust,
        environment_changed,
    })
}

fn require_native_domains(declarations: &DeclarationSnapshot) -> Result<(), String> {
    let unsupported = declarations
        .enabled_modules()
        .into_iter()
        .filter(|name| !matches!(*name, "bun" | "pwsh" | "msvc" | "rust"))
        .collect::<Vec<_>>();
    if unsupported.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "native development setup does not yet handle these enabled declarations: {}.",
            unsupported.join(", ")
        ))
    }
}

fn setup_archive_tool(
    context: &NativeSetupContext,
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

fn setup_msvc(
    context: &NativeSetupContext,
    definition: Option<&MsvcDefinition>,
    plan: &mut EnvironmentPlan,
    progress: &mut dyn FnMut(&str, u64, Option<u64>),
) -> Result<Option<MsvcSetupResult>, String> {
    let Some(definition) = definition else {
        return Ok(None);
    };
    let mut msvc_progress = |_: &str, current, total| progress("msvc", current, total);
    let result = ensure_msvc_installed(
        MsvcInstallContext::new(&context.data_root, &context.cache_data_root)
            .map_err(|error| error.to_string())?,
        &definition,
        &mut msvc_progress,
    )
    .map_err(|error| error.to_string())?;
    result.installation().add_environment(plan)?;
    Ok(Some(MsvcSetupResult {
        channel: definition.channel().to_owned(),
        tool_version: result.installation().tool_version().to_owned(),
        sdk_version: result.installation().sdk_version().to_owned(),
        root: result.installation().root().to_path_buf(),
        outcome: result.outcome(),
        warnings: result.warnings().to_vec(),
    }))
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

fn setup_rust(
    context: &NativeSetupContext,
    definition: Option<&RustDefinition>,
    plan: &mut EnvironmentPlan,
    progress: &mut dyn FnMut(&str, u64, Option<u64>),
) -> Result<Option<RustSetupResult>, String> {
    let Some(definition) = definition else {
        return Ok(None);
    };
    let mut rust_progress = |current, total| progress("rust", current, total);
    let result = ensure_rust_installed(
        RustInstallContext::new(&context.data_root, &context.cache_data_root)
            .map_err(|error| error.to_string())?,
        definition,
        &mut rust_progress,
    )
    .map_err(|error| error.to_string())?;
    result.installation().add_environment(definition, plan)?;
    Ok(Some(RustSetupResult {
        toolchain: definition.toolchain().to_owned(),
        root: result.installation().root().to_path_buf(),
        outcome: result.outcome(),
        warnings: result.warnings().to_vec(),
    }))
}

#[cfg(test)]
#[path = "native/tests.rs"]
mod tests;
