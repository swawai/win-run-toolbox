use std::collections::BTreeMap;
use std::io;

use axum::{
    Json,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use crate::{
    catalog::CatalogSnapshot,
    command_journal::{CommandJournalAccess, CommandJournalAccessError, CommandLocator},
    data_root::DataRootSessionState,
    profile::EntryProfileStore,
};

use super::{ServerState, api_error, data_root_status, host_control};

const OPEN_DIRECTORY_COMMAND: &str = "open-journal-directory";

pub(super) async fn get_command_journals(
    State(state): State<ServerState>,
    RawQuery(query): RawQuery,
) -> Response {
    let query = match parse_query(query.as_deref(), false) {
        Ok(query) => query,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, error).into_response(),
    };
    let prepared = match prepare_journal(&state, query.command).await {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    match tokio::task::spawn_blocking(move || prepared.journal.history()).await {
        Ok(Ok(document)) => Json(document).into_response(),
        Ok(Err(error)) => journal_error(error).into_response(),
        Err(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("command journal worker failed: {error}"),
        )
        .into_response(),
    }
}

pub(super) async fn get_command_journal(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let query = match parse_query(query.as_deref(), true) {
        Ok(query) => query,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, error).into_response(),
    };
    let prepared = match prepare_journal(&state, query.command).await {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    match tokio::task::spawn_blocking(move || prepared.journal.run(&id, query.after)).await {
        Ok(Ok(document)) => Json(document).into_response(),
        Ok(Err(error)) => journal_error(error).into_response(),
        Err(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("command journal worker failed: {error}"),
        )
        .into_response(),
    }
}

pub(super) async fn post_open_command_journal_directory(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    if !host_control::has_control_header(&headers, OPEN_DIRECTORY_COMMAND) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let query = match parse_query(query.as_deref(), false) {
        Ok(query) => query,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, error).into_response(),
    };
    let prepared = match prepare_journal(&state, query.command).await {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    match tokio::task::spawn_blocking(move || prepared.journal.open_run_directory(&id)).await {
        Ok(Ok(_)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => journal_error(error).into_response(),
        Err(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("command journal directory worker failed: {error}"),
        )
        .into_response(),
    }
}

struct JournalQuery {
    command: CommandLocator,
    after: u64,
}

struct PreparedJournal {
    journal: CommandJournalAccess,
}

async fn prepare_journal(
    state: &ServerState,
    locator: CommandLocator,
) -> Result<PreparedJournal, (StatusCode, Json<super::ApiError>)> {
    let data_root = match data_root_status(state).await? {
        DataRootSessionState::Ready(resolved) => resolved,
        DataRootSessionState::ClaimRequired(_) => {
            return Err(api_error(
                StatusCode::CONFLICT,
                "DataRoot ownership claim is required",
            ));
        }
    };
    let profile_store = EntryProfileStore::new(&state.context.swawkit_home, data_root.path());
    let context = state.context.clone();
    let data_root_path = data_root.path().to_path_buf();
    tokio::task::spawn_blocking(move || {
        let profile_state = profile_store.read();
        let catalog = CatalogSnapshot::discover(&context, profile_state.ready()).map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "catalog discovery failed",
            )
        })?;
        CommandJournalAccess::resolve(&context, &data_root_path, &profile_state, &catalog, locator)
            .map(|journal| PreparedJournal { journal })
            .map_err(access_error)
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("command journal preparation worker failed: {error}"),
        )
    })?
}

fn parse_query(query: Option<&str>, accepts_after: bool) -> Result<JournalQuery, String> {
    let query = query
        .filter(|query| !query.is_empty())
        .ok_or_else(|| "the command journal query requires one 'command' parameter".to_owned())?;
    let mut values = BTreeMap::new();
    for pair in query.split('&') {
        let (raw_name, raw_value) = pair
            .split_once('=')
            .ok_or_else(|| "command journal query parameters require values".to_owned())?;
        let name = decode_component(raw_name)?;
        let value = decode_component(raw_value)?;
        if values.insert(name.clone(), value).is_some() {
            return Err(format!("the command journal query cannot repeat '{name}'"));
        }
    }
    if values
        .keys()
        .any(|name| name != "command" && (!accepts_after || name != "after"))
    {
        return Err("the command journal query contains an unknown parameter".to_owned());
    }
    let command = values
        .remove("command")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "the command journal query requires a non-empty 'command' parameter".to_owned()
        })
        .and_then(|value| CommandLocator::parse(value).map_err(|error| error.to_string()))?;
    let after = match values.remove("after") {
        Some(value) if accepts_after => value.parse().map_err(|_| {
            "the command journal 'after' cursor must be an unsigned integer".to_owned()
        })?,
        Some(_) => {
            return Err("the command journal history query does not accept 'after'".to_owned());
        }
        None => 0,
    };
    Ok(JournalQuery { command, after })
}

fn decode_component(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => decoded.push(b' '),
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err(
                        "command journal query contains invalid percent encoding".to_owned()
                    );
                }
                let high = hex(bytes[index + 1])?;
                let low = hex(bytes[index + 2])?;
                decoded.push((high << 4) | low);
                index += 2;
            }
            byte => decoded.push(byte),
        }
        index += 1;
    }
    String::from_utf8(decoded).map_err(|_| "command journal query is not valid UTF-8".to_owned())
}

fn hex(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err("command journal query contains invalid percent encoding".to_owned()),
    }
}

fn journal_error(error: io::Error) -> (StatusCode, Json<super::ApiError>) {
    if error.kind() == io::ErrorKind::NotFound {
        api_error(StatusCode::NOT_FOUND, "command journal not found")
    } else {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot read command journal: {error}"),
        )
    }
}

fn access_error(error: CommandJournalAccessError) -> (StatusCode, Json<super::ApiError>) {
    match error {
        CommandJournalAccessError::InvalidLocator(message) => {
            api_error(StatusCode::BAD_REQUEST, message)
        }
        CommandJournalAccessError::ProfileRequired => {
            api_error(StatusCode::CONFLICT, error.to_string())
        }
        CommandJournalAccessError::CommandNotFound => {
            api_error(StatusCode::NOT_FOUND, error.to_string())
        }
        error @ CommandJournalAccessError::AmbiguousCommand(_) => {
            api_error(StatusCode::BAD_REQUEST, error.to_string())
        }
        CommandJournalAccessError::CatalogInvariant(message) => {
            api_error(StatusCode::INTERNAL_SERVER_ERROR, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_contract_decodes_command_locator_and_rejects_ambiguity() {
        let readable = parse_query(Some("command=kernel/.dev.status"), false).unwrap();
        assert_eq!(readable.command.to_string(), "kernel/.dev.status");
        let query = parse_query(Some("command=kernel%2F.demo&after=12"), true).unwrap();
        assert_eq!(
            query.command.source(),
            crate::catalog::CommandSource::Kernel
        );
        assert_eq!(query.command.address(), ".demo");
        assert_eq!(query.after, 12);
        assert_eq!(
            parse_query(Some("command=action%2Fdemo.build"), false)
                .unwrap()
                .command
                .address(),
            "demo.build"
        );
        assert!(parse_query(Some("command=kernel%2F.demo&command=action%2Fdemo"), false).is_err());
        assert!(parse_query(Some("command=control%2F..entry&after=1"), true).is_err());
        assert!(parse_query(Some("command=kernel%2F.demo&unknown=1"), true).is_err());
        assert!(parse_query(Some("command=kernel%2F.demo&after=-1"), true).is_err());
    }
}
