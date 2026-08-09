use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Component, PathBuf};
use std::process::Command;

use crate::{
    catalog::CommandSource,
    context::EntryContext,
    launch::{ENTRY_FILE_ENV, LAUNCH_MODE_ENV},
    profile::{EntryProfile, EntryProfileRecord},
};

use super::{CommandError, CommandResult, GuardScope, ResolvedCommand};

const TRANSIENT_ENVIRONMENT: [&str; 3] = [
    ENTRY_FILE_ENV,
    LAUNCH_MODE_ENV,
    "SWAWKIT_PROJ_CORE_COMMAND_GUARD_SCOPE",
];

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
    pub profile: EntryProfileRecord,
    pub environment_input_revision: String,
    pub profile_revision: String,
}

impl CommandExecutionContext {
    pub fn new(
        entry: &EntryContext,
        profile: &EntryProfile,
        data_root: impl Into<PathBuf>,
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
            profile: profile.record().clone(),
            environment_input_revision: profile.environment_input_revision().to_owned(),
            profile_revision: profile.profile_revision().to_owned(),
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

fn command_data_root(
    context: &CommandExecutionContext,
    command: &ResolvedCommand,
) -> CommandResult<PathBuf> {
    let (source_name, source_root) = match command.source {
        CommandSource::Control => ("control", &context.kernel_root),
        CommandSource::Kernel => ("kernel", &context.kernel_root),
        CommandSource::Action => ("action", &context.action_root),
    };
    let relative = command.directory.strip_prefix(source_root).map_err(|_| {
        CommandError::new(format!(
            "Catalog invariant failed for '{}': command directory is outside its source root",
            command.address
        ))
    })?;
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(CommandError::new(format!(
            "Catalog invariant failed for '{}': command directory has an unsafe relative path",
            command.address
        )));
    }
    Ok(context
        .data_root
        .join("modules")
        .join(source_name)
        .join(relative))
}
