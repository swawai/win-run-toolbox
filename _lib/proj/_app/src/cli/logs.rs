use std::ffi::{OsStr, OsString};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use swawkit_proj::{
    catalog::{CatalogSnapshot, CommandSource},
    command_journal::{CommandJournalAccess, CommandLocator, RunJournalHistoryDocument},
    context::EntryContext,
    profile::EntryProfileState,
    subject::{SUBJECT_COLLECTION_PROTOCOL, SubjectCollection, SubjectRef, SubjectSummary},
};

use super::{CliError, write_output};

const LOGS_ADDRESS: &str = ".logs";
const MAX_LATEST_RANGE: usize = 32;
const RUNS_FACET: &str = "runs";
const RUN_KIND: &str = "run";

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

    match argv.get(1..) {
        Some([option]) if option == "--json" => {
            write_json(&run_collection(
                snapshot,
                context,
                data_root,
                profile_state,
            )?)?;
            return Ok(Some(0));
        }
        Some([option, target]) if option == "--json" => {
            write_json(&command_run_collection(
                snapshot,
                context,
                data_root,
                profile_state,
                unicode_argument(target, "command locator")?,
            )?)?;
            return Ok(Some(0));
        }
        Some([option, id]) if option == "--run" => {
            let id = unicode_argument(id, "run id")?;
            write_json(&global_run(
                snapshot,
                context,
                data_root,
                profile_state,
                id,
                0,
            )?)?;
            return Ok(Some(0));
        }
        Some([option, id, cursor_option, cursor])
            if option == "--run" && cursor_option == "--after" =>
        {
            let id = unicode_argument(id, "run id")?;
            let after = parse_after_cursor(cursor)?;
            write_json(&global_run(
                snapshot,
                context,
                data_root,
                profile_state,
                id,
                after,
            )?)?;
            return Ok(Some(0));
        }
        Some([option, id]) if option == "--open" => {
            let id = unicode_argument(id, "run id")?;
            let path = global_run_access(snapshot, context, data_root, profile_state, id)?
                .open_run_directory(id)
                .map_err(|error| CliError::new(format!("cannot open command journal: {error}")))?;
            write_output(&path.display().to_string())
                .map_err(|error| CliError::new(format!("cannot write CLI output: {error}")))?;
            return Ok(Some(0));
        }
        _ => {}
    }

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
            let after = parse_after_cursor(cursor)?;
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

fn parse_after_cursor(cursor: &OsStr) -> Result<u64, CliError> {
    unicode_argument(cursor, "after cursor")?
        .parse::<u64>()
        .map_err(|_| CliError::new("after cursor must be an unsigned integer"))
}

fn run_collection(
    snapshot: &CatalogSnapshot,
    context: &EntryContext,
    data_root: &Path,
    profile_state: &EntryProfileState,
) -> Result<SubjectCollection, CliError> {
    let facet_ids = run_facet_ids(snapshot)?;
    let mut runs = Vec::new();
    for (locator, journal) in all_journals(snapshot, context, data_root, profile_state)? {
        for run in journal.subject_runs().map_err(journal_read_error)? {
            runs.push((run.started_at_unix_ms, locator.clone(), run));
        }
    }
    runs.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.2.id.cmp(&left.2.id))
    });
    runs.truncate(32);

    let mut seen = std::collections::BTreeSet::new();
    let subjects = runs
        .into_iter()
        .map(|(_, locator, run)| {
            if !seen.insert(run.id.clone()) {
                return Err(CliError::new(format!(
                    "run id '{}' is ambiguous across command journals",
                    run.id
                )));
            }
            Ok(SubjectSummary {
                reference: SubjectRef::Instance {
                    kind: RUN_KIND.to_owned(),
                    id: run.id,
                },
                label: format_timestamp(run.started_at_unix_ms),
                summary: format!(
                    "{locator} · {state} · {source} · {} events",
                    run.event_count,
                    state = run.state,
                    source = run.source,
                ),
                facet_ids: facet_ids.clone(),
            })
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    Ok(SubjectCollection {
        protocol: SUBJECT_COLLECTION_PROTOCOL.to_owned(),
        owner: SubjectRef::Command {
            source: CommandSource::Kernel,
            address: LOGS_ADDRESS.to_owned(),
        },
        facet: RUNS_FACET.to_owned(),
        subjects,
    })
}

fn command_run_collection(
    snapshot: &CatalogSnapshot,
    context: &EntryContext,
    data_root: &Path,
    profile_state: &EntryProfileState,
    target: &str,
) -> Result<SubjectCollection, CliError> {
    let facet_ids = run_facet_ids(snapshot)?;
    let locator = CommandLocator::from_cli_target(snapshot, target)
        .map_err(|error| CliError::new(error.to_string()))?;
    let owner = SubjectRef::Command {
        source: locator.source(),
        address: locator.address().to_owned(),
    };
    let locator_label = locator.to_string();
    let journal =
        CommandJournalAccess::resolve(context, data_root, profile_state, snapshot, locator)
            .map_err(|error| CliError::new(error.to_string()))?;
    let subjects = journal
        .subject_runs()
        .map_err(journal_read_error)?
        .into_iter()
        .take(32)
        .map(|run| SubjectSummary {
            reference: SubjectRef::Instance {
                kind: RUN_KIND.to_owned(),
                id: run.id,
            },
            label: format_timestamp(run.started_at_unix_ms),
            summary: format!(
                "{locator_label} · {state} · {source} · {} events",
                run.event_count,
                state = run.state,
                source = run.source,
            ),
            facet_ids: facet_ids.clone(),
        })
        .collect();
    Ok(SubjectCollection {
        protocol: SUBJECT_COLLECTION_PROTOCOL.to_owned(),
        owner,
        facet: RUNS_FACET.to_owned(),
        subjects,
    })
}

fn run_facet_ids(snapshot: &CatalogSnapshot) -> Result<Vec<String>, CliError> {
    let owner = snapshot
        .commands
        .iter()
        .find(|command| {
            command.source == CommandSource::Kernel
                && command.address == LOGS_ADDRESS
                && command.alias_of.is_none()
        })
        .ok_or_else(|| CliError::new("Run Subject owner command is unavailable"))?;
    let kind = owner
        .subject_kinds
        .iter()
        .find(|kind| kind.kind == RUN_KIND)
        .ok_or_else(|| CliError::new("Run Subject kind is unavailable"))?;
    Ok(kind.facets.iter().map(|facet| facet.id.clone()).collect())
}

fn all_journals(
    snapshot: &CatalogSnapshot,
    context: &EntryContext,
    data_root: &Path,
    profile_state: &EntryProfileState,
) -> Result<Vec<(String, CommandJournalAccess)>, CliError> {
    snapshot
        .commands
        .iter()
        .filter(|command| {
            command.source != CommandSource::Control
                && !command.address.is_empty()
                && command.alias_of.is_none()
                && (command.source != CommandSource::Action || profile_state.ready().is_some())
        })
        .map(|command| {
            let source = match command.source {
                CommandSource::Kernel => "kernel",
                CommandSource::Action => "action",
                CommandSource::Control => unreachable!("Control journals are filtered"),
            };
            let locator = format!("{source}/{}", command.address);
            let journal = CommandJournalAccess::resolve(
                context,
                data_root,
                profile_state,
                snapshot,
                CommandLocator::parse(&locator)
                    .map_err(|error| CliError::new(error.to_string()))?,
            )
            .map_err(|error| CliError::new(error.to_string()))?;
            Ok((locator, journal))
        })
        .collect()
}

fn global_run(
    snapshot: &CatalogSnapshot,
    context: &EntryContext,
    data_root: &Path,
    profile_state: &EntryProfileState,
    id: &str,
    after: u64,
) -> Result<swawkit_proj::command_journal::RunJournalDocument, CliError> {
    global_run_access(snapshot, context, data_root, profile_state, id)?
        .run(id, after)
        .map_err(journal_read_error)
}

fn global_run_access(
    snapshot: &CatalogSnapshot,
    context: &EntryContext,
    data_root: &Path,
    profile_state: &EntryProfileState,
    id: &str,
) -> Result<CommandJournalAccess, CliError> {
    let mut found = None;
    for (_, journal) in all_journals(snapshot, context, data_root, profile_state)? {
        match journal.run_directory(id) {
            Ok(_) if found.is_some() => {
                return Err(CliError::new(format!(
                    "run id '{id}' is ambiguous across command journals"
                )));
            }
            Ok(_) => found = Some(journal),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(journal_read_error(error)),
        }
    }
    found.ok_or_else(|| CliError::new("cannot read command journal: command journal not found"))
}

fn format_timestamp(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    let second = seconds % 60;
    let minutes = seconds / 60;
    let minute = minutes % 60;
    let hours = minutes / 60;
    let hour = hours % 24;
    let days = i64::try_from(hours / 24).unwrap_or(i64::MAX);
    let (year, month, day) = civil_date(days);
    format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{:03}Z",
        milliseconds % 1_000
    )
}

fn civil_date(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

fn require_logs_command(snapshot: &CatalogSnapshot) -> Result<(), CliError> {
    if snapshot.commands.iter().any(|command| {
        command.source == CommandSource::Kernel
            && command.address == LOGS_ADDRESS
            && command.adapter.as_deref() == Some("core")
            && command.handler.as_deref() == Some("meta.logs")
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
        "usage: .logs [--json [<command-locator>] | --run <run-id> [--after <cursor>] | --open <run-id>] | .logs <command-address> [--latest <n|n..m> | --run <run-id> [--after <cursor>] | --open <run-id>]",
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

    #[test]
    fn global_run_subject_timestamps_are_stable_utc_labels() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00.000Z");
        assert_eq!(
            format_timestamp(1_787_027_678_901),
            "2026-08-18 04:34:38.901Z"
        );
    }
}
