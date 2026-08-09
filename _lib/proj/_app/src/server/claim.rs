use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header::ETAG},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::data_root::{
    DataRootClaim, DataRootClaimDocument, DataRootClaimResultDocument, DataRootSessionError,
    DataRootSessionState,
};

use super::{ServerState, api_error, data_root_status, expected_revision};

pub(super) async fn get_claim(State(state): State<ServerState>) -> Response {
    match data_root_status(&state).await {
        Ok(DataRootSessionState::Ready(resolved)) if resolved.warnings().is_empty() => {
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(DataRootSessionState::Ready(resolved)) => Json(DataRootClaimResultDocument::ready(
            resolved.warnings().to_vec(),
        ))
        .into_response(),
        Ok(DataRootSessionState::ClaimRequired(claim)) => claim_response(&claim),
        Err(error) => error.into_response(),
    }
}

pub(super) async fn post_claim(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(request): Json<ClaimRequest>,
) -> Response {
    let revision = match expected_revision(&headers, "DataRoot claim") {
        Ok(revision) => revision.to_owned(),
        Err(error) => return error.into_response(),
    };
    let data_root = state.data_root.clone();
    let confirmation = request.confirmation;
    let claim = match tokio::task::spawn_blocking(move || data_root.claim(&revision, &confirmation))
        .await
    {
        Ok(claim) => claim,
        Err(error) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("DataRoot claim worker failed: {error}"),
            )
            .into_response();
        }
    };
    match claim {
        Ok(warnings) => {
            for warning in &warnings {
                eprintln!("[WARNING] {warning}");
            }
            Json(DataRootClaimResultDocument::claimed(warnings))
            .into_response()
        }
        Err(error @ DataRootSessionError::ConfirmationMismatch { .. }) => {
            api_error(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()).into_response()
        }
        Err(DataRootSessionError::Conflict) => api_error(
            StatusCode::CONFLICT,
            "DataRoot claim changed since it was loaded; review the current claim before confirming",
        )
        .into_response(),
        Err(error) => api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

fn claim_response(claim: &DataRootClaim) -> Response {
    let revision = claim.revision();
    let etag = HeaderValue::from_str(&format!("\"{revision}\""))
        .expect("claim revisions are valid entity tags");
    let document = DataRootClaimDocument::inspect(Some(claim));
    ([(ETAG, etag)], Json(document)).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ClaimRequest {
    confirmation: String,
}
