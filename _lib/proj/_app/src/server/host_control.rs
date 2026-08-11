use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use tokio::sync::watch;

use super::ServerState;
use crate::host_runtime::{HOST_BOOT_HEADER, HOST_ENTRY_HEADER};

const CONTROL_HEADER: &str = "x-swawkit-control";
const SHUTDOWN_COMMAND: &str = "shutdown";

#[derive(Clone)]
pub(super) struct HostControl {
    requested: watch::Sender<bool>,
}

impl HostControl {
    pub(super) fn new() -> Self {
        let (requested, _receiver) = watch::channel(false);
        Self { requested }
    }

    fn request(&self) -> bool {
        !self.requested.send_replace(true)
    }

    pub(super) async fn wait(&self) {
        let mut requested = self.requested.subscribe();
        loop {
            if *requested.borrow() {
                return;
            }
            if requested.changed().await.is_err() {
                return;
            }
        }
    }
}

pub(super) async fn get_host(State(state): State<ServerState>) -> impl IntoResponse {
    Json(state.host_runtime)
}

pub(super) async fn post_shutdown(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> StatusCode {
    if !has_control_header(&headers, SHUTDOWN_COMMAND) {
        return StatusCode::FORBIDDEN;
    }
    if state.host_control.request() {
        StatusCode::ACCEPTED
    } else {
        StatusCode::NO_CONTENT
    }
}

pub(super) fn has_control_header(headers: &HeaderMap, command: &str) -> bool {
    headers
        .get(CONTROL_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == command)
}

pub(super) async fn health(State(state): State<ServerState>) -> Response {
    let mut response = "ok\n".into_response();
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static(HOST_BOOT_HEADER),
        HeaderValue::from_str(&state.host_runtime.boot_id).expect("validated Host boot ID"),
    );
    headers.insert(
        HeaderName::from_static(HOST_ENTRY_HEADER),
        HeaderValue::from_str(&state.host_runtime.entry_key_sha256)
            .expect("validated Host Entry identity"),
    );
    response
}
