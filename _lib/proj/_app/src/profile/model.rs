use serde::{Deserialize, Serialize};

use super::{PROFILE_SCHEMA, ProfileError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryProfileRecord {
    pub schema: String,
    pub target_project_root: String,
    pub preferences: Preferences,
    pub development: DevelopmentProfile,
    pub git: GitProfile,
    pub repository: RepositoryProfile,
}

impl EntryProfileRecord {
    #[cfg(test)]
    pub(crate) fn mutable_string_field_paths() -> Vec<String> {
        let document = serde_json::to_value(Self::default())
            .expect("the default Entry Profile must serialize");
        let mut fields = Vec::new();
        collect_string_field_paths(&document, "", &mut fields);
        fields.retain(|field| field != "schema");
        fields.sort_unstable();
        fields
    }

    pub fn validate(&self) -> Result<(), ProfileError> {
        if self.schema != PROFILE_SCHEMA {
            return Err(ProfileError::new(format!(
                "unsupported entry profile schema '{}'",
                self.schema
            )));
        }
        require_trimmed("targetProjectRoot", &self.target_project_root)?;
        require_trimmed("preferences.defaultShell", &self.preferences.default_shell)?;
        require_trimmed("preferences.defaultIde", &self.preferences.default_ide)?;
        optional_trimmed("preferences.helpLanguage", &self.preferences.help_language)?;

        validate_versioned_tool("development.bun", &self.development.bun, "managed")?;
        validate_pwsh(&self.development.pwsh)?;
        validate_channel_tool("development.msvc", &self.development.msvc)?;
        validate_rust(&self.development.rust)?;
        validate_declared_only_tool("development.uv", &self.development.uv)?;
        validate_declared_only_tool("development.python", &self.development.python)?;
        validate_declared_only_tool("development.go", &self.development.go)?;
        validate_system_tool("development.gh", &self.development.gh)?;
        validate_system_tool("development.vscode", &self.development.vscode)?;
        validate_system_tool("development.cursor", &self.development.cursor)?;

        optional_trimmed("git.name", &self.git.name)?;
        optional_trimmed("git.email", &self.git.email)?;
        optional_trimmed("git.access", &self.git.access)?;
        optional_trimmed("repository.remote", &self.repository.remote)?;
        Ok(())
    }

    pub(crate) fn set_value(&mut self, field: &str, value: String) -> Result<(), ProfileError> {
        if field == "schema" || field.is_empty() {
            return Err(ProfileError::new(format!(
                "entry profile field cannot be changed: {field}"
            )));
        }
        let mut document = serde_json::to_value(&*self)
            .map_err(|error| ProfileError::new(format!("cannot inspect entry profile: {error}")))?;
        let mut segments = field.split('.').peekable();
        let mut current = &mut document;
        while let Some(segment) = segments.next() {
            let Some(object) = current.as_object_mut() else {
                return Err(ProfileError::new(format!(
                    "entry profile field is not a string leaf: {field}"
                )));
            };
            let Some(next) = object.get_mut(segment) else {
                return Err(ProfileError::new(format!(
                    "unknown entry profile field: {field}"
                )));
            };
            if segments.peek().is_none() {
                if !next.is_string() {
                    return Err(ProfileError::new(format!(
                        "entry profile field is not a string leaf: {field}"
                    )));
                }
                *next = serde_json::Value::String(value);
                break;
            }
            current = next;
        }
        *self = serde_json::from_value(document).map_err(|error| {
            ProfileError::new(format!("cannot update entry profile field: {error}"))
        })?;
        Ok(())
    }
}

#[cfg(test)]
fn collect_string_field_paths(value: &serde_json::Value, prefix: &str, fields: &mut Vec<String>) {
    match value {
        serde_json::Value::String(_) if !prefix.is_empty() => fields.push(prefix.to_owned()),
        serde_json::Value::Object(object) => {
            for (name, child) in object {
                let path = if prefix.is_empty() {
                    name.to_owned()
                } else {
                    format!("{prefix}.{name}")
                };
                collect_string_field_paths(child, &path, fields);
            }
        }
        _ => {}
    }
}

impl Default for EntryProfileRecord {
    fn default() -> Self {
        Self {
            schema: PROFILE_SCHEMA.to_owned(),
            target_project_root: "${SWAWKIT_HOME}".to_owned(),
            preferences: Preferences::default(),
            development: DevelopmentProfile::default(),
            git: GitProfile::default(),
            repository: RepositoryProfile::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub default_shell: String,
    pub default_ide: String,
    pub help_language: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            default_shell: "pwsh".to_owned(),
            default_ide: "code".to_owned(),
            help_language: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentProfile {
    pub bun: VersionedTool,
    pub pwsh: VersionedTool,
    pub msvc: ChannelTool,
    pub rust: RustTool,
    pub uv: VersionedTool,
    pub python: VersionedTool,
    pub go: VersionedTool,
    pub gh: ModeTool,
    pub vscode: ModeTool,
    pub cursor: ModeTool,
}

impl Default for DevelopmentProfile {
    fn default() -> Self {
        Self {
            bun: VersionedTool::managed("1.2.15"),
            pwsh: VersionedTool::managed("latest"),
            msvc: ChannelTool {
                mode: "managed".to_owned(),
                channel: "17".to_owned(),
            },
            rust: RustTool {
                mode: "rustup".to_owned(),
                toolchain: "stable".to_owned(),
                profile: "minimal".to_owned(),
                host: "x86_64-pc-windows-msvc".to_owned(),
            },
            uv: VersionedTool::disabled("0.10.2"),
            python: VersionedTool::disabled("3.13"),
            go: VersionedTool::disabled(""),
            gh: ModeTool::system(),
            vscode: ModeTool::system(),
            cursor: ModeTool::system(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionedTool {
    pub mode: String,
    pub version: String,
    pub sha256: String,
}

impl VersionedTool {
    fn managed(version: &str) -> Self {
        Self {
            mode: "managed".to_owned(),
            version: version.to_owned(),
            sha256: String::new(),
        }
    }

    fn disabled(version: &str) -> Self {
        Self {
            mode: "disabled".to_owned(),
            version: version.to_owned(),
            sha256: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelTool {
    pub mode: String,
    pub channel: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustTool {
    pub mode: String,
    pub toolchain: String,
    pub profile: String,
    pub host: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeTool {
    pub mode: String,
}

impl ModeTool {
    fn system() -> Self {
        Self {
            mode: "system".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitProfile {
    pub name: String,
    pub email: String,
    pub access: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryProfile {
    pub remote: String,
}

fn validate_versioned_tool(
    path: &str,
    tool: &VersionedTool,
    install_mode: &str,
) -> Result<(), ProfileError> {
    allowed_mode(path, &tool.mode, &[install_mode, "disabled"])?;
    if tool.mode == install_mode {
        require_trimmed(&format!("{path}.version"), &tool.version)?;
    } else {
        optional_trimmed(&format!("{path}.version"), &tool.version)?;
    }
    validate_sha256(&format!("{path}.sha256"), &tool.sha256)
}

fn validate_pwsh(tool: &VersionedTool) -> Result<(), ProfileError> {
    allowed_mode(
        "development.pwsh",
        &tool.mode,
        &["managed", "system", "disabled"],
    )?;
    if tool.mode == "managed" {
        require_trimmed("development.pwsh.version", &tool.version)?;
    } else {
        optional_trimmed("development.pwsh.version", &tool.version)?;
    }
    validate_sha256("development.pwsh.sha256", &tool.sha256)
}

fn validate_channel_tool(path: &str, tool: &ChannelTool) -> Result<(), ProfileError> {
    allowed_mode(path, &tool.mode, &["managed", "disabled"])?;
    if tool.mode == "managed" {
        require_trimmed(&format!("{path}.channel"), &tool.channel)
    } else {
        optional_trimmed(&format!("{path}.channel"), &tool.channel)
    }
}

fn validate_rust(tool: &RustTool) -> Result<(), ProfileError> {
    allowed_mode("development.rust", &tool.mode, &["rustup", "disabled"])?;
    if tool.mode == "rustup" {
        require_trimmed("development.rust.toolchain", &tool.toolchain)?;
    } else {
        optional_trimmed("development.rust.toolchain", &tool.toolchain)?;
    }
    if tool.profile != "minimal" {
        return Err(ProfileError::new(
            "development.rust.profile must be 'minimal' in V0",
        ));
    }
    if tool.host != "x86_64-pc-windows-msvc" {
        return Err(ProfileError::new(
            "development.rust.host must be 'x86_64-pc-windows-msvc' in V0",
        ));
    }
    Ok(())
}

fn validate_declared_only_tool(path: &str, tool: &VersionedTool) -> Result<(), ProfileError> {
    allowed_mode(path, &tool.mode, &["disabled"])?;
    optional_trimmed(&format!("{path}.version"), &tool.version)?;
    validate_sha256(&format!("{path}.sha256"), &tool.sha256)
}

fn validate_system_tool(path: &str, tool: &ModeTool) -> Result<(), ProfileError> {
    allowed_mode(path, &tool.mode, &["system", "disabled"])
}

fn allowed_mode(path: &str, actual: &str, allowed: &[&str]) -> Result<(), ProfileError> {
    if allowed.contains(&actual) {
        Ok(())
    } else {
        Err(ProfileError::new(format!(
            "{path}.mode must be one of: {}",
            allowed.join(", ")
        )))
    }
}

fn require_trimmed(path: &str, value: &str) -> Result<(), ProfileError> {
    if value.is_empty() {
        return Err(ProfileError::new(format!(
            "required property '{path}' is missing"
        )));
    }
    optional_trimmed(path, value)
}

fn optional_trimmed(path: &str, value: &str) -> Result<(), ProfileError> {
    if value.trim() != value {
        Err(ProfileError::new(format!(
            "{path} cannot have surrounding whitespace"
        )))
    } else {
        Ok(())
    }
}

fn validate_sha256(path: &str, value: &str) -> Result<(), ProfileError> {
    if value.is_empty() || (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        Ok(())
    } else {
        Err(ProfileError::new(format!(
            "{path} must be empty or contain exactly 64 hexadecimal characters"
        )))
    }
}
