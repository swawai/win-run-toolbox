use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use serde_json::Value;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::catalog::{CommandNode, ModuleProvision};
use crate::command::catalog_command_data_root;
use crate::context::EntryContext;
use crate::profile::EntryProfile;

use super::{ExportItem, PublicationCheck};

const PROVIDER_STATE_SCHEMA: &str = "swawkit.command-provider-state/v1";
const MAX_PROVIDER_STATE_BYTES: u64 = 64 * 1024;
const MAX_EXPORT_ITEMS: usize = 64;
const REVISION_PREFIX: &str = "sha256-";

pub(super) fn inspect_publication(
    context: &EntryContext,
    data_root: &Path,
    profile: Option<&EntryProfile>,
    provider: &CommandNode,
    provision: &ModuleProvision,
) -> PublicationCheck {
    let module_root = match catalog_command_data_root(
        context,
        data_root,
        profile.map(|profile| profile.binding()),
        provider,
    ) {
        Ok(path) => path,
        Err(error) => {
            return publication_failure(
                provider,
                provision,
                "data-root-unavailable",
                error.to_string(),
            );
        }
    };
    let state_path = module_root.join("_state.json");
    let export_root = module_root.join("export");
    let (exports, exports_truncated) = match list_exports(&export_root) {
        Ok(exports) => exports,
        Err(error) => {
            return PublicationCheck {
                provider: provider.address.clone(),
                contract: provision.contract.clone(),
                ready: false,
                status: "export-invalid".to_owned(),
                message: Some(error),
                state_path: Some(display_path(&state_path)),
                export_root: Some(display_path(&export_root)),
                exports: Vec::new(),
                exports_truncated: false,
            };
        }
    };
    let state = match read_provider_state(&state_path) {
        Ok(state) => state,
        Err(error) => {
            return PublicationCheck {
                provider: provider.address.clone(),
                contract: provision.contract.clone(),
                ready: false,
                status: "state-invalid".to_owned(),
                message: Some(error),
                state_path: Some(display_path(&state_path)),
                export_root: Some(display_path(&export_root)),
                exports,
                exports_truncated,
            };
        }
    };
    let Some(state) = state else {
        return PublicationCheck {
            provider: provider.address.clone(),
            contract: provision.contract.clone(),
            ready: false,
            status: "state-missing".to_owned(),
            message: Some(format!("run '{} {}'", context.entry_name, provider.address)),
            state_path: Some(display_path(&state_path)),
            export_root: Some(display_path(&export_root)),
            exports,
            exports_truncated,
        };
    };
    let status = state
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("invalid");
    let contract = state.get("producerContract").and_then(Value::as_str);
    let export_ready = regular_directory(&export_root);
    let ready = status == "ready" && contract == Some(provision.contract.as_str()) && export_ready;
    let message = if status != "ready" {
        Some(format!(
            "provider state is {status}; run '{} {}'",
            context.entry_name, provider.address
        ))
    } else if contract != Some(provision.contract.as_str()) {
        Some("provider state contract does not match the declaration".to_owned())
    } else if !export_ready {
        Some("provider export directory is missing or unsafe".to_owned())
    } else {
        None
    };
    PublicationCheck {
        provider: provider.address.clone(),
        contract: provision.contract.clone(),
        ready,
        status: if ready { "ready" } else { "not-ready" }.to_owned(),
        message,
        state_path: Some(display_path(&state_path)),
        export_root: Some(display_path(&export_root)),
        exports,
        exports_truncated,
    }
}

fn publication_failure(
    provider: &CommandNode,
    provision: &ModuleProvision,
    status: &str,
    message: String,
) -> PublicationCheck {
    PublicationCheck {
        provider: provider.address.clone(),
        contract: provision.contract.clone(),
        ready: false,
        status: status.to_owned(),
        message: Some(message),
        state_path: None,
        export_root: None,
        exports: Vec::new(),
        exports_truncated: false,
    }
}

fn read_provider_state(path: &Path) -> Result<Option<Value>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot inspect provider state '{}': {error}",
                path.display()
            ));
        }
    };
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() > MAX_PROVIDER_STATE_BYTES
    {
        return Err(format!(
            "provider state is not a bounded regular file: {}",
            path.display()
        ));
    }
    let content = fs::read(path)
        .map_err(|error| format!("cannot read provider state '{}': {error}", path.display()))?;
    let value: Value = serde_json::from_slice(&content)
        .map_err(|error| format!("cannot parse provider state '{}': {error}", path.display()))?;
    validate_provider_state(&value)
        .map_err(|error| format!("provider state '{}' is invalid: {error}", path.display()))?;
    Ok(Some(value))
}

fn validate_provider_state(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "expected a JSON object".to_owned())?;
    let status = object
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "status is invalid".to_owned())?;
    let expected: &[&str] = match status {
        "unavailable" => &["schema", "status", "inputRevision", "token"],
        "ready" => &[
            "schema",
            "status",
            "inputRevision",
            "token",
            "producerContract",
        ],
        _ => return Err("status is invalid".to_owned()),
    };
    if object.len() != expected.len()
        || expected
            .iter()
            .any(|name| !object.get(*name).is_some_and(Value::is_string))
    {
        return Err("shape is invalid".to_owned());
    }
    if object["schema"] != PROVIDER_STATE_SCHEMA {
        return Err("schema is invalid".to_owned());
    }
    if !valid_revision(object["inputRevision"].as_str().unwrap_or_default()) {
        return Err("input revision is invalid".to_owned());
    }
    if !is_lower_hex(object["token"].as_str().unwrap_or_default(), 32) {
        return Err("publication token is invalid".to_owned());
    }
    if status == "ready"
        && !object["producerContract"]
            .as_str()
            .is_some_and(valid_contract)
    {
        return Err("producer contract is invalid".to_owned());
    }
    Ok(())
}

fn valid_revision(value: &str) -> bool {
    value.len() == REVISION_PREFIX.len() + 64
        && value.starts_with(REVISION_PREFIX)
        && is_lower_hex(&value[REVISION_PREFIX.len()..], 64)
}

fn valid_contract(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && (value.as_bytes()[0].is_ascii_lowercase() || value.as_bytes()[0].is_ascii_digit())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'/' | b'-')
        })
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn list_exports(root: &Path) -> Result<(Vec<ExportItem>, bool), String> {
    if !regular_directory(root) {
        return Ok((Vec::new(), false));
    }
    let mut entries = fs::read_dir(root)
        .map_err(|error| format!("cannot inspect export '{}': {error}", root.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot inspect export '{}': {error}", root.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    let truncated = entries.len() > MAX_EXPORT_ITEMS;
    entries.truncate(MAX_EXPORT_ITEMS);
    let mut exports = Vec::with_capacity(entries.len());
    for entry in entries {
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            format!(
                "cannot inspect export item '{}': {error}",
                entry.path().display()
            )
        })?;
        let kind = if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            "reparse"
        } else if metadata.is_dir() {
            "directory"
        } else if metadata.is_file() {
            "file"
        } else {
            "other"
        };
        exports.push(ExportItem {
            name: entry.file_name().to_string_lossy().into_owned(),
            kind,
        });
    }
    Ok((exports, truncated))
}

fn regular_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
    })
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::validate_provider_state;

    fn revision() -> String {
        format!("sha256-{}", "a".repeat(64))
    }

    #[test]
    fn accepts_exact_unavailable_and_ready_states() {
        let unavailable = json!({
            "schema": "swawkit.command-provider-state/v1",
            "status": "unavailable",
            "inputRevision": revision(),
            "token": "1".repeat(32),
        });
        let ready = json!({
            "schema": "swawkit.command-provider-state/v1",
            "status": "ready",
            "inputRevision": revision(),
            "token": "2".repeat(32),
            "producerContract": "swawkit.dev-environment/v1",
        });

        assert_eq!(validate_provider_state(&unavailable), Ok(()));
        assert_eq!(validate_provider_state(&ready), Ok(()));
    }

    #[test]
    fn rejects_incomplete_or_extended_provider_states() {
        let missing_token = json!({
            "schema": "swawkit.command-provider-state/v1",
            "status": "ready",
            "inputRevision": revision(),
            "producerContract": "swawkit.dev-environment/v1",
        });
        let extended = json!({
            "schema": "swawkit.command-provider-state/v1",
            "status": "unavailable",
            "inputRevision": revision(),
            "token": "3".repeat(32),
            "reason": "stale",
        });

        assert_eq!(
            validate_provider_state(&missing_token),
            Err("shape is invalid".to_owned())
        );
        assert_eq!(
            validate_provider_state(&extended),
            Err("shape is invalid".to_owned())
        );
    }

    #[test]
    fn rejects_invalid_revision_token_status_and_contract() {
        let invalid = [
            json!({
                "schema": "swawkit.command-provider-state/v1",
                "status": "ready",
                "inputRevision": format!("sha256-{}", "A".repeat(64)),
                "token": "4".repeat(32),
                "producerContract": "swawkit.dev-environment/v1",
            }),
            json!({
                "schema": "swawkit.command-provider-state/v1",
                "status": "ready",
                "inputRevision": revision(),
                "token": "g".repeat(32),
                "producerContract": "swawkit.dev-environment/v1",
            }),
            json!({
                "schema": "swawkit.command-provider-state/v1",
                "status": "stale",
                "inputRevision": revision(),
                "token": "5".repeat(32),
            }),
            json!({
                "schema": "swawkit.command-provider-state/v1",
                "status": "ready",
                "inputRevision": revision(),
                "token": "6".repeat(32),
                "producerContract": "Invalid Contract",
            }),
        ];

        for value in invalid {
            assert!(validate_provider_state(&value).is_err());
        }
    }
}
