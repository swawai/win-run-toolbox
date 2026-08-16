use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

use super::{
    filesystem::{directory_files, named_directories},
    invalid_data,
};

const WEB_VIEW_SCHEMA: &str = "swawkit.command-view/web/v4";
const MAX_RUN_OPERATIONS: usize = 8;
const MAX_OPERATION_ARGUMENTS: usize = 32;
const MAX_ARGUMENT_LENGTH: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandView {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children_column: Option<ChildrenColumnView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<RunView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChildrenColumnView {
    pub width: ColumnWidth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColumnWidth {
    Normal,
    Wide,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunView {
    pub operations: Vec<RunOperationView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunOperationView {
    pub id: String,
    pub label: String,
    pub arguments: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WebViewManifest {
    schema: String,
    children_column: Option<ChildrenColumnView>,
    run: Option<RunView>,
}

fn validate_run(run: &RunView) -> io::Result<()> {
    if run.operations.is_empty() || run.operations.len() > MAX_RUN_OPERATIONS {
        return invalid_data(format!(
            "Web run operations must contain between 1 and {MAX_RUN_OPERATIONS} items"
        ));
    }

    let mut identifiers = std::collections::BTreeSet::new();
    for operation in &run.operations {
        let valid_id = !operation.id.is_empty()
            && operation.id.len() <= 32
            && operation.id.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_lowercase() || (index > 0 && (byte.is_ascii_digit() || byte == b'-'))
            });
        if !valid_id || !identifiers.insert(operation.id.as_str()) {
            return invalid_data(format!(
                "Web run operation id '{}' must be unique and match [a-z][a-z0-9-]{{0,31}}",
                operation.id
            ));
        }
        if operation.label.trim() != operation.label
            || operation.label.is_empty()
            || operation.label.chars().count() > 64
        {
            return invalid_data(format!(
                "Web run operation '{}' label must be a trimmed string of 1 to 64 characters",
                operation.id
            ));
        }
        if operation.arguments.len() > MAX_OPERATION_ARGUMENTS
            || operation
                .arguments
                .iter()
                .any(|argument| argument.len() > MAX_ARGUMENT_LENGTH)
        {
            return invalid_data(format!(
                "Web run operation '{}' exceeds the argument limits",
                operation.id
            ));
        }
        if operation.confirmation.as_ref().is_some_and(|confirmation| {
            confirmation.trim() != confirmation
                || confirmation.is_empty()
                || confirmation.chars().count() > 500
        }) {
            return invalid_data(format!(
                "Web run operation '{}' confirmation must be a trimmed string of 1 to 500 characters",
                operation.id
            ));
        }
    }
    Ok(())
}

pub(super) fn read_local_web_view(command_directory: &Path) -> io::Result<Option<CommandView>> {
    let directories = named_directories(command_directory, "_view")?;
    if directories.len() > 1 {
        return invalid_data(format!(
            "view directory name collision below '{}'",
            command_directory.display()
        ));
    }
    let Some(view_directory) = directories.first() else {
        return Ok(None);
    };
    if view_directory.name != "_view" {
        return invalid_data(format!(
            "non-canonical view directory '{}'; expected '_view'",
            view_directory.name
        ));
    }
    if view_directory.reparse_point {
        return invalid_data(format!(
            "view directory cannot be a reparse point: {}",
            view_directory.path.display()
        ));
    }

    let files = directory_files(&view_directory.path)?
        .into_iter()
        .filter(|file| file.name.eq_ignore_ascii_case("web.json"))
        .collect::<Vec<_>>();
    if files.len() > 1 {
        return invalid_data(format!(
            "Web view file name collision below '{}'",
            view_directory.path.display()
        ));
    }
    let Some(view_file) = files.first() else {
        return Ok(None);
    };
    if view_file.name != "web.json" {
        return invalid_data(format!(
            "non-canonical Web view file '{}'; expected 'web.json'",
            view_file.name
        ));
    }
    if view_file.reparse_point {
        return invalid_data(format!(
            "Web view file cannot be a reparse point: {}",
            view_file.path.display()
        ));
    }

    let content = fs::read_to_string(&view_file.path)?;
    let manifest: WebViewManifest = serde_json::from_str(&content).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "invalid Web command view manifest '{}': {error}",
                view_file.path.display()
            ),
        )
    })?;
    if manifest.schema != WEB_VIEW_SCHEMA {
        return invalid_data(format!(
            "unsupported Web command view schema '{}' in '{}'",
            manifest.schema,
            view_file.path.display()
        ));
    }
    if manifest.children_column.is_none() && manifest.run.is_none() {
        return invalid_data(format!(
            "Web command view manifest '{}' must declare childrenColumn or run",
            view_file.path.display()
        ));
    }
    if let Some(run) = &manifest.run {
        validate_run(run)?;
    }
    Ok(Some(CommandView {
        children_column: manifest.children_column,
        run: manifest.run,
    }))
}
