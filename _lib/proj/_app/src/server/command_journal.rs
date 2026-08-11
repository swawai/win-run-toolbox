use std::collections::BTreeMap;
use std::io;

use axum::{
    Json,
    extract::{Path, RawQuery, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::run_journal::{read_run, read_run_history};

use super::{ServerState, api_error, command_run::prepare_run};

pub(super) async fn get_command_journals(
    State(state): State<ServerState>,
    RawQuery(query): RawQuery,
) -> Response {
    let query = match parse_query(query.as_deref(), false) {
        Ok(query) => query,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, error).into_response(),
    };
    let prepared = match prepare_run(&state, &query.address).await {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    let address = query.address;
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
    let prepared = match prepare_run(&state, &query.address).await {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    let address = query.address;
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
    address: String,
    after: u64,
}

fn parse_query(query: Option<&str>, accepts_after: bool) -> Result<JournalQuery, String> {
    let query = query
        .filter(|query| !query.is_empty())
        .ok_or_else(|| "the command journal query requires one 'address' parameter".to_owned())?;
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
        .any(|name| name != "address" && (!accepts_after || name != "after"))
    {
        return Err("the command journal query contains an unknown parameter".to_owned());
    }
    let address = values
        .remove("address")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "the command journal query requires a non-empty 'address' parameter".to_owned()
        })?;
    let after = match values.remove("after") {
        Some(value) if accepts_after => value.parse().map_err(|_| {
            "the command journal 'after' cursor must be an unsigned integer".to_owned()
        })?,
        Some(_) => {
            return Err("the command journal history query does not accept 'after'".to_owned());
        }
        None => 0,
    };
    Ok(JournalQuery { address, after })
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
    fn query_contract_decodes_address_and_rejects_ambiguity() {
        let query = parse_query(Some("address=.demo&after=12"), true).unwrap();
        assert_eq!(query.address, ".demo");
        assert_eq!(query.after, 12);
        assert_eq!(
            parse_query(Some("address=%2Edemo"), false).unwrap().address,
            ".demo"
        );
        assert!(parse_query(Some("address=.demo&address=.other"), false).is_err());
        assert!(parse_query(Some("address=.demo&unknown=1"), true).is_err());
        assert!(parse_query(Some("address=.demo&after=-1"), true).is_err());
    }
}
