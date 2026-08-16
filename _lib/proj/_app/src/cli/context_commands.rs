use std::ffi::OsString;
use std::io::{self, Read};
use std::path::Path;

use swawkit_proj::catalog::{CatalogSnapshot, CommandSource};
use swawkit_proj::command::catalog_command_data_root;
use swawkit_proj::context::EntryContext;
use swawkit_proj::context_store::{
    CONTEXT_OWNER_ADDRESS, ContextCommand, ContextRecord, ContextStore, MAX_NOTE_BYTES,
    MAX_PROMPT_BYTES, project_context_collection, render_markdown, static_child_ids, validate_text,
};
use swawkit_proj::profile::EntryLanguage;
use swawkit_proj::subject::SubjectRef;

use super::{CliError, write_output};

const CONTEXT_PREFIX: &str = "context.";

pub(super) fn dispatch(
    snapshot: &CatalogSnapshot,
    argv: &[OsString],
    entry_context: &EntryContext,
    data_root: &Path,
) -> Result<Option<i32>, CliError> {
    let Some(address) = argv.first().and_then(|value| value.to_str()) else {
        return Ok(None);
    };
    let Some(command) = snapshot.commands.iter().find(|command| {
        command.source == CommandSource::Kernel
            && command.address == address
            && command.adapter.as_deref() == Some("core")
            && command
                .handler
                .as_deref()
                .is_some_and(|handler| handler.starts_with(CONTEXT_PREFIX))
    }) else {
        return Ok(None);
    };
    if !command.runnable {
        return Err(CliError::new(format!(
            "command '{address}' is not runnable: {}",
            command
                .diagnostic
                .as_deref()
                .unwrap_or("the Context command has no recognized Core entry")
        )));
    }

    let owner = snapshot
        .commands
        .iter()
        .find(|command| {
            command.source == CommandSource::Kernel
                && command.address == CONTEXT_OWNER_ADDRESS
                && command.alias_of.is_none()
        })
        .ok_or_else(|| CliError::new("the .context command module is unavailable"))?;
    let module_data_root = catalog_command_data_root(entry_context, data_root, None, owner)
        .map_err(|error| CliError::new(error.to_string()))?;
    let store = ContextStore::new(
        data_root,
        module_data_root,
        static_child_ids(snapshot, ".context"),
    );
    let arguments = argv.get(1..).unwrap_or_default();
    let handler = command
        .handler
        .as_deref()
        .expect("a resolved Context command must have a handler");
    let exit_code = match handler {
        "context.new" => create(address, arguments, &store)?,
        "context.add" => add(address, arguments, snapshot, &store)?,
        "context.remove" => remove(address, arguments, &store)?,
        "context.note" => note(address, arguments, &store, &mut io::stdin().lock())?,
        "context.prompt" => prompt(address, arguments, &store, &mut io::stdin().lock())?,
        "context.show" => show(address, arguments, &store)?,
        "context.render" => render(address, arguments, &store)?,
        "context.list" => list(address, arguments, snapshot, &store)?,
        "context.delete" => delete(address, arguments, &store)?,
        _ => {
            return Err(CliError::new(format!(
                "unsupported Context command handler: {handler}"
            )));
        }
    };
    Ok(Some(exit_code))
}

fn create(address: &str, arguments: &[OsString], store: &ContextStore) -> Result<i32, CliError> {
    let [id] = arguments else {
        return Err(usage(address, "<context-id>"));
    };
    let id = unicode(id, "Context ID")?;
    store.create(id).map_err(store_error)?;
    write_message(&format!("Context created: {id}"))?;
    Ok(0)
}

fn add(
    address: &str,
    arguments: &[OsString],
    snapshot: &CatalogSnapshot,
    store: &ContextStore,
) -> Result<i32, CliError> {
    let Some((id, targets)) = arguments
        .split_first()
        .filter(|(_, targets)| !targets.is_empty())
    else {
        return Err(usage(address, "<context-id> <command-address>..."));
    };
    let id = unicode(id, "Context ID")?;
    let mut commands = Vec::with_capacity(targets.len());
    for target in targets {
        let target = unicode(target, "command address")?;
        let command = snapshot
            .commands
            .iter()
            .find(|command| command.address == target)
            .ok_or_else(|| CliError::new(format!("command not found: {target}")))?;
        commands.push(ContextCommand {
            source: command.source,
            address: command.address.clone(),
        });
    }
    let record = store.add_commands(id, commands).map_err(store_error)?;
    write_updated(&record)?;
    Ok(0)
}

fn remove(address: &str, arguments: &[OsString], store: &ContextStore) -> Result<i32, CliError> {
    let Some((id, targets)) = arguments
        .split_first()
        .filter(|(_, targets)| !targets.is_empty())
    else {
        return Err(usage(address, "<context-id> <command-address>..."));
    };
    let id = unicode(id, "Context ID")?;
    let targets = targets
        .iter()
        .map(|target| unicode(target, "command address").map(str::to_owned))
        .collect::<Result<Vec<_>, _>>()?;
    let record = store.remove_commands(id, &targets).map_err(store_error)?;
    write_updated(&record)?;
    Ok(0)
}

fn note(
    address: &str,
    arguments: &[OsString],
    store: &ContextStore,
    input: &mut impl Read,
) -> Result<i32, CliError> {
    let (id, text) = text_input(address, arguments, MAX_NOTE_BYTES, input)?;
    let record = store.append_note(id, text).map_err(store_error)?;
    write_updated(&record)?;
    Ok(0)
}

fn prompt(
    address: &str,
    arguments: &[OsString],
    store: &ContextStore,
    input: &mut impl Read,
) -> Result<i32, CliError> {
    let (id, text) = text_input(address, arguments, MAX_PROMPT_BYTES, input)?;
    let record = store.set_prompt(id, text).map_err(store_error)?;
    write_updated(&record)?;
    Ok(0)
}

fn show(address: &str, arguments: &[OsString], store: &ContextStore) -> Result<i32, CliError> {
    let [id] = arguments else {
        return Err(usage(address, "<context-id>"));
    };
    let id = unicode(id, "Context ID")?;
    let record = store.read(id).map_err(store_error)?;
    let output = serde_json::to_string_pretty(&record)
        .map_err(|error| CliError::new(format!("cannot serialize Context: {error}")))?;
    write_message(&output)?;
    Ok(0)
}

fn render(address: &str, arguments: &[OsString], store: &ContextStore) -> Result<i32, CliError> {
    let [id] = arguments else {
        return Err(usage(address, "<context-id>"));
    };
    let record = store
        .read(unicode(id, "Context ID")?)
        .map_err(store_error)?;
    let output = render_markdown(&record);
    write_message(&output)?;
    Ok(0)
}

fn list(
    address: &str,
    arguments: &[OsString],
    snapshot: &CatalogSnapshot,
    store: &ContextStore,
) -> Result<i32, CliError> {
    if !(arguments.is_empty() || matches!(arguments, [mode] if mode == "--json")) {
        return Err(usage(address, "[--json]"));
    }
    let language = EntryLanguage::parse(snapshot.language)
        .expect("a Catalog snapshot always contains a supported Entry language");
    let collection = project_context_collection(snapshot, language, store).map_err(store_error)?;
    let output = if arguments.is_empty() {
        let identifiers = collection
            .subjects
            .iter()
            .filter_map(|subject| match &subject.reference {
                SubjectRef::Instance { id, .. } => Some(id.as_str()),
                SubjectRef::Command { .. } => None,
            })
            .collect::<Vec<_>>();
        if identifiers.is_empty() {
            "No Contexts.".to_owned()
        } else {
            identifiers.join("\n")
        }
    } else {
        serde_json::to_string_pretty(&collection).map_err(|error| {
            CliError::new(format!("cannot serialize Context collection: {error}"))
        })?
    };
    write_message(&output)?;
    Ok(0)
}

fn delete(address: &str, arguments: &[OsString], store: &ContextStore) -> Result<i32, CliError> {
    let [id] = arguments else {
        return Err(usage(address, "<context-id>"));
    };
    let id = unicode(id, "Context ID")?;
    store.delete(id).map_err(store_error)?;
    write_message(&format!("Context deleted: {id}"))?;
    Ok(0)
}

fn text_input<'a>(
    address: &str,
    arguments: &'a [OsString],
    max_bytes: usize,
    input: &mut impl Read,
) -> Result<(&'a str, String), CliError> {
    let Some((id, text_arguments)) = arguments.split_first() else {
        return Err(usage(address, "<context-id> <text...> | --stdin"));
    };
    let id = unicode(id, "Context ID")?;
    let text = match text_arguments {
        [mode] if mode == "--stdin" => read_stdin(input, max_bytes)?,
        [] => return Err(usage(address, "<context-id> <text...> | --stdin")),
        values => values
            .iter()
            .map(|value| unicode(value, "Context text"))
            .collect::<Result<Vec<_>, _>>()?
            .join(" "),
    };
    validate_text(&text, "Context text", max_bytes).map_err(store_error)?;
    Ok((id, text))
}

fn read_stdin(input: &mut impl Read, max_bytes: usize) -> Result<String, CliError> {
    let mut bytes = Vec::new();
    input
        .take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| CliError::new(format!("cannot read Context text from stdin: {error}")))?;
    if bytes.len() > max_bytes {
        return Err(CliError::new(format!(
            "Context text from stdin accepts at most {max_bytes} UTF-8 bytes"
        )));
    }
    String::from_utf8(bytes)
        .map_err(|_| CliError::new("Context text from stdin is not valid UTF-8"))
}

fn write_updated(record: &ContextRecord) -> Result<(), CliError> {
    write_message(&format!(
        "Context updated: {} ({} commands, {} notes, prompt: {})",
        record.id,
        record.commands.len(),
        record.notes.len(),
        if record.prompt.is_empty() {
            "no"
        } else {
            "yes"
        }
    ))
}

fn write_message(message: &str) -> Result<(), CliError> {
    write_output(message)
        .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))
}

fn unicode<'a>(value: &'a OsString, label: &str) -> Result<&'a str, CliError> {
    value
        .to_str()
        .ok_or_else(|| CliError::new(format!("{label} is not valid Unicode")))
}

fn usage(address: &str, suffix: &str) -> CliError {
    let separator = if suffix.is_empty() { "" } else { " " };
    CliError::new(format!("usage: {address}{separator}{suffix}"))
}

fn store_error(error: impl ToString) -> CliError {
    CliError::new(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_text_joins_arguments_without_stripping_data_quotes() {
        let arguments = [
            OsString::from("work"),
            OsString::from("检查环境"),
            OsString::from("\"then build\""),
        ];
        let (id, text) = text_input(
            ".context.prompt",
            &arguments,
            MAX_PROMPT_BYTES,
            &mut io::empty(),
        )
        .unwrap();
        assert_eq!(id, "work");
        assert_eq!(text, "检查环境 \"then build\"");
    }

    #[test]
    fn stdin_preserves_multiline_utf8_and_enforces_the_byte_limit() {
        let arguments = [OsString::from("work"), OsString::from("--stdin")];
        let mut input = "第一行\n| > & \"第二行\"\n".as_bytes();
        let (_, text) =
            text_input(".context.prompt", &arguments, MAX_PROMPT_BYTES, &mut input).unwrap();
        assert_eq!(text, "第一行\n| > & \"第二行\"\n");

        let oversized_bytes = vec![b'x'; 5];
        let mut oversized = oversized_bytes.as_slice();
        assert!(
            read_stdin(&mut oversized, 4)
                .unwrap_err()
                .to_string()
                .contains("at most 4")
        );
    }
}
