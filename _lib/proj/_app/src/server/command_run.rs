mod registry;

use std::ffi::OsString;
use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, RawQuery, State},
    http::{HeaderValue, StatusCode, header::LOCATION},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};

use crate::{
    catalog::{CatalogSnapshot, CommandSource},
    command::{CommandExecutionContext, CommandProcessMode, ResolvedCommand, command_data_root},
    data_root::DataRootSessionState,
    entry_runner::EntryRunSpec,
    profile::{EntryProfileState, EntryProfileStore},
    run_journal::{RunJournalEvent, RunJournalSource, StartRunJournal},
};

use super::{ServerState, api_error, data_root_status};
pub(super) use registry::{CommandRuns, RegistryError};

pub(super) const COMMAND_RUN_PROTOCOL: &str = "swawkit.command-run/v1";
const MAX_ARGUMENT_COUNT: usize = 128;
const MAX_ARGUMENT_UTF16: usize = 4096;
const MAX_COMMAND_UTF16: usize = 8192;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StartCommandRunRequest {
    address: String,
    #[serde(default)]
    arguments: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CommandRunDocument {
    pub protocol: &'static str,
    pub id: String,
    pub address: String,
    pub state: CommandRunState,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub next_cursor: u64,
    pub events: Vec<RunJournalEvent>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum CommandRunState {
    #[default]
    Running,
    Canceling,
    Exited,
    Canceled,
    Failed,
}

impl CommandRunState {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Exited | Self::Canceled | Self::Failed)
    }
}

pub(super) async fn post_command_run(
    State(state): State<ServerState>,
    Json(request): Json<StartCommandRunRequest>,
) -> Response {
    if let Err(error) = validate_start_request(&request) {
        return api_error(StatusCode::UNPROCESSABLE_ENTITY, error).into_response();
    }
    let prepared = match prepare_run(&state, &request.address).await {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    let address = request.address;
    let mut argv = Vec::with_capacity(request.arguments.len() + 1);
    argv.push(OsString::from(&address));
    argv.extend(request.arguments.into_iter().map(OsString::from));
    let argument_count = argv.len().saturating_sub(1);
    let spec = EntryRunSpec {
        id: String::new(),
        entry_file: state.context.entry_file.clone(),
        working_directory: prepared.working_directory,
        argv,
    };
    let runs = state.command_runs.clone();
    let start_address = address.clone();
    let journal_request = StartRunJournal {
        module_data_root: prepared.module_data_root,
        address: address.clone(),
        source: RunJournalSource::Web,
        argument_count,
        profile_revision: prepared.profile_revision,
    };
    let started =
        match tokio::task::spawn_blocking(move || runs.start(start_address, spec, journal_request))
            .await
        {
            Ok(started) => started,
            Err(error) => {
                return api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("command run worker failed: {error}"),
                )
                .into_response();
            }
        };
    match started {
        Ok(document) => {
            let location = HeaderValue::from_str(&format!("/api/v2/command-runs/{}", document.id))
                .expect("command run identifiers are valid Location values");
            (StatusCode::CREATED, [(LOCATION, location)], Json(document)).into_response()
        }
        Err(error) => registry_error(error).into_response(),
    }
}

fn validate_start_request(request: &StartCommandRunRequest) -> Result<(), &'static str> {
    if request.arguments.len() > MAX_ARGUMENT_COUNT {
        return Err("a command run accepts at most 128 arguments");
    }
    if request.address.contains('\0') || request.arguments.iter().any(|value| value.contains('\0'))
    {
        return Err("command addresses and arguments cannot contain NUL characters");
    }

    let address_units = request.address.encode_utf16().count();
    if address_units > MAX_ARGUMENT_UTF16
        || request
            .arguments
            .iter()
            .any(|value| value.encode_utf16().count() > MAX_ARGUMENT_UTF16)
    {
        return Err("each command address or argument accepts at most 4096 UTF-16 code units");
    }
    let total_units = address_units
        + request
            .arguments
            .iter()
            .map(|value| value.encode_utf16().count())
            .sum::<usize>();
    if total_units > MAX_COMMAND_UTF16 {
        return Err("a command run accepts at most 8192 UTF-16 code units in total");
    }
    Ok(())
}

pub(super) async fn get_command_run(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    RawQuery(query): RawQuery,
) -> Response {
    let after = match parse_after(query.as_deref()) {
        Ok(after) => after,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, error).into_response(),
    };
    match state.command_runs.get(&id, after) {
        Ok(document) => Json(document).into_response(),
        Err(error) => registry_error(error).into_response(),
    }
}

fn parse_after(query: Option<&str>) -> Result<u64, &'static str> {
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return Ok(0);
    };
    let value = query
        .strip_prefix("after=")
        .filter(|value| !value.is_empty() && !value.contains('&'))
        .ok_or("the command run query accepts only one 'after' cursor")?;
    value
        .parse()
        .map_err(|_| "the command run 'after' cursor must be an unsigned integer")
}

pub(super) async fn delete_command_run(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Response {
    let runs = state.command_runs.clone();
    match tokio::task::spawn_blocking(move || runs.cancel(&id)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => registry_error(error).into_response(),
        Err(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("command cancellation worker failed: {error}"),
        )
        .into_response(),
    }
}

pub(super) struct PreparedRun {
    pub working_directory: PathBuf,
    pub module_data_root: PathBuf,
    pub profile_revision: String,
}

pub(super) async fn prepare_run(
    state: &ServerState,
    address: &str,
) -> Result<PreparedRun, (StatusCode, Json<super::ApiError>)> {
    if address.is_empty() {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "command address cannot be empty",
        ));
    }
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
    let address = address.to_owned();
    tokio::task::spawn_blocking(move || {
        let profile_state = profile_store.read();
        let profile = match &profile_state {
            EntryProfileState::Ready(profile) => profile,
            EntryProfileState::Missing { .. } => {
                return Err(api_error(
                    StatusCode::CONFLICT,
                    "entry profile setup is required before running commands",
                ));
            }
            EntryProfileState::Invalid { error, .. } => {
                return Err(api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    format!("entry profile is invalid: {error}"),
                ));
            }
        };
        let catalog =
            CatalogSnapshot::discover(&context, Some(profile.binding())).map_err(|_| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "catalog discovery failed",
                )
            })?;
        if !catalog
            .commands
            .iter()
            .any(|command| command.address == address)
        {
            return Err(api_error(StatusCode::NOT_FOUND, "command not found"));
        }
        let command = ResolvedCommand::from_catalog(&catalog, &address)
            .map_err(|error| api_error(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
        if !matches!(
            command.source,
            CommandSource::Kernel | CommandSource::Action
        ) {
            return Err(api_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "only Kernel and Action commands can run through the Web command worker",
            ));
        }
        let execution_context = CommandExecutionContext::new(
            &context,
            profile,
            &data_root_path,
            CommandProcessMode::NoWindow,
        );
        Ok(PreparedRun {
            working_directory: profile.binding().target_project_root().to_path_buf(),
            module_data_root: command_data_root(&execution_context, &command)
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?,
            profile_revision: profile.profile_revision().to_owned(),
        })
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("command preparation worker failed: {error}"),
        )
    })?
}

fn registry_error(error: RegistryError) -> (StatusCode, Json<super::ApiError>) {
    match error {
        RegistryError::Capacity => api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "too many command runs are active",
        ),
        RegistryError::NotFound => api_error(StatusCode::NOT_FOUND, "command run not found"),
        RegistryError::ShuttingDown => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "the Entry Host is shutting down",
        ),
        RegistryError::Start(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot start the Entry command: {error}"),
        ),
        RegistryError::Journal(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot start the command journal: {error}"),
        ),
        RegistryError::Cancel(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot cancel the Entry command: {error}"),
        ),
        RegistryError::Unavailable => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "command run registry is unavailable",
        ),
    }
}
