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
    name: &'static str,
    field: &'static str,
    publication: VariablePublication,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SettingSpec {
    address: &'static str,
    field: &'static str,
}

const SETTING_SPECS: [SettingSpec; 18] = [
    setting(".dev.bun.mode", "development.bun.mode"),
    setting(".dev.bun.sha256", "development.bun.sha256"),
    setting(".dev.bun.version", "development.bun.version"),
    setting(".dev.cursor.mode", "development.cursor.mode"),
    setting(".dev.gh.mode", "development.gh.mode"),
    setting(".dev.msvc.channel", "development.msvc.channel"),
    setting(".dev.msvc.mode", "development.msvc.mode"),
    setting(".dev.pwsh.mode", "development.pwsh.mode"),
    setting(".dev.pwsh.sha256", "development.pwsh.sha256"),
    setting(".dev.pwsh.version", "development.pwsh.version"),
    setting(".dev.rust.mode", "development.rust.mode"),
    setting(".dev.rust.toolchain", "development.rust.toolchain"),
    setting(".dev.vscode.mode", "development.vscode.mode"),
    setting("..entry.git.access", "git.access"),
    setting("..entry.git.email", "git.email"),
    setting("..entry.git.name", "git.name"),
    setting("..entry.language", "language"),
    setting("..entry.project.root", "targetProjectRoot"),
];

const VARIABLE_SPECS: [VariableSpec; 29] = [
    variable("SWAWKIT_PROJ_BUN_MODE", "development.bun.mode"),
    optional("SWAWKIT_PROJ_BUN_SHA256", "development.bun.sha256"),
    variable("SWAWKIT_PROJ_BUN_VERSION", "development.bun.version"),
    optional("SWAWKIT_PROJ_GIT_ID_ACCESS", "git.access"),
    optional("SWAWKIT_PROJ_GIT_ID_EMAIL", "git.email"),
    optional("SWAWKIT_PROJ_GIT_ID_NAME", "git.name"),
    variable("SWAWKIT_PROJ_GO_MODE", "development.go.mode"),
    optional("SWAWKIT_PROJ_GO_SHA256", "development.go.sha256"),
    optional("SWAWKIT_PROJ_GO_VERSION", "development.go.version"),
    variable("SWAWKIT_PROJ_MSVC_CHANNEL", "development.msvc.channel"),
    variable("SWAWKIT_PROJ_MSVC_MODE", "development.msvc.mode"),
    variable("SWAWKIT_PROJ_LANGUAGE", "language"),
    resolved_target("SWAWKIT_PROJ_TARGET_PROJECT_ROOT", "targetProjectRoot"),
    variable("SWAWKIT_PROJ_PWSH_MODE", "development.pwsh.mode"),
    optional("SWAWKIT_PROJ_PWSH_SHA256", "development.pwsh.sha256"),
    variable("SWAWKIT_PROJ_PWSH_VERSION", "development.pwsh.version"),
    variable("SWAWKIT_PROJ_PYTHON_MODE", "development.python.mode"),
    optional("SWAWKIT_PROJ_PYTHON_SHA256", "development.python.sha256"),
    variable("SWAWKIT_PROJ_PYTHON_VERSION", "development.python.version"),
    variable("SWAWKIT_PROJ_RUST_HOST", "development.rust.host"),
    variable("SWAWKIT_PROJ_RUST_MODE", "development.rust.mode"),
    variable("SWAWKIT_PROJ_RUST_PROFILE", "development.rust.profile"),
    variable("SWAWKIT_PROJ_RUST_TOOLCHAIN", "development.rust.toolchain"),
    variable("SWAWKIT_PROJ_CURSOR_MODE", "development.cursor.mode"),
    variable("SWAWKIT_PROJ_GH_MODE", "development.gh.mode"),
    variable("SWAWKIT_PROJ_VSCODE_MODE", "development.vscode.mode"),
    variable("SWAWKIT_PROJ_UV_MODE", "development.uv.mode"),
    optional("SWAWKIT_PROJ_UV_SHA256", "development.uv.sha256"),
    variable("SWAWKIT_PROJ_UV_VERSION", "development.uv.version"),
];

const fn setting(address: &'static str, field: &'static str) -> SettingSpec {
    SettingSpec { address, field }
}

const fn variable(name: &'static str, field: &'static str) -> VariableSpec {
    VariableSpec {
        name,
        field,
        publication: VariablePublication::Always,
    }
}

const fn optional(name: &'static str, field: &'static str) -> VariableSpec {
    VariableSpec {
        name,
        field,
        publication: VariablePublication::NonEmpty,
    }
}

const fn resolved_target(name: &'static str, field: &'static str) -> VariableSpec {
    VariableSpec {
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

    pub fn profile_setting_addresses() -> Vec<&'static str> {
        SETTING_SPECS.iter().map(|spec| spec.address).collect()
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

    pub fn profile_setting_values(&self) -> BTreeMap<&'static str, String> {
        let document = serde_json::to_value(self).expect("Entry Profile must serialize");
        SETTING_SPECS
            .iter()
            .map(|spec| {
                let value = string_field(&document, spec.field)
                    .expect("Entry Profile setting registry must reference a string field");
                (spec.address, value.to_owned())
            })
            .collect()
    }

    pub fn is_profile_setting_address(address: &str) -> bool {
        SETTING_SPECS.iter().any(|spec| spec.address == address)
    }

    pub fn set_profile_setting(
        &mut self,
        address: &str,
        value: String,
    ) -> Result<(), ProfileError> {
        let spec = SETTING_SPECS
            .iter()
            .find(|spec| spec.address == address)
            .ok_or_else(|| {
                ProfileError::new(format!("unknown Entry Profile setting: {address}"))
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
    pub(crate) fn profile_setting_fields() -> Vec<&'static str> {
        SETTING_SPECS.iter().map(|spec| spec.field).collect()
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
