use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use super::ServerState;

const CLEANUP_PREVIEW: &str = "runtime-cleanup-preview";
const CLEANUP_APPLY: &str = "runtime-cleanup-apply";

pub(super) async fn get_runtime(State(state): State<ServerState>) -> Response {
    match crate::runtime_control::inspect_running(&state.context, &state.host_runtime) {
        Ok(document) => Json(document).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Runtime status is unavailable: {error}\n"),
        )
            .into_response(),
    }
}

pub(super) async fn post_cleanup(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    let apply = if super::host_control::has_control_header(&headers, CLEANUP_PREVIEW) {
        false
    } else if super::host_control::has_control_header(&headers, CLEANUP_APPLY) {
        true
    } else {
        return StatusCode::FORBIDDEN.into_response();
    };
    match tokio::task::spawn_blocking(move || {
        crate::runtime_cleanup::execute_json(&state.context, apply)
    })
    .await
    {
        Ok(Ok(document)) => Json(document).into_response(),
        Ok(Err(error)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Runtime cleanup failed: {error}\n"),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Runtime cleanup worker failed: {error}\n"),
        )
            .into_response(),
    }
}
