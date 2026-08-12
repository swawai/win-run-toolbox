use std::collections::BTreeMap;

use super::{EntryProfileRecord, ProfileError};
#[cfg(test)]
use crate::development::setup::declaration::provider_input_names;
use crate::development::setup::declaration::{InputNormalization, provider_input_normalization};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VariablePublication {
    ResolvedTarget,
    Always,
    NonEmpty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VariableSpec {
    group: &'static str,
    name: &'static str,
    field: &'static str,
    publication: VariablePublication,
}

const VARIABLE_SPECS: [VariableSpec; 32] = [
    variable("bun", "SWAWKIT_PROJ_BUN_MODE", "development.bun.mode"),
    optional("bun", "SWAWKIT_PROJ_BUN_SHA256", "development.bun.sha256"),
    variable("bun", "SWAWKIT_PROJ_BUN_VERSION", "development.bun.version"),
    optional("git", "SWAWKIT_PROJ_GIT_ID_ACCESS", "git.access"),
    optional("git", "SWAWKIT_PROJ_GIT_ID_EMAIL", "git.email"),
    optional("git", "SWAWKIT_PROJ_GIT_ID_NAME", "git.name"),
    optional("git", "SWAWKIT_PROJ_REPO_REMOTE", "repository.remote"),
    variable("go", "SWAWKIT_PROJ_GO_MODE", "development.go.mode"),
    optional("go", "SWAWKIT_PROJ_GO_SHA256", "development.go.sha256"),
    optional("go", "SWAWKIT_PROJ_GO_VERSION", "development.go.version"),
    variable(
        "msvc",
        "SWAWKIT_PROJ_MSVC_CHANNEL",
        "development.msvc.channel",
    ),
    variable("msvc", "SWAWKIT_PROJ_MSVC_MODE", "development.msvc.mode"),
    variable(
        "preferences",
        "SWAWKIT_PROJ_DEFAULT_IDE",
        "preferences.defaultIde",
    ),
    variable(
        "preferences",
        "SWAWKIT_PROJ_DEFAULT_SHELL",
        "preferences.defaultShell",
    ),
    optional(
        "preferences",
        "SWAWKIT_PROJ_HELP_LANG",
        "preferences.helpLanguage",
    ),
    resolved_target(
        "project",
        "SWAWKIT_PROJ_TARGET_PROJECT_ROOT",
        "targetProjectRoot",
    ),
    variable("pwsh", "SWAWKIT_PROJ_PWSH_MODE", "development.pwsh.mode"),
    optional(
        "pwsh",
        "SWAWKIT_PROJ_PWSH_SHA256",
        "development.pwsh.sha256",
    ),
    variable(
        "pwsh",
        "SWAWKIT_PROJ_PWSH_VERSION",
        "development.pwsh.version",
    ),
    variable(
        "python",
        "SWAWKIT_PROJ_PYTHON_MODE",
        "development.python.mode",
    ),
    optional(
        "python",
        "SWAWKIT_PROJ_PYTHON_SHA256",
        "development.python.sha256",
    ),
    variable(
        "python",
        "SWAWKIT_PROJ_PYTHON_VERSION",
        "development.python.version",
    ),
    variable("rust", "SWAWKIT_PROJ_RUST_HOST", "development.rust.host"),
    variable("rust", "SWAWKIT_PROJ_RUST_MODE", "development.rust.mode"),
    variable(
        "rust",
        "SWAWKIT_PROJ_RUST_PROFILE",
        "development.rust.profile",
    ),
    variable(
        "rust",
        "SWAWKIT_PROJ_RUST_TOOLCHAIN",
        "development.rust.toolchain",
    ),
    variable(
        "system",
        "SWAWKIT_PROJ_CURSOR_MODE",
        "development.cursor.mode",
    ),
    variable("system", "SWAWKIT_PROJ_GH_MODE", "development.gh.mode"),
    variable(
        "system",
        "SWAWKIT_PROJ_VSCODE_MODE",
        "development.vscode.mode",
    ),
    variable("uv", "SWAWKIT_PROJ_UV_MODE", "development.uv.mode"),
    optional("uv", "SWAWKIT_PROJ_UV_SHA256", "development.uv.sha256"),
    variable("uv", "SWAWKIT_PROJ_UV_VERSION", "development.uv.version"),
];

const fn variable(group: &'static str, name: &'static str, field: &'static str) -> VariableSpec {
    VariableSpec {
        group,
        name,
        field,
        publication: VariablePublication::Always,
    }
}

const fn optional(group: &'static str, name: &'static str, field: &'static str) -> VariableSpec {
    VariableSpec {
        group,
        name,
        field,
        publication: VariablePublication::NonEmpty,
    }
}

const fn resolved_target(
    group: &'static str,
    name: &'static str,
    field: &'static str,
) -> VariableSpec {
    VariableSpec {
        group,
        name,
        field,
        publication: VariablePublication::ResolvedTarget,
    }
}

impl EntryProfileRecord {
    pub fn environment_variable_names() -> Vec<&'static str> {
        let mut names = VARIABLE_SPECS
            .iter()
            .map(|spec| spec.name)
            .collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    pub fn environment_variable_commands() -> Vec<(&'static str, &'static str)> {
        VARIABLE_SPECS
            .iter()
            .map(|spec| (spec.group, spec.name))
            .collect()
    }

    pub fn environment_variable_values(&self) -> BTreeMap<&'static str, String> {
        let document = serde_json::to_value(self).expect("Entry Profile must serialize");
        VARIABLE_SPECS
            .iter()
            .map(|spec| {
                let value = string_field(&document, spec.field)
                    .expect("Entry Profile variable registry must reference a string field");
                (spec.name, value.to_owned())
            })
            .collect()
    }

    pub fn set_environment_variable(
        &mut self,
        name: &str,
        value: String,
    ) -> Result<(), ProfileError> {
        let spec = VARIABLE_SPECS
            .iter()
            .find(|spec| spec.name == name)
            .ok_or_else(|| {
                ProfileError::new(format!(
                    "unknown Entry Profile environment variable: {name}"
                ))
            })?;
        self.set_value(spec.field, value)
    }

    pub(crate) fn published_environment_variables(&self) -> Vec<(&'static str, String, bool)> {
        let values = self.environment_variable_values();
        VARIABLE_SPECS
            .iter()
            .filter_map(|spec| match spec.publication {
                VariablePublication::ResolvedTarget => None,
                VariablePublication::Always => Some((spec.name, values[spec.name].clone(), false)),
                VariablePublication::NonEmpty => Some((spec.name, values[spec.name].clone(), true)),
            })
            .collect()
    }

    pub(super) fn dev_setup_input_values(&self) -> BTreeMap<&'static str, String> {
        let values = self.environment_variable_values();
        VARIABLE_SPECS
            .iter()
            .filter_map(|spec| {
                let normalization = provider_input_normalization(spec.name)?;
                let value = values[spec.name].clone();
                let value = match normalization {
                    InputNormalization::Exact => value,
                    InputNormalization::Lowercase => value.to_lowercase(),
                };
                Some((spec.name, value))
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn environment_variable_fields() -> Vec<&'static str> {
        VARIABLE_SPECS.iter().map(|spec| spec.field).collect()
    }

    #[cfg(test)]
    pub(crate) fn dev_setup_input_variable_names() -> Vec<&'static str> {
        provider_input_names()
    }
}

fn string_field<'a>(document: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    let mut current = document;
    for segment in field.split('.') {
        current = current.as_object()?.get(segment)?;
    }
    current.as_str()
}
