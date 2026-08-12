use std::collections::BTreeMap;
use std::fmt;

use crate::development::ArchiveToolContract;
use crate::development::archive_tool::ArchiveToolRequest;
use crate::development::msvc::MsvcDefinition;
use crate::development::rust::RustDefinition;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputNormalization {
    Exact,
    Lowercase,
}

#[derive(Clone, Copy)]
enum SnapshotNormalization {
    Literal,
    Hash,
}

#[derive(Clone, Copy)]
struct Setting {
    name: &'static str,
    snapshot: SnapshotNormalization,
    input: Option<InputNormalization>,
}

#[derive(Clone, Copy)]
struct Module {
    name: &'static str,
    mode: &'static str,
    setup_implemented: bool,
    settings: &'static [Setting],
}

const BUN_SETTINGS: &[Setting] = &[
    input(
        "SWAWKIT_PROJ_BUN_SHA256",
        SnapshotNormalization::Hash,
        InputNormalization::Lowercase,
    ),
    input(
        "SWAWKIT_PROJ_BUN_VERSION",
        SnapshotNormalization::Literal,
        InputNormalization::Exact,
    ),
];
const GO_SETTINGS: &[Setting] = &[literal("SWAWKIT_PROJ_GO_VERSION")];
const MSVC_SETTINGS: &[Setting] = &[input(
    "SWAWKIT_PROJ_MSVC_CHANNEL",
    SnapshotNormalization::Literal,
    InputNormalization::Exact,
)];
const PWSH_SETTINGS: &[Setting] = &[
    input(
        "SWAWKIT_PROJ_PWSH_SHA256",
        SnapshotNormalization::Hash,
        InputNormalization::Lowercase,
    ),
    input(
        "SWAWKIT_PROJ_PWSH_VERSION",
        SnapshotNormalization::Literal,
        InputNormalization::Exact,
    ),
];
const PYTHON_SETTINGS: &[Setting] = &[literal("SWAWKIT_PROJ_PYTHON_VERSION")];
const RUST_SETTINGS: &[Setting] = &[
    input(
        "SWAWKIT_PROJ_RUST_HOST",
        SnapshotNormalization::Literal,
        InputNormalization::Exact,
    ),
    input(
        "SWAWKIT_PROJ_RUST_PROFILE",
        SnapshotNormalization::Literal,
        InputNormalization::Exact,
    ),
    input(
        "SWAWKIT_PROJ_RUST_TOOLCHAIN",
        SnapshotNormalization::Literal,
        InputNormalization::Lowercase,
    ),
];
const UV_SETTINGS: &[Setting] = &[literal("SWAWKIT_PROJ_UV_VERSION")];

const MODULES: &[Module] = &[
    module("bun", "SWAWKIT_PROJ_BUN_MODE", true, BUN_SETTINGS),
    module("go", "SWAWKIT_PROJ_GO_MODE", false, GO_SETTINGS),
    module("msvc", "SWAWKIT_PROJ_MSVC_MODE", true, MSVC_SETTINGS),
    module("pwsh", "SWAWKIT_PROJ_PWSH_MODE", true, PWSH_SETTINGS),
    module("python", "SWAWKIT_PROJ_PYTHON_MODE", false, PYTHON_SETTINGS),
    module("rust", "SWAWKIT_PROJ_RUST_MODE", true, RUST_SETTINGS),
    module("uv", "SWAWKIT_PROJ_UV_MODE", false, UV_SETTINGS),
];

const fn module(
    name: &'static str,
    mode: &'static str,
    setup_implemented: bool,
    settings: &'static [Setting],
) -> Module {
    Module {
        name,
        mode,
        setup_implemented,
        settings,
    }
}

const fn input(
    name: &'static str,
    snapshot: SnapshotNormalization,
    input: InputNormalization,
) -> Setting {
    Setting {
        name,
        snapshot,
        input: Some(input),
    }
}

const fn literal(name: &'static str) -> Setting {
    Setting {
        name,
        snapshot: SnapshotNormalization::Literal,
        input: None,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationSnapshot {
    values: BTreeMap<&'static str, String>,
}

impl DeclarationSnapshot {
    pub fn values(&self) -> &BTreeMap<&'static str, String> {
        &self.values
    }

    pub fn enabled_modules(&self) -> Vec<&'static str> {
        MODULES
            .iter()
            .filter(|module| self.values[module.mode] != "disabled")
            .map(|module| module.name)
            .collect()
    }

    pub fn pending_modules(&self) -> Vec<&'static str> {
        MODULES
            .iter()
            .filter(|module| !module.setup_implemented && self.values[module.mode] != "disabled")
            .map(|module| module.name)
            .collect()
    }

    pub fn require_supported(&self) -> Result<(), DeclarationError> {
        let pending = self.pending_modules();
        if pending.is_empty() {
            Ok(())
        } else {
            Err(DeclarationError(format!(
                ".dev.setup does not yet handle these enabled declarations: {}.",
                pending.join(", ")
            )))
        }
    }

    pub fn archive_request(
        &self,
        tool: &ArchiveToolContract,
    ) -> Result<Option<ArchiveToolRequest>, DeclarationError> {
        let mode = self.values.get(tool.mode_variable).ok_or_else(|| {
            DeclarationError(format!(
                "archive tool '{}' is absent from the setup declaration registry",
                tool.name
            ))
        })?;
        if mode == "disabled" {
            return Ok(None);
        }
        if mode != "managed" {
            return Err(DeclarationError(format!(
                "unsupported {} value '{}'; expected 'managed' or 'disabled'",
                tool.mode_variable, mode
            )));
        }
        let version = self.values.get(tool.version_variable).ok_or_else(|| {
            DeclarationError(format!(
                "enabled {} must declare {}",
                tool.display_name, tool.version_variable
            ))
        })?;
        let project_sha256 = self.values.get(tool.hash_variable).ok_or_else(|| {
            DeclarationError(format!(
                "enabled {} must declare {} as an empty or pinned value",
                tool.display_name, tool.hash_variable
            ))
        })?;
        ArchiveToolRequest::new(tool, version, project_sha256)
            .map(Some)
            .map_err(|error| DeclarationError(error.to_string()))
    }

    pub fn msvc_definition(&self) -> Result<Option<MsvcDefinition>, DeclarationError> {
        let mode = self
            .values
            .get("SWAWKIT_PROJ_MSVC_MODE")
            .ok_or_else(|| DeclarationError("MSVC is absent from the setup registry".to_owned()))?;
        if mode == "disabled" {
            return Ok(None);
        }
        if mode != "managed" {
            return Err(DeclarationError(format!(
                "unsupported SWAWKIT_PROJ_MSVC_MODE value '{mode}'; expected 'managed' or 'disabled'"
            )));
        }
        let channel = self
            .values
            .get("SWAWKIT_PROJ_MSVC_CHANNEL")
            .ok_or_else(|| DeclarationError("enabled MSVC must declare its channel".to_owned()))?;
        MsvcDefinition::new(channel)
            .map(Some)
            .map_err(|error| DeclarationError(error.to_string()))
    }

    pub fn rust_definition(&self) -> Result<Option<RustDefinition>, DeclarationError> {
        let mode = self.values.get("SWAWKIT_PROJ_RUST_MODE").ok_or_else(|| {
            DeclarationError("Rust is absent from the setup declaration registry".to_owned())
        })?;
        if mode == "disabled" {
            return Ok(None);
        }
        if mode != "rustup" {
            return Err(DeclarationError(format!(
                "unsupported SWAWKIT_PROJ_RUST_MODE value '{mode}'; expected 'rustup' or 'disabled'"
            )));
        }
        RustDefinition::new(
            self.values
                .get("SWAWKIT_PROJ_RUST_TOOLCHAIN")
                .ok_or_else(|| {
                    DeclarationError("enabled Rust must declare a toolchain".to_owned())
                })?,
            self.values
                .get("SWAWKIT_PROJ_RUST_PROFILE")
                .ok_or_else(|| {
                    DeclarationError("enabled Rust must declare a profile".to_owned())
                })?,
            self.values
                .get("SWAWKIT_PROJ_RUST_HOST")
                .ok_or_else(|| DeclarationError("enabled Rust must declare a host".to_owned()))?,
        )
        .map(Some)
        .map_err(|error| DeclarationError(error.to_string()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationError(String);

impl fmt::Display for DeclarationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DeclarationError {}

pub fn snapshot_from_environment() -> DeclarationSnapshot {
    snapshot(|name| std::env::var(name).ok())
}

pub fn snapshot(mut get: impl FnMut(&str) -> Option<String>) -> DeclarationSnapshot {
    let mut values = BTreeMap::new();
    for module in MODULES {
        let mode = get(module.mode)
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let mode = if mode.is_empty() {
            "disabled".to_owned()
        } else {
            mode
        };
        values.insert(module.mode, mode.clone());
        if mode == "disabled" {
            continue;
        }
        for setting in module.settings {
            let mut value = get(setting.name).unwrap_or_default().trim().to_owned();
            if matches!(setting.snapshot, SnapshotNormalization::Hash) {
                value.make_ascii_lowercase();
                if let Some(digest) = value.strip_prefix("sha256:") {
                    value = digest.to_owned();
                }
            }
            values.insert(setting.name, value);
        }
    }
    DeclarationSnapshot { values }
}

pub fn provider_input_normalization(name: &str) -> Option<InputNormalization> {
    MODULES.iter().find_map(|module| {
        if module.setup_implemented && module.mode == name {
            return Some(InputNormalization::Exact);
        }
        module
            .settings
            .iter()
            .find_map(|setting| (setting.name == name).then_some(setting.input).flatten())
    })
}

pub fn provider_input_names() -> Vec<&'static str> {
    let mut names = MODULES
        .iter()
        .flat_map(|module| {
            module
                .setup_implemented
                .then_some(module.mode)
                .into_iter()
                .chain(
                    module
                        .settings
                        .iter()
                        .filter(|setting| setting.input.is_some())
                        .map(|setting| setting.name),
                )
        })
        .collect::<Vec<_>>();
    names.sort_unstable();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_matches_the_manifest_declaration_contract() {
        let values = BTreeMap::from([
            ("SWAWKIT_PROJ_BUN_MODE", " MANAGED "),
            ("SWAWKIT_PROJ_BUN_VERSION", "1.2.15"),
            ("SWAWKIT_PROJ_BUN_SHA256", " SHA256:AAAA "),
            ("SWAWKIT_PROJ_GO_MODE", "managed"),
            ("SWAWKIT_PROJ_GO_VERSION", "1.25"),
        ]);
        let snapshot = snapshot(|name| values.get(name).map(|value| (*value).to_owned()));

        assert_eq!(snapshot.values()["SWAWKIT_PROJ_BUN_MODE"], "managed");
        assert_eq!(snapshot.values()["SWAWKIT_PROJ_BUN_SHA256"], "aaaa");
        assert_eq!(snapshot.pending_modules(), ["go"]);
        assert_eq!(
            snapshot.require_supported().unwrap_err().to_string(),
            ".dev.setup does not yet handle these enabled declarations: go."
        );
    }

    #[test]
    fn provider_inputs_have_one_typed_registry() {
        assert_eq!(provider_input_names().len(), 12);
        assert_eq!(
            provider_input_normalization("SWAWKIT_PROJ_RUST_TOOLCHAIN"),
            Some(InputNormalization::Lowercase)
        );
        assert_eq!(provider_input_normalization("SWAWKIT_PROJ_GO_MODE"), None);
    }

    #[test]
    fn archive_requests_are_typed() {
        let values = BTreeMap::from([
            ("SWAWKIT_PROJ_BUN_MODE", "managed"),
            ("SWAWKIT_PROJ_BUN_VERSION", "1.2.15"),
            ("SWAWKIT_PROJ_BUN_SHA256", ""),
        ]);
        let snapshot = snapshot(|name| values.get(name).map(|value| (*value).to_owned()));
        assert_eq!(
            snapshot
                .archive_request(&crate::development::BUN)
                .unwrap()
                .unwrap()
                .requested(),
            "1.2.15"
        );
    }

    #[test]
    fn msvc_declarations_are_typed() {
        let values = BTreeMap::from([
            ("SWAWKIT_PROJ_MSVC_MODE", "managed"),
            ("SWAWKIT_PROJ_MSVC_CHANNEL", "17"),
        ]);
        let snapshot = snapshot(|name| values.get(name).map(|value| (*value).to_owned()));

        assert_eq!(snapshot.msvc_definition().unwrap().unwrap().channel(), "17");
    }

    #[test]
    fn rust_declarations_share_the_domain_definition() {
        let values = BTreeMap::from([
            ("SWAWKIT_PROJ_RUST_MODE", "rustup"),
            ("SWAWKIT_PROJ_RUST_TOOLCHAIN", "stable"),
            ("SWAWKIT_PROJ_RUST_PROFILE", "minimal"),
            ("SWAWKIT_PROJ_RUST_HOST", "x86_64-pc-windows-msvc"),
        ]);
        let snapshot = snapshot(|name| values.get(name).map(|value| (*value).to_owned()));

        let definition = snapshot.rust_definition().unwrap().unwrap();

        assert_eq!(definition.toolchain(), "stable");
        assert_eq!(definition.toolchain_name(), "stable-x86_64-pc-windows-msvc");
    }
}
