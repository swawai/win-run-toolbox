use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use tokio::sync::watch;

use super::ServerState;
use crate::host_runtime::{HOST_BOOT_HEADER, HOST_ENTRY_HEADER};
use crate::runtime_control::HostStatusDocument;

const CONTROL_HEADER: &str = "x-swawkit-control";
const SHUTDOWN_COMMAND: &str = "shutdown";
const RESTART_COMMAND: &str = "restart";
const RUNNING: u8 = 0;
const PREPARING_RESTART: u8 = 1;
const STOPPING: u8 = 2;

#[derive(Clone)]
pub(super) struct HostControl {
    requested: watch::Sender<bool>,
    state: Arc<AtomicU8>,
}

enum ControlRequest {
    Accepted,
    AlreadyRequested,
    Busy,
}

impl HostControl {
    pub(super) fn new() -> Self {
        let (requested, _receiver) = watch::channel(false);
        Self {
            requested,
            state: Arc::new(AtomicU8::new(RUNNING)),
        }
    }

    fn request_shutdown(&self) -> ControlRequest {
        match self
            .state
            .compare_exchange(RUNNING, STOPPING, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                self.requested.send_replace(true);
                ControlRequest::Accepted
            }
            Err(PREPARING_RESTART) => ControlRequest::Busy,
            Err(_) => ControlRequest::AlreadyRequested,
        }
    }

    fn request_restart(
        &self,
        context: &crate::context::EntryContext,
    ) -> Result<ControlRequest, String> {
        match self.state.compare_exchange(
            RUNNING,
            PREPARING_RESTART,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {}
            Err(PREPARING_RESTART) => return Ok(ControlRequest::Busy),
            Err(_) => return Ok(ControlRequest::AlreadyRequested),
        }
        if let Err(error) = crate::host_restart::prepare(context) {
            self.state.store(RUNNING, Ordering::Release);
            return Err(error);
        }
        self.state.store(STOPPING, Ordering::Release);
        self.requested.send_replace(true);
        Ok(ControlRequest::Accepted)
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

pub(super) async fn get_host(State(state): State<ServerState>) -> Response {
    let selected_release_id = match crate::runtime_release::selected_release_id(&state.context) {
        Ok(release_id) => release_id,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Runtime Release status is unavailable: {error}\n"),
            )
                .into_response();
        }
    };
    match HostStatusDocument::new(
        &state.host_runtime,
        state.context.release_id,
        selected_release_id,
    ) {
        Ok(document) => Json(document).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot compose Host runtime status: {error}\n"),
        )
            .into_response(),
    }
}

pub(super) async fn post_shutdown(
    State(state): State<ServerState>,
    headers: HeaderMap,
) -> Response {
    if !has_control_header(&headers, SHUTDOWN_COMMAND) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match state.host_control.request_shutdown() {
        ControlRequest::Accepted => StatusCode::ACCEPTED,
        ControlRequest::AlreadyRequested => StatusCode::NO_CONTENT,
        ControlRequest::Busy => StatusCode::CONFLICT,
    }
    .into_response()
}

pub(super) async fn post_restart(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    if !has_control_header(&headers, RESTART_COMMAND) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let selected_release_id = match crate::runtime_release::selected_release_id(&state.context) {
        Ok(release_id) => release_id,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Runtime Release status is unavailable: {error}\n"),
            )
                .into_response();
        }
    };
    if selected_release_id == state.context.release_id {
        return (
            StatusCode::CONFLICT,
            "the Host already runs the selected Runtime Release\n",
        )
            .into_response();
    }

    let host_control = state.host_control;
    let context = state.context;
    match tokio::task::spawn_blocking(move || host_control.request_restart(&context)).await {
        Ok(Ok(ControlRequest::Accepted)) => StatusCode::ACCEPTED.into_response(),
        Ok(Ok(ControlRequest::AlreadyRequested)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Ok(ControlRequest::Busy)) => StatusCode::CONFLICT.into_response(),
        Ok(Err(error)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot prepare Host restart: {error}\n"),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Host restart task failed: {error}\n"),
        )
            .into_response(),
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
