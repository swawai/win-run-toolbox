use std::ffi::OsString;
use std::path::PathBuf;

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::{
    catalog::{CatalogSnapshot, CommandNode, CommandSource},
    context::EntryContext,
    data_root::DataRootSessionState,
    entry_runner::EntryRunSpec,
    facet::{Facet, FacetKind, FacetResolver, valid_facet_id},
    profile::EntryProfileStore,
    subject::{SUBJECT_COLLECTION_PROTOCOL, SubjectCollection, SubjectRef},
};

use super::command_run::CommandRuns;
use super::{ServerState, api_error, data_root_status};

mod collection;

use collection::{collection_subject_kind, validate_collection_contract};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FacetResolutionRequest {
    subject: SubjectRef,
    facet: String,
    #[serde(default)]
    via: Option<FacetCollectionRef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FacetCollectionRef {
    subject: SubjectRef,
    facet: String,
}

struct FacetResolutionDocument {
    value: serde_json::Value,
    collection: Option<SubjectCollection>,
}

struct ResolutionContext {
    entry: EntryContext,
    working_directory: PathBuf,
    catalog: CatalogSnapshot,
    command_runs: CommandRuns,
}

type ApiResult<T> = Result<T, (StatusCode, Json<super::ApiError>)>;

pub(super) async fn post_facet_resolution(
    State(state): State<ServerState>,
    Json(request): Json<FacetResolutionRequest>,
) -> Response {
    if !valid_facet_id(&request.facet) {
        return api_error(StatusCode::UNPROCESSABLE_ENTITY, "facet id is invalid").into_response();
    }
    let resolution = match resolution_context(&state).await {
        Ok(resolution) => resolution,
        Err(error) => return error.into_response(),
    };
    match tokio::task::spawn_blocking(move || resolve_request(&resolution, request)).await {
        Ok(Ok(document)) => Json(document.value).into_response(),
        Ok(Err(error)) => error.into_response(),
        Err(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("facet resolution worker failed: {error}"),
        )
        .into_response(),
    }
}

async fn resolution_context(state: &ServerState) -> ApiResult<ResolutionContext> {
    let resolved = match data_root_status(state).await? {
        DataRootSessionState::Ready(resolved) => resolved,
        DataRootSessionState::ClaimRequired(_) => {
            return Err(api_error(
                StatusCode::CONFLICT,
                "DataRoot ownership claim is required",
            ));
        }
    };
    let entry = state.context.clone();
    let command_runs = state.command_runs.clone();
    let data_root = resolved.path().to_path_buf();
    tokio::task::spawn_blocking(move || {
        let profile_state = EntryProfileStore::new(&entry.swawkit_home, &data_root).read();
        let catalog = CatalogSnapshot::discover(&entry, profile_state.ready()).map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "catalog discovery failed",
            )
        })?;
        let working_directory = profile_state
            .ready()
            .map(|profile| profile.binding().target_project_root().to_path_buf())
            .unwrap_or_else(|| entry.invocation_directory.clone());
        Ok(ResolutionContext {
            entry,
            working_directory,
            catalog,
            command_runs,
        })
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("facet resolution worker failed: {error}"),
        )
    })?
}

fn resolve_request(
    context: &ResolutionContext,
    request: FacetResolutionRequest,
) -> ApiResult<FacetResolutionDocument> {
    let facet = match &request.subject {
        SubjectRef::Command { source, address } => {
            if request.via.is_some() {
                return Err(api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "a command Subject facet cannot declare a via collection",
                ));
            }
            command_facet(&context.catalog, *source, address, &request.facet)?.clone()
        }
        SubjectRef::Instance { .. } => {
            let via = request.via.as_ref().ok_or_else(|| {
                api_error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "an instance Subject facet requires a via collection",
                )
            })?;
            instance_facet(context, &request.subject, &request.facet, via)?
        }
    };
    let document = resolve_declared_facet(context, &facet)?;
    if let Some(collection) = &document.collection {
        if collection.protocol != SUBJECT_COLLECTION_PROTOCOL
            || &collection.owner != &request.subject
            || collection.facet != request.facet
        {
            return Err(api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "collection facet resolver returned a mismatched owner or facet",
            ));
        }
        validate_collection_contract(&context.catalog, collection, &facet)?;
    }
    Ok(document)
}

fn command_facet<'a>(
    catalog: &'a CatalogSnapshot,
    source: CommandSource,
    address: &str,
    facet_id: &str,
) -> ApiResult<&'a Facet> {
    let command = catalog
        .commands
        .iter()
        .find(|command| {
            command.source == source && command.address == address && command.alias_of.is_none()
        })
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Subject not found"))?;
    command
        .facets
        .iter()
        .find(|facet| facet.id == facet_id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Subject facet not found"))
}

fn instance_facet(
    context: &ResolutionContext,
    subject_ref: &SubjectRef,
    facet_id: &str,
    via: &FacetCollectionRef,
) -> ApiResult<Facet> {
    if !valid_facet_id(&via.facet) {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "via facet id is invalid",
        ));
    }
    let SubjectRef::Command { source, address } = &via.subject else {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "a via collection must belong to a command Subject",
        ));
    };
    let collection_facet = command_facet(&context.catalog, *source, address, &via.facet)?;
    if collection_facet.kind != FacetKind::Collection {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the requested via facet is not a collection",
        ));
    }
    let document = resolve_declared_facet(context, collection_facet)?;
    let collection = document.collection.ok_or_else(|| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "collection facet resolver returned the wrong document type",
        )
    })?;
    if &collection.owner != &via.subject || collection.facet != via.facet {
        return Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "collection facet resolver returned a mismatched owner or facet",
        ));
    }
    validate_collection_contract(&context.catalog, &collection, collection_facet)?;
    let subject = collection
        .subjects
        .iter()
        .find(|subject| &subject.reference == subject_ref)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Subject not found"))?;
    if !subject
        .facet_ids
        .iter()
        .any(|candidate| candidate == facet_id)
    {
        return Err(api_error(StatusCode::NOT_FOUND, "Subject facet not found"));
    }
    let subject_kind = collection_subject_kind(&context.catalog, collection_facet)?;
    let SubjectRef::Instance { id, .. } = subject_ref else {
        unreachable!("SubjectCollection v2 only contains instance Subjects");
    };
    subject_kind
        .instantiate(facet_id, id)
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Subject facet template is invalid",
            )
        })?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Subject facet not found"))
}

fn resolve_declared_facet(
    context: &ResolutionContext,
    facet: &Facet,
) -> ApiResult<FacetResolutionDocument> {
    if facet.kind == FacetKind::Operation {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "operation facets must run through the command execution boundary",
        ));
    }
    let Some(FacetResolver::Command {
        address,
        arguments,
        accepts_tail,
        confirmation,
        returns,
    }) = &facet.resolver
    else {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the requested facet has no document resolver",
        ));
    };
    if *accepts_tail || confirmation.is_some() {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "document resolvers must use exact arguments without confirmation",
        ));
    }
    let returns = returns.as_deref().ok_or_else(|| {
        api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "document resolvers must declare their return protocol",
        )
    })?;
    if (facet.kind == FacetKind::Collection) != (returns == SUBJECT_COLLECTION_PROTOCOL) {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "collection facets must return the Subject collection protocol",
        ));
    }
    resolve_command_document(context, address, arguments, returns)
}

fn resolve_command_document(
    context: &ResolutionContext,
    address: &str,
    arguments: &[String],
    returns: &str,
) -> ApiResult<FacetResolutionDocument> {
    exact_runnable_command(&context.catalog, address)?;
    let mut argv = Vec::with_capacity(arguments.len() + 1);
    argv.push(OsString::from(address));
    argv.extend(arguments.iter().map(OsString::from));
    let output = context
        .command_runs
        .query(EntryRunSpec {
            id: String::new(),
            entry_file: context.entry.entry_file.clone(),
            working_directory: context.working_directory.clone(),
            argv,
        })
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "facet resolver command failed",
            )
        })?;
    let value: serde_json::Value = serde_json::from_str(&output.stdout).map_err(|_| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "facet resolver command returned invalid JSON",
        )
    })?;
    validate_return_protocol(&value, returns)?;
    let collection = if returns == SUBJECT_COLLECTION_PROTOCOL {
        let collection: SubjectCollection =
            serde_json::from_value(value.clone()).map_err(|_| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "facet resolver command returned an invalid Subject collection",
                )
            })?;
        collection.validate().map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "facet resolver command returned an invalid Subject collection",
            )
        })?;
        Some(collection)
    } else {
        None
    };
    Ok(FacetResolutionDocument { value, collection })
}

fn validate_return_protocol(document: &serde_json::Value, expected: &str) -> ApiResult<()> {
    let object = document.as_object().ok_or_else(|| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "facet resolver command must return a JSON object",
        )
    })?;
    let protocol = object.get("protocol").and_then(serde_json::Value::as_str);
    let schema = object.get("schema").and_then(serde_json::Value::as_str);
    if matches!((protocol, schema), (Some(actual), None) | (None, Some(actual)) if actual == expected)
    {
        Ok(())
    } else {
        Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "facet resolver command returned the wrong protocol",
        ))
    }
}

fn exact_runnable_command<'a>(
    catalog: &'a CatalogSnapshot,
    address: &str,
) -> ApiResult<&'a CommandNode> {
    let mut matches = catalog.commands.iter().filter(|command| {
        command.address == address
            && command.source != CommandSource::Control
            && command.runnable
            && command.alias_of.is_none()
    });
    let command = matches
        .next()
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "resolver command not found"))?;
    if matches.next().is_some() {
        return Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "resolver command address is ambiguous",
        ));
    }
    Ok(command)
}
