use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::{
    binding::ProjectBinding,
    catalog::{CommandNode, CommandSource},
    command_event::{COMMAND_EVENT_FRAME_PROTOCOL, COMMAND_EVENT_PROTOCOL_ENV},
    context::EntryContext,
    development::setup::environment::EnvironmentPlan,
    launch::{ENTRY_FILE_ENV, LAUNCH_MODE_ENV},
    profile::{EntryProfile, EntryProfileRecord},
};

use super::{CommandError, CommandResult, GuardScope, ResolvedCommand};

const TRANSIENT_ENVIRONMENT: [&str; 3] = [
    ENTRY_FILE_ENV,
    LAUNCH_MODE_ENV,
    "SWAWKIT_PROJ_CORE_COMMAND_GUARD_SCOPE",
];
const DEVELOPMENT_ENVIRONMENT_VARIABLES: &[&str] = &[
    "CARGO_BUILD_RUSTC",
    "CARGO_BUILD_RUSTDOC",
    "CARGO_HOME",
    "INCLUDE",
    "LIB",
    "RUSTC",
    "RUSTDOC",
    "RUSTUP_DIST_ROOT",
    "RUSTUP_DIST_SERVER",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "RUSTUP_TOOLCHAIN_SOURCE",
    "RUSTUP_UPDATE_ROOT",
    "RUSTUP_VERSION",
    "UCRTVersion",
    "UniversalCRTSdkDir",
    "VCINSTALLDIR",
    "VCToolsInstallDir",
    "VCToolsVersion",
    "VSCMD_ARG_HOST_ARCH",
    "VSCMD_ARG_TGT_ARCH",
    "WindowsSDKVersion",
    "WindowsSdkBinPath",
    "WindowsSdkVerBinPath",
];
const DEVELOPMENT_METADATA_PREFIX: &str = "SWAWKIT_PROJ_MODULE_KERNEL_DEV_SETUP_";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandExecutionContext {
    pub swawkit_home: PathBuf,
    pub kernel_root: PathBuf,
    pub target_project_root: PathBuf,
    pub action_root: PathBuf,
    pub data_root: PathBuf,
    pub entry_name: String,
    pub entry_file: PathBuf,
    pub invocation_directory: PathBuf,
    pub toolchain_executable: PathBuf,
    pub profile: EntryProfileRecord,
    pub environment_input_revision: String,
    pub profile_revision: String,
    pub process_mode: CommandProcessMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommandProcessMode {
    #[default]
    InheritConsole,
    NoWindow,
}

impl CommandExecutionContext {
    pub fn new(
        entry: &EntryContext,
        profile: &EntryProfile,
        data_root: impl Into<PathBuf>,
        process_mode: CommandProcessMode,
    ) -> Self {
        let binding = profile.binding();
        Self {
            swawkit_home: entry.swawkit_home.clone(),
            kernel_root: entry.kernel_root(),
            target_project_root: binding.target_project_root().to_path_buf(),
            action_root: binding.action_root(),
            data_root: data_root.into(),
            entry_name: entry.entry_name.clone(),
            entry_file: entry.entry_file.clone(),
            invocation_directory: entry.invocation_directory.clone(),
            toolchain_executable: entry.sibling_product_executable("swawkit-proj-toolchain.exe"),
            profile: profile.record().clone(),
            environment_input_revision: profile.environment_input_revision().to_owned(),
            profile_revision: profile.profile_revision().to_owned(),
            process_mode,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExecutionPhase {
    Run,
    Guard(GuardScope),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProcessEnvironment {
    values: BTreeMap<OsString, Option<OsString>>,
}

impl ProcessEnvironment {
    pub(crate) fn for_command(
        context: &CommandExecutionContext,
        protocol_command: &ResolvedCommand,
        phase: ExecutionPhase,
    ) -> CommandResult<Self> {
        let mut environment = Self::default();
        for name in TRANSIENT_ENVIRONMENT {
            environment.remove(name);
        }
        environment.set("SWAWKIT_PROJ_CORE_COMMAND_PROTOCOL", "1");
        environment.set(COMMAND_EVENT_PROTOCOL_ENV, COMMAND_EVENT_FRAME_PROTOCOL);
        environment.set(
            "SWAWKIT_PROJ_CORE_COMMAND_PHASE",
            match phase {
                ExecutionPhase::Run => "run",
                ExecutionPhase::Guard(_) => "guard",
            },
        );
        environment.set(
            "SWAWKIT_PROJ_CORE_COMMAND_ADDRESS",
            &protocol_command.address,
        );
        environment.set("SWAWKIT_PROJ_CORE_COMMAND_DIR", &protocol_command.directory);
        environment.set(
            "SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT",
            command_data_root(context, protocol_command)?,
        );
        if let ExecutionPhase::Guard(scope) = phase {
            environment.set("SWAWKIT_PROJ_CORE_COMMAND_GUARD_SCOPE", scope.as_str());
        }
        environment.set(
            "SWAWKIT_PROJ_CORE_COMMAND_INVOCATION_DIR",
            &context.invocation_directory,
        );
        environment.set("SWAWKIT_HOME", &context.swawkit_home);
        environment.set(
            "SWAWKIT_PROJ_TARGET_PROJECT_ROOT",
            &context.target_project_root,
        );
        environment.set("SWAWKIT_PROJ_ACTION_ROOT", &context.action_root);
        environment.set("SWAWKIT_PROJ_DATA_ROOT", &context.data_root);
        environment.set("SWAWKIT_PROJ_ENTRY_COMMAND", &context.entry_name);
        environment.set("SWAWKIT_PROJ_CORE_COMMAND_ENTRY_FILE", &context.entry_file);
        validate_toolchain_executable(&context.toolchain_executable)?;
        environment.set(
            "SWAWKIT_PROJ_CORE_TOOLCHAIN_EXECUTABLE",
            &context.toolchain_executable,
        );
        environment.set(
            "SWAWKIT_PROJ_CORE_COMMAND_ENVIRONMENT_INPUT_REVISION",
            &context.environment_input_revision,
        );
        environment.set(
            "SWAWKIT_PROJ_CORE_COMMAND_PROFILE_REVISION",
            &context.profile_revision,
        );
        environment.apply_profile(&context.profile);
        Ok(environment)
    }

    fn apply_profile(&mut self, profile: &EntryProfileRecord) {
        for (name, value, omit_when_empty) in profile.published_environment_variables() {
            if omit_when_empty {
                self.set_optional(name, &value);
            } else {
                self.set(name, value);
            }
        }
    }

    pub(crate) fn apply_development_environment(
        &mut self,
        plan: &EnvironmentPlan,
        managed_root: &Path,
    ) -> CommandResult<()> {
        for name in DEVELOPMENT_ENVIRONMENT_VARIABLES {
            self.remove(name);
        }
        for (name, _) in env::vars_os() {
            if name
                .to_str()
                .is_some_and(|name| has_ascii_prefix(name, DEVELOPMENT_METADATA_PREFIX))
            {
                self.remove(name);
            }
        }
        for (name, value) in plan.variables() {
            match value {
                Some(value) => self.set(name, value),
                None => self.remove(name),
            }
        }
        self.prepend_paths(plan.paths(), Some(managed_root))
    }

    fn prepend_paths(
        &mut self,
        directories: &[PathBuf],
        excluded_inherited_root: Option<&Path>,
    ) -> CommandResult<()> {
        let mut paths = directories.to_vec();
        if let Some(inherited) = env::var_os("PATH") {
            paths.extend(env::split_paths(&inherited).filter(|path| {
                excluded_inherited_root.is_none_or(|root| !path_is_within_windows(path, root))
            }));
        }
        let value = env::join_paths(paths).map_err(|error| {
            CommandError::new(format!(
                "cannot publish the Entry tool path '{}': {error}",
                directories
                    .first()
                    .map_or_else(|| "<empty>".into(), |path| path.display().to_string())
            ))
        })?;
        self.set("PATH", value);
        Ok(())
    }

    fn set(&mut self, name: impl Into<OsString>, value: impl AsRef<OsStr>) {
        self.values
            .insert(name.into(), Some(value.as_ref().to_os_string()));
    }

    fn remove(&mut self, name: impl Into<OsString>) {
        self.values.insert(name.into(), None);
    }

    fn set_optional(&mut self, name: &'static str, value: &str) {
        if value.is_empty() {
            self.remove(name);
        } else {
            self.set(name, value);
        }
    }

    pub(crate) fn apply(&self, command: &mut Command) {
        for (name, value) in &self.values {
            match value {
                Some(value) => {
                    command.env(name, value);
                }
                None => {
                    command.env_remove(name);
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn value(&self, name: &str) -> Option<Option<&OsStr>> {
        self.values
            .get(OsStr::new(name))
            .map(|value| value.as_deref())
    }
}

fn has_ascii_prefix(value: &str, prefix: &str) -> bool {
    value
        .get(..prefix.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
}

fn path_is_within_windows(path: &Path, root: &Path) -> bool {
    let path = path.to_string_lossy();
    let root = root.to_string_lossy();
    path.eq_ignore_ascii_case(&root)
        || path
            .get(..root.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&root))
            && path
                .as_bytes()
                .get(root.len())
                .is_some_and(|byte| *byte == b'\\' || *byte == b'/')
}

fn validate_toolchain_executable(path: &Path) -> CommandResult<()> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        CommandError::new(format!(
            "the Runtime Release Toolchain is unavailable at '{}': {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(CommandError::new(format!(
            "the Runtime Release Toolchain is not a regular file: '{}'",
            path.display()
        )));
    }
    Ok(())
}

pub(crate) fn command_data_root(
    context: &CommandExecutionContext,
    command: &ResolvedCommand,
) -> CommandResult<PathBuf> {
    let (source_name, source_root) = match command.source {
        CommandSource::Control => ("control", &context.kernel_root),
        CommandSource::Kernel => ("kernel", &context.kernel_root),
        CommandSource::Action => ("action", &context.action_root),
    };
    module_data_root(
        &context.data_root,
        source_name,
        source_root,
        &command.directory,
        &command.address,
    )
}

pub(crate) fn catalog_command_data_root(
    context: &EntryContext,
    data_root: &Path,
    binding: Option<&ProjectBinding>,
    command: &CommandNode,
) -> CommandResult<PathBuf> {
    let kernel_root = context.kernel_root();
    let action_root = binding.map(ProjectBinding::action_root);
    let (source_name, source_root) = match command.source {
        CommandSource::Control => ("control", kernel_root.as_path()),
        CommandSource::Kernel => ("kernel", kernel_root.as_path()),
        CommandSource::Action => (
            "action",
            action_root.as_deref().ok_or_else(|| {
                CommandError::new("a ready Entry Profile is required to locate Action command data")
            })?,
        ),
    };
    module_data_root(
        data_root,
        source_name,
        source_root,
        &command.directory,
        &command.address,
    )
}

fn module_data_root(
    data_root: &Path,
    source_name: &str,
    source_root: &Path,
    command_directory: &Path,
    address: &str,
) -> CommandResult<PathBuf> {
    let relative = command_directory.strip_prefix(source_root).map_err(|_| {
        CommandError::new(format!(
            "Catalog invariant failed for '{}': command directory is outside its source root",
            address
        ))
    })?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(CommandError::new(format!(
            "Catalog invariant failed for '{}': command directory has an unsafe relative path",
            address
        )));
    }
    Ok(data_root.join("modules").join(source_name).join(relative))
}
