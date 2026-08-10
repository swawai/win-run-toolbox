use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    thread,
};

use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::{
        HeaderMap, HeaderName, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, ETAG, HOST, IF_MATCH},
    },
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, sync::oneshot};

use crate::{
    catalog::CatalogSnapshot,
    catalog_reader::CatalogReader,
    context::EntryContext,
    data_root::{DataRootSession, DataRootSessionState},
    host_runtime::{HostRuntimeDocument, HostRuntimeIdentity},
    profile::{EntryProfileDocument, EntryProfileStore, ProfileUpdateError},
    web_assets,
};

mod claim;
mod command_run;
mod host_control;

use command_run::CommandRuns;
use host_control::HostControl;

#[derive(Clone)]
struct ServerState {
    context: EntryContext,
    data_root: DataRootSession,
    command_runs: CommandRuns,
    host_control: HostControl,
    host_runtime: HostRuntimeDocument,
}

#[derive(Debug)]
pub enum ServerEvent {
    Ready(HostRuntimeDocument),
    Stopped(Result<(), String>),
}

pub fn spawn<F>(
    context: EntryContext,
    data_root: DataRootSession,
    host_runtime: HostRuntimeIdentity,
    notify: F,
    shutdown: oneshot::Receiver<()>,
) -> io::Result<thread::JoinHandle<()>>
where
    F: Fn(ServerEvent) -> Result<(), String> + Send + 'static,
{
    thread::Builder::new()
        .name("swawkit-web".to_owned())
        .spawn(move || {
            let result = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(run_server(
                    context,
                    data_root,
                    host_runtime,
                    |document| notify(ServerEvent::Ready(document)),
                    shutdown,
                )),
                Err(error) => Err(error.to_string()),
            };

            let _ = notify(ServerEvent::Stopped(result));
        })
}

async fn run_server<F>(
    context: EntryContext,
    data_root: DataRootSession,
    host_runtime: HostRuntimeIdentity,
    notify_ready: F,
    shutdown: oneshot::Receiver<()>,
) -> Result<(), String>
where
    F: FnOnce(HostRuntimeDocument) -> Result<(), String>,
{
    let listener = bind_loopback().await.map_err(|error| error.to_string())?;
    let address = listener.local_addr().map_err(|error| error.to_string())?;
    let authority = address.to_string();
    let url = format!("http://{authority}/");
    let host_runtime = host_runtime
        .document(url)
        .map_err(|error| error.to_string())?;

    notify_ready(host_runtime.clone())?;

    let command_runs = CommandRuns::native();
    let host_control = HostControl::new();
    let shutdown_control = host_control.clone();
    let serve_result = axum::serve(
        listener,
        router_with_runs(
            authority,
            context,
            data_root,
            command_runs.clone(),
            host_runtime,
            host_control,
        ),
    )
    .with_graceful_shutdown(async move {
        tokio::select! {
            _ = shutdown => {}
            _ = shutdown_control.wait() => {}
        }
    })
    .await
    .map_err(|error| error.to_string());
    let shutdown_result = tokio::task::spawn_blocking(move || command_runs.shutdown())
        .await
        .map_err(|error| format!("command worker shutdown failed: {error}"))
        .and_then(|result| result);

    match (serve_result, shutdown_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(serve_error), Err(shutdown_error)) => Err(format!("{serve_error}; {shutdown_error}")),
    }
}

async fn bind_loopback() -> io::Result<TcpListener> {
    TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await
}

#[cfg(test)]
fn router(expected_authority: String, context: EntryContext, data_root: DataRootSession) -> Router {
    let host_runtime = HostRuntimeDocument::new(
        "0".repeat(64),
        "test-host",
        std::process::id(),
        format!("http://{expected_authority}/"),
    )
    .expect("test Host runtime");
    router_with_runs(
        expected_authority,
        context,
        data_root,
        CommandRuns::native(),
        host_runtime,
        HostControl::new(),
    )
}

fn router_with_runs(
    expected_authority: String,
    context: EntryContext,
    data_root: DataRootSession,
    command_runs: CommandRuns,
    host_runtime: HostRuntimeDocument,
    host_control: HostControl,
) -> Router {
    Router::new()
        .route("/", get(web_assets::index))
        .route("/commands", get(web_assets::index))
        .route("/commands/{*path}", get(web_assets::index))
        .route("/assets/{*path}", get(web_assets::asset))
        .route("/api/v2/catalog", get(get_catalog))
        .route(
            "/api/v2/data-root/claim",
            get(claim::get_claim).post(claim::post_claim),
        )
        .route("/api/v2/profile", get(get_profile))
        .route("/api/v2/host", get(host_control::get_host))
        .route(
            "/api/v2/host/shutdown",
            axum::routing::post(host_control::post_shutdown),
        )
        .route(
            "/api/v2/command-runs",
            axum::routing::post(command_run::post_command_run),
        )
        .route(
            "/api/v2/command-runs/{id}",
            get(command_run::get_command_run).delete(command_run::delete_command_run),
        )
        .route(
            "/api/v2/profile/variables/{name}",
            axum::routing::put(put_profile_variable),
        )
        .route("/healthz", get(host_control::health))
        .layer(middleware::from_fn(security_headers))
        .layer(middleware::from_fn_with_state(
            Arc::<str>::from(expected_authority),
            enforce_authority,
        ))
        .with_state(ServerState {
            context,
            data_root,
            command_runs,
            host_control,
            host_runtime,
        })
}

async fn enforce_authority(
    State(expected_authority): State<Arc<str>>,
    request: Request,
    next: Next,
) -> Response {
    let matches = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case(&expected_authority));

    if !matches {
        return (StatusCode::MISDIRECTED_REQUEST, "misdirected request\n").into_response();
    }

    next.run(request).await
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; connect-src 'self'; \
             style-src 'self'; img-src 'self'; \
             base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        ),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn get_catalog(
    State(state): State<ServerState>,
) -> Result<Json<CatalogSnapshot>, (StatusCode, Json<ApiError>)> {
    let profile_store = ready_profile_store(&state).await?;
    CatalogReader::new(state.context, profile_store)
        .read()
        .await
        .map(Json)
        .map_err(|_| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "catalog discovery failed",
            )
        })
}

#[derive(Debug, Serialize)]
struct ApiError {
    error: String,
}

async fn get_profile(State(state): State<ServerState>) -> Response {
    let profile_store = match ready_profile_store(&state).await {
        Ok(profile_store) => profile_store,
        Err(error) => return error.into_response(),
    };
    let document = match tokio::task::spawn_blocking(move || profile_store.document()).await {
        Ok(document) => document,
        Err(error) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("entry profile worker failed: {error}"),
            )
            .into_response();
        }
    };
    profile_response(document)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileVariableUpdate {
    value: String,
}

async fn put_profile_variable(
    State(state): State<ServerState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(update): Json<ProfileVariableUpdate>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let expected_revision = expected_revision(&headers, "entry profile")?.to_owned();
    let profile_store = ready_profile_store(&state).await?;
    let update = tokio::task::spawn_blocking(move || {
        profile_store.update_environment_variable_if_revision(
            &expected_revision,
            &name,
            update.value,
        )
    })
    .await
    .map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("entry profile worker failed: {error}"),
        )
    })?;
    match update {
        Ok(document) => Ok(profile_response(document)),
        Err(ProfileUpdateError::Conflict { .. }) => Err(api_error(
            StatusCode::CONFLICT,
            "entry profile changed since it was loaded; reload before saving again",
        )),
        Err(ProfileUpdateError::Profile(error)) => Err(api_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            error.to_string(),
        )),
    }
}

fn expected_revision<'a>(
    headers: &'a HeaderMap,
    subject: &str,
) -> Result<&'a str, (StatusCode, Json<ApiError>)> {
    let value = headers.get(IF_MATCH).ok_or_else(|| {
        api_error(
            StatusCode::PRECONDITION_REQUIRED,
            format!("If-Match with the loaded {subject} revision is required"),
        )
    })?;
    let value = value.to_str().map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            format!("If-Match must contain one strong {subject} revision"),
        )
    })?;
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .filter(|value| !value.is_empty() && !value.contains('"') && !value.contains(','))
        .ok_or_else(|| {
            api_error(
                StatusCode::BAD_REQUEST,
                format!("If-Match must contain one quoted strong {subject} revision"),
            )
        })
}

fn profile_response(document: EntryProfileDocument) -> Response {
    let etag = HeaderValue::from_str(&format!("\"{}\"", document.revision))
        .expect("profile revisions are valid entity tags");
    ([(ETAG, etag)], Json(document)).into_response()
}

fn api_error(status: StatusCode, error: impl Into<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: error.into(),
        }),
    )
}

async fn data_root_status(
    state: &ServerState,
) -> Result<DataRootSessionState, (StatusCode, Json<ApiError>)> {
    let data_root = state.data_root.clone();
    tokio::task::spawn_blocking(move || data_root.status())
        .await
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("DataRoot worker failed: {error}"),
            )
        })?
        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

async fn ready_profile_store(
    state: &ServerState,
) -> Result<EntryProfileStore, (StatusCode, Json<ApiError>)> {
    match data_root_status(state).await? {
        DataRootSessionState::Ready(resolved) => Ok(EntryProfileStore::new(
            &state.context.swawkit_home,
            resolved.path(),
        )),
        DataRootSessionState::ClaimRequired(_) => Err(api_error(
            StatusCode::CONFLICT,
            "DataRoot ownership claim is required",
        )),
    }
}

#[cfg(test)]
mod tests;
