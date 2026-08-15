use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{filesystem::directory_files, invalid_data};

pub const MODULE_CONTRACT_PROTOCOL: &str = "swawkit.command-module/v1";
const MODULE_CONTRACT_FILE: &str = "_module.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandModuleContract {
    pub schema: String,
    pub requires: Vec<ModuleRequirement>,
    pub provides: Vec<ModuleProvision>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleRequirement {
    pub provider: String,
    pub contract: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleProvision {
    pub contract: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModuleManifest {
    schema: String,
    #[serde(default)]
    requires: Vec<ModuleRequirement>,
    #[serde(default)]
    provides: Vec<ModuleProvision>,
}

pub(super) fn read_local_module_contract(
    command_directory: &Path,
) -> io::Result<Option<CommandModuleContract>> {
    let files = directory_files(command_directory)?;
    let matches = files
        .iter()
        .filter(|file| file.name.eq_ignore_ascii_case(MODULE_CONTRACT_FILE))
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return invalid_data(format!(
            "module contract file name collision below '{}': {}",
            command_directory.display(),
            matches
                .iter()
                .map(|file| file.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let Some(file) = matches.first() else {
        return Ok(None);
    };
    if file.name != MODULE_CONTRACT_FILE {
        return invalid_data(format!(
            "non-canonical module contract file '{}'; expected '{MODULE_CONTRACT_FILE}'",
            file.name
        ));
    }
    if file.reparse_point {
        return invalid_data(format!(
            "module contract file cannot be a reparse point: {}",
            file.path.display()
        ));
    }

    let content = fs::read_to_string(&file.path)?;
    let manifest: ModuleManifest = serde_json::from_str(&content).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "invalid module contract manifest '{}': {error}",
                file.path.display()
            ),
        )
    })?;
    validate_manifest(&manifest, &file.path)?;
    Ok(Some(CommandModuleContract {
        schema: manifest.schema,
        requires: manifest.requires,
        provides: manifest.provides,
    }))
}

fn validate_manifest(manifest: &ModuleManifest, path: &Path) -> io::Result<()> {
    if manifest.schema != MODULE_CONTRACT_PROTOCOL {
        return invalid_data(format!(
            "unsupported module contract schema '{}' in '{}'",
            manifest.schema,
            path.display()
        ));
    }
    if manifest.requires.is_empty() && manifest.provides.is_empty() {
        return invalid_data(format!(
            "module contract manifest must declare requires or provides: {}",
            path.display()
        ));
    }

    let mut requirements = BTreeSet::new();
    for requirement in &manifest.requires {
        if !valid_provider_address(&requirement.provider) {
            return invalid_data(format!(
                "invalid module provider address '{}' in '{}'",
                requirement.provider,
                path.display()
            ));
        }
        validate_contract(&requirement.contract, path)?;
        if !requirements.insert((&requirement.provider, &requirement.contract)) {
            return invalid_data(format!(
                "duplicate module requirement '{} -> {}' in '{}'",
                requirement.provider,
                requirement.contract,
                path.display()
            ));
        }
    }

    let mut provisions = BTreeSet::new();
    for provision in &manifest.provides {
        validate_contract(&provision.contract, path)?;
        if !provisions.insert(&provision.contract) {
            return invalid_data(format!(
                "duplicate module provision '{}' in '{}'",
                provision.contract,
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_contract(contract: &str, path: &Path) -> io::Result<()> {
    let valid = (1..=128).contains(&contract.len())
        && contract
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && contract.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'/' | b'-')
        });
    if valid {
        Ok(())
    } else {
        invalid_data(format!(
            "invalid module producer contract '{contract}' in '{}'",
            path.display()
        ))
    }
}

fn valid_provider_address(address: &str) -> bool {
    let value = match address.strip_prefix('.') {
        Some(value) if !value.starts_with('.') => value,
        Some(_) => return false,
        None => address,
    };
    !value.is_empty() && value.split('.').all(valid_segment)
}

fn valid_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_addresses_are_intentionally_narrow() {
        for valid in [".dev.setup", ".dev.rust.setup", "proj.build.app"] {
            assert!(valid_provider_address(valid), "{valid}");
        }
        for invalid in ["", ".", "..entry", "Dev.setup", ".dev..setup"] {
            assert!(!valid_provider_address(invalid), "{invalid}");
        }
    }
}
