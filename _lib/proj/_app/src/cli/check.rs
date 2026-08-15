use std::ffi::OsString;
use std::path::Path;

use swawkit_proj::{
    catalog::{CatalogSnapshot, CommandSource},
    context::EntryContext,
    module_check::{DependencyCheck, ModuleCheckDocument, PublicationCheck, inspect},
    profile::EntryProfileState,
};

use super::{CliError, write_output};

const CHECK_ADDRESS: &str = ".check";

pub(super) fn dispatch(
    snapshot: &CatalogSnapshot,
    argv: &[OsString],
    context: &EntryContext,
    data_root: &Path,
    profile_state: &EntryProfileState,
) -> Result<Option<i32>, CliError> {
    let Some(address) = argv.first().and_then(|value| value.to_str()) else {
        return Ok(None);
    };
    if address != CHECK_ADDRESS {
        return Ok(None);
    }
    require_check_command(snapshot)?;
    let (target, json) = match argv {
        [_, target] => (unicode(target, "command address")?, false),
        [_, target, format] if format == "--json" => (unicode(target, "command address")?, true),
        _ => return Err(check_usage()),
    };
    let document = inspect(context, data_root, profile_state.ready(), snapshot, target)
        .map_err(CliError::new)?;
    let output = if json {
        serde_json::to_string_pretty(&document)
            .map_err(|error| CliError::new(format!("cannot serialize module check: {error}")))?
    } else {
        render_text(&document)
    };
    write_output(&output)
        .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))?;
    Ok(Some(if document.ok { 0 } else { 1 }))
}

fn require_check_command(snapshot: &CatalogSnapshot) -> Result<(), CliError> {
    if snapshot.commands.iter().any(|command| {
        command.source == CommandSource::Kernel
            && command.address == CHECK_ADDRESS
            && command.adapter.as_deref() == Some("core")
            && command.handler.as_deref() == Some("meta.check")
            && command.runnable
    }) {
        Ok(())
    } else {
        Err(CliError::new("command not found: .check"))
    }
}

fn render_text(document: &ModuleCheckDocument) -> String {
    let mut lines = vec![
        format!("Command: {}", document.command.address),
        format!(
            "Status: {}",
            if document.ok { "ready" } else { "not ready" }
        ),
        format!("Runnable: {}", yes_no(document.command.runnable)),
        format!(
            "Adapter: {}",
            document.command.adapter.as_deref().unwrap_or("none")
        ),
    ];
    if let Some(diagnostic) = &document.command.diagnostic {
        lines.push(format!("Diagnostic: {diagnostic}"));
    }

    lines.push(String::new());
    lines.push("Guards:".to_owned());
    if document.guards.is_empty() {
        lines.push("  none".to_owned());
    } else {
        lines.extend(
            document
                .guards
                .iter()
                .map(|guard| format!("  {}: {}", guard.scope, guard.entry)),
        );
    }

    lines.push(String::new());
    lines.push("Dependencies:".to_owned());
    if document.dependencies.is_empty() {
        lines.push("  none declared".to_owned());
    } else {
        for dependency in &document.dependencies {
            append_dependency(&mut lines, dependency, 1);
        }
    }

    lines.push(String::new());
    lines.push("Publications:".to_owned());
    if document.publications.is_empty() {
        lines.push("  none declared".to_owned());
    } else {
        for publication in &document.publications {
            append_publication(&mut lines, publication, 1);
        }
    }
    lines.join("\n")
}

fn append_dependency(lines: &mut Vec<String>, dependency: &DependencyCheck, depth: usize) {
    let indent = "  ".repeat(depth);
    lines.push(format!(
        "{indent}{} {} [{}]",
        marker(dependency.ready),
        dependency.provider,
        dependency.contract
    ));
    if let Some(message) = &dependency.message {
        lines.push(format!("{indent}  {message}"));
    }
    if let Some(publication) = &dependency.publication {
        if let Some(root) = &publication.export_root {
            lines.push(format!("{indent}  export: {root}"));
        }
    }
    for child in &dependency.dependencies {
        append_dependency(lines, child, depth + 1);
    }
}

fn append_publication(lines: &mut Vec<String>, publication: &PublicationCheck, depth: usize) {
    let indent = "  ".repeat(depth);
    lines.push(format!(
        "{indent}{} {} [{}]",
        marker(publication.ready),
        publication.provider,
        publication.contract
    ));
    if let Some(message) = &publication.message {
        lines.push(format!("{indent}  {message}"));
    }
    if let Some(root) = &publication.export_root {
        lines.push(format!("{indent}  export: {root}"));
    }
    for item in &publication.exports {
        lines.push(format!("{indent}  - {} ({})", item.name, item.kind));
    }
    if publication.exports_truncated {
        lines.push(format!("{indent}  - ... additional items omitted"));
    }
}

fn marker(ready: bool) -> &'static str {
    if ready { "[READY]" } else { "[BLOCKED]" }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn unicode<'a>(value: &'a OsString, label: &str) -> Result<&'a str, CliError> {
    value
        .to_str()
        .ok_or_else(|| CliError::new(format!("{label} is not valid Unicode")))
}

fn check_usage() -> CliError {
    CliError::new("usage: .check <command-address> [--json]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use swawkit_proj::module_check::{CheckedCommand, MODULE_CHECK_PROTOCOL};

    #[test]
    fn text_report_has_stable_sections() {
        let document = ModuleCheckDocument {
            protocol: MODULE_CHECK_PROTOCOL,
            command: CheckedCommand {
                address: ".tool".to_owned(),
                source: CommandSource::Kernel,
                runnable: true,
                adapter: Some("exe".to_owned()),
                diagnostic: None,
            },
            guards: Vec::new(),
            dependencies: Vec::new(),
            publications: Vec::new(),
            ok: true,
        };
        let output = render_text(&document);
        assert!(output.contains("Command: .tool"));
        assert!(output.contains("Dependencies:\n  none declared"));
        assert!(output.contains("Publications:\n  none declared"));
    }
}
