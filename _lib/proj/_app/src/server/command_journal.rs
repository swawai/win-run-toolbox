use std::collections::BTreeMap;
use std::io;

use axum::{
    Json,
    extract::{Path, RawQuery, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::run_journal::{read_run, read_run_history};
use crate::{
    catalog::{CatalogSnapshot, CommandSource},
    command::catalog_command_data_root,
    data_root::DataRootSessionState,
    profile::EntryProfileStore,
};

use super::{ServerState, api_error, data_root_status};

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
    let address = prepared.address;
    match tokio::task::spawn_blocking(move || {
        read_run_history(&prepared.module_data_root, &address)
    })
    .await
    {
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
    let address = prepared.address;
    match tokio::task::spawn_blocking(move || {
        read_run(&prepared.module_data_root, &address, &id, query.after)
    })
    .await
    {
        Ok(Ok(document)) => Json(document).into_response(),
        Ok(Err(error)) => journal_error(error).into_response(),
        Err(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("command journal worker failed: {error}"),
        )
        .into_response(),
    }
}

struct JournalQuery {
    command: CommandLocator,
    after: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandLocator {
    source: CommandSource,
    address: String,
}

impl CommandLocator {
    fn parse(value: String) -> Result<Self, String> {
        let (source, address) = value.split_once('/').ok_or_else(|| {
            "the command locator must use the '<source>/<address>' form".to_owned()
        })?;
        if address.is_empty() || address.contains('/') {
            return Err("the command locator must contain one non-empty address".to_owned());
        }
        let source = match source {
            "kernel" => CommandSource::Kernel,
            "action" => CommandSource::Action,
            _ => {
                return Err(
                    "the command locator source must be either 'kernel' or 'action'".to_owned(),
                );
            }
        };
        Ok(Self {
            source,
            address: address.to_owned(),
        })
    }
}

struct PreparedJournal {
    address: String,
    module_data_root: std::path::PathBuf,
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
        let binding = profile_state.ready().map(|profile| profile.binding());
        if locator.source == CommandSource::Action && binding.is_none() {
            return Err(api_error(
                StatusCode::CONFLICT,
                "a ready Entry Profile is required to locate Action command journals",
            ));
        }
        let catalog = CatalogSnapshot::discover(&context, binding).map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "catalog discovery failed",
            )
        })?;
        let command = catalog
            .commands
            .iter()
            .find(|command| command.source == locator.source && command.address == locator.address)
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "command not found"))?;
        let module_data_root =
            catalog_command_data_root(&context, &data_root_path, binding, command)
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        Ok(PreparedJournal {
            address: locator.address,
            module_data_root,
        })
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
        .and_then(CommandLocator::parse)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_contract_decodes_command_locator_and_rejects_ambiguity() {
        let query = parse_query(Some("command=kernel%2F.demo&after=12"), true).unwrap();
        assert_eq!(query.command.source, CommandSource::Kernel);
        assert_eq!(query.command.address, ".demo");
        assert_eq!(query.after, 12);
        assert_eq!(
            parse_query(Some("command=action%2Fdemo.build"), false)
                .unwrap()
                .command
                .address,
            "demo.build"
        );
        assert!(parse_query(Some("command=kernel%2F.demo&command=action%2Fdemo"), false).is_err());
        assert!(parse_query(Some("command=control%2F..entry&after=1"), true).is_err());
        assert!(parse_query(Some("command=kernel%2F.demo&unknown=1"), true).is_err());
        assert!(parse_query(Some("command=kernel%2F.demo&after=-1"), true).is_err());
    }
}
