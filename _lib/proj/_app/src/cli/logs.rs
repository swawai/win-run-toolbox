use std::ffi::{OsStr, OsString};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use swawkit_proj::{
    catalog::{CatalogSnapshot, CommandSource},
    command_journal::{CommandJournalAccess, CommandLocator, RunJournalHistoryDocument},
    context::EntryContext,
    profile::EntryProfileState,
};

use super::{CliError, write_output};

const LOGS_ADDRESS: &str = ".logs";
const MAX_LATEST_RANGE: usize = 32;

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
    if address != LOGS_ADDRESS {
        return Ok(None);
    }
    require_logs_command(snapshot)?;
    let target = argv
        .get(1)
        .ok_or_else(logs_usage)
        .and_then(|value| unicode_argument(value, "command address"))?;
    let locator = CommandLocator::from_cli_target(snapshot, target)
        .map_err(|error| CliError::new(error.to_string()))?;
    let journal =
        CommandJournalAccess::resolve(context, data_root, profile_state, snapshot, locator)
            .map_err(|error| CliError::new(error.to_string()))?;

    match argv.get(2..) {
        Some([]) => write_numbered_history(&journal.history().map_err(journal_read_error)?)?,
        Some([option, selector]) if option == "--latest" => {
            let selector = parse_latest_selector(unicode_argument(selector, "latest selector")?)?;
            if selector.start == selector.end {
                write_json(
                    &journal
                        .latest_run(selector.start)
                        .map_err(journal_read_error)?,
                )?;
            } else {
                let documents = journal
                    .latest_runs(selector.start, selector.end)
                    .map_err(journal_read_error)?;
                write_json(&documents)?;
            }
        }
        Some([option, id]) if option == "--run" => {
            let id = unicode_argument(id, "run id")?;
            write_json(&journal.run(id, 0).map_err(journal_read_error)?)?;
        }
        Some([option, id, cursor_option, cursor])
            if option == "--run" && cursor_option == "--after" =>
        {
            let id = unicode_argument(id, "run id")?;
            let after = unicode_argument(cursor, "after cursor")?
                .parse::<u64>()
                .map_err(|_| CliError::new("after cursor must be an unsigned integer"))?;
            write_json(&journal.run(id, after).map_err(journal_read_error)?)?;
        }
        Some([option, id]) if option == "--open" => {
            let id = unicode_argument(id, "run id")?;
            let path = journal
                .open_run_directory(id)
                .map_err(|error| CliError::new(format!("cannot open command journal: {error}")))?;
            write_output(&path.display().to_string())
                .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))?;
        }
        _ => return Err(logs_usage()),
    }
    Ok(Some(0))
}

fn require_logs_command(snapshot: &CatalogSnapshot) -> Result<(), CliError> {
    if snapshot.commands.iter().any(|command| {
        command.source == CommandSource::Kernel
            && command.address == LOGS_ADDRESS
            && command.runnable
    }) {
        Ok(())
    } else {
        Err(CliError::new("command not found: .logs"))
    }
}

struct LatestSelector {
    start: usize,
    end: usize,
}

fn parse_latest_selector(value: &str) -> Result<LatestSelector, CliError> {
    let (start, end) = match value.split_once("..") {
        Some((start, end)) if !end.contains("..") => {
            (parse_latest_ordinal(start)?, parse_latest_ordinal(end)?)
        }
        None => {
            let ordinal = parse_latest_ordinal(value)?;
            (ordinal, ordinal)
        }
        _ => return Err(latest_selector_error()),
    };
    if start > end || end - start + 1 > MAX_LATEST_RANGE {
        return Err(latest_selector_error());
    }
    Ok(LatestSelector { start, end })
}

fn parse_latest_ordinal(value: &str) -> Result<usize, CliError> {
    value
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(latest_selector_error)
}

fn latest_selector_error() -> CliError {
    CliError::new("latest selector must be a positive ordinal or inclusive range such as '1..3'")
}

fn write_numbered_history(document: &RunJournalHistoryDocument) -> Result<(), CliError> {
    let mut value = serde_json::to_value(document)
        .map_err(|error| CliError::new(format!("cannot serialize command journals: {error}")))?;
    let runs = value
        .get_mut("runs")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| CliError::new("command journal history invariant failed"))?;
    for (index, run) in runs.iter_mut().enumerate() {
        run.as_object_mut()
            .ok_or_else(|| CliError::new("command journal history invariant failed"))?
            .insert("latest".to_owned(), Value::from(index + 1));
    }
    write_json(&value)
}

fn logs_usage() -> CliError {
    CliError::new(
        "usage: .logs <command-address> [--latest <n|n..m> | --run <run-id> [--after <cursor>] | --open <run-id>]",
    )
}

fn journal_read_error(error: std::io::Error) -> CliError {
    CliError::new(format!("cannot read command journal: {error}"))
}

fn write_json(document: &impl Serialize) -> Result<(), CliError> {
    let output = serde_json::to_string_pretty(document)
        .map_err(|error| CliError::new(format!("cannot serialize command journal: {error}")))?;
    write_output(&output)
        .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))
}

fn unicode_argument<'a>(argument: &'a OsStr, name: &str) -> Result<&'a str, CliError> {
    argument
        .to_str()
        .ok_or_else(|| CliError::new(format!("{name} is not valid Unicode")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_selectors_are_explicit_and_bounded() {
        let one = parse_latest_selector("1").unwrap();
        assert_eq!((one.start, one.end), (1, 1));
        let range = parse_latest_selector("1..3").unwrap();
        assert_eq!((range.start, range.end), (1, 3));
        for invalid in ["0", "-1", "1,3", "3..1", "1..33", "1..2..3"] {
            assert!(parse_latest_selector(invalid).is_err(), "{invalid}");
        }
    }
}
