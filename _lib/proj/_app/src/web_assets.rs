use axum::{
    extract::Path,
    http::{StatusCode, header::CONTENT_TYPE},
    response::{Html, IntoResponse, Response},
};

const INDEX_HTML: &str = include_str!("../web/index.html");

const APP_CSS: &str = include_str!("../web/app.css");
const THEME_CSS: &str = include_str!("../web/styles/theme.css");
const BASE_CSS: &str = include_str!("../web/styles/base.css");
const SHELL_CSS: &str = include_str!("../web/styles/shell.css");
const EXPLORER_CSS: &str = include_str!("../web/styles/explorer.css");
const DETAIL_CSS: &str = include_str!("../web/styles/detail.css");
const ENTRY_PROFILE_CSS: &str = include_str!("../web/styles/entry-profile.css");
const RUNTIME_CONTROL_CSS: &str = include_str!("../web/styles/runtime-control.css");
const CLAIM_CSS: &str = include_str!("../web/styles/claim.css");
const COMMAND_RUN_CSS: &str = include_str!("../web/styles/command-run.css");
const RUN_PROJECTION_CSS: &str = include_str!("../web/styles/run-projection.css");
const CONTEXT_PROJECTION_CSS: &str = include_str!("../web/styles/context-projection.css");

const APP_JS: &str = include_str!("../web/app.js");
const I18N_JS: &str = include_str!("../web/i18n.js");
const CATALOG_MODEL_JS: &str = include_str!("../web/catalog-model.js");
const FACET_MODEL_JS: &str = include_str!("../web/facet-model.js");
const FACET_RESOLUTION_CLIENT_JS: &str = include_str!("../web/facet-resolution-client.js");
const COMMAND_EVENT_CLIENT_JS: &str = include_str!("../web/command-event-client.js");
const NAVIGATION_JS: &str = include_str!("../web/navigation.js");
const EXPLORER_JS: &str = include_str!("../web/explorer.js");
const EXPLORER_MODEL_JS: &str = include_str!("../web/explorer-model.js");
const DETAIL_JS: &str = include_str!("../web/detail.js");
const DOCUMENT_PROJECTION_JS: &str = include_str!("../web/document-projection.js");
const ENTRY_PROFILE_JS: &str = include_str!("../web/entry-profile.js");
const RUNTIME_CONTROL_JS: &str = include_str!("../web/runtime-control.js");
const CLAIM_JS: &str = include_str!("../web/claim.js");
const COMMAND_RUN_JS: &str = include_str!("../web/command-run.js");
const COMMAND_RUN_OPERATIONS_JS: &str = include_str!("../web/command-run-operations.js");
const COMMAND_RUN_CLIENT_JS: &str = include_str!("../web/command-run-client.js");
const COMMAND_RUN_MODEL_JS: &str = include_str!("../web/command-run-model.js");
const COMMAND_RUN_OUTPUT_JS: &str = include_str!("../web/command-run-output.js");
const RUN_PROJECTION_MODEL_JS: &str = include_str!("../web/run-projection-model.js");
const RUN_PROJECTION_JS: &str = include_str!("../web/run-projection.js");
const CONTEXT_PROJECTION_MODEL_JS: &str = include_str!("../web/context-projection-model.js");
const CONTEXT_PROJECTION_JS: &str = include_str!("../web/context-projection.js");
const SUBJECT_COLLECTION_MODEL_JS: &str = include_str!("../web/subject-collection-model.js");
const SUBJECT_KIND_MODEL_JS: &str = include_str!("../web/subject-kind-model.js");
const SUBJECT_EXPLORER_JS: &str = include_str!("../web/subject-explorer.js");
const SUBJECT_FACET_JS: &str = include_str!("../web/subject-facet.js");

pub(crate) async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

pub(crate) async fn asset(Path(path): Path<String>) -> Response {
    let asset = match path.as_str() {
        "app.css" => Some(("text/css; charset=utf-8", APP_CSS)),
        "styles/theme.css" => Some(("text/css; charset=utf-8", THEME_CSS)),
        "styles/base.css" => Some(("text/css; charset=utf-8", BASE_CSS)),
        "styles/shell.css" => Some(("text/css; charset=utf-8", SHELL_CSS)),
        "styles/explorer.css" => Some(("text/css; charset=utf-8", EXPLORER_CSS)),
        "styles/detail.css" => Some(("text/css; charset=utf-8", DETAIL_CSS)),
        "styles/entry-profile.css" => Some(("text/css; charset=utf-8", ENTRY_PROFILE_CSS)),
        "styles/runtime-control.css" => Some(("text/css; charset=utf-8", RUNTIME_CONTROL_CSS)),
        "styles/claim.css" => Some(("text/css; charset=utf-8", CLAIM_CSS)),
        "styles/command-run.css" => Some(("text/css; charset=utf-8", COMMAND_RUN_CSS)),
        "styles/run-projection.css" => Some(("text/css; charset=utf-8", RUN_PROJECTION_CSS)),
        "styles/context-projection.css" => {
            Some(("text/css; charset=utf-8", CONTEXT_PROJECTION_CSS))
        }
        "app.js" => Some(("text/javascript; charset=utf-8", APP_JS)),
        "i18n.js" => Some(("text/javascript; charset=utf-8", I18N_JS)),
        "catalog-model.js" => Some(("text/javascript; charset=utf-8", CATALOG_MODEL_JS)),
        "facet-model.js" => Some(("text/javascript; charset=utf-8", FACET_MODEL_JS)),
        "facet-resolution-client.js" => {
            Some(("text/javascript; charset=utf-8", FACET_RESOLUTION_CLIENT_JS))
        }
        "command-event-client.js" => {
            Some(("text/javascript; charset=utf-8", COMMAND_EVENT_CLIENT_JS))
        }
        "navigation.js" => Some(("text/javascript; charset=utf-8", NAVIGATION_JS)),
        "explorer.js" => Some(("text/javascript; charset=utf-8", EXPLORER_JS)),
        "explorer-model.js" => Some(("text/javascript; charset=utf-8", EXPLORER_MODEL_JS)),
        "detail.js" => Some(("text/javascript; charset=utf-8", DETAIL_JS)),
        "document-projection.js" => {
            Some(("text/javascript; charset=utf-8", DOCUMENT_PROJECTION_JS))
        }
        "entry-profile.js" => Some(("text/javascript; charset=utf-8", ENTRY_PROFILE_JS)),
        "runtime-control.js" => Some(("text/javascript; charset=utf-8", RUNTIME_CONTROL_JS)),
        "claim.js" => Some(("text/javascript; charset=utf-8", CLAIM_JS)),
        "command-run.js" => Some(("text/javascript; charset=utf-8", COMMAND_RUN_JS)),
        "command-run-operations.js" => {
            Some(("text/javascript; charset=utf-8", COMMAND_RUN_OPERATIONS_JS))
        }
        "command-run-client.js" => Some(("text/javascript; charset=utf-8", COMMAND_RUN_CLIENT_JS)),
        "command-run-model.js" => Some(("text/javascript; charset=utf-8", COMMAND_RUN_MODEL_JS)),
        "command-run-output.js" => Some(("text/javascript; charset=utf-8", COMMAND_RUN_OUTPUT_JS)),
        "run-projection-model.js" => {
            Some(("text/javascript; charset=utf-8", RUN_PROJECTION_MODEL_JS))
        }
        "run-projection.js" => Some(("text/javascript; charset=utf-8", RUN_PROJECTION_JS)),
        "context-projection-model.js" => Some((
            "text/javascript; charset=utf-8",
            CONTEXT_PROJECTION_MODEL_JS,
        )),
        "context-projection.js" => Some(("text/javascript; charset=utf-8", CONTEXT_PROJECTION_JS)),
        "subject-collection-model.js" => Some((
            "text/javascript; charset=utf-8",
            SUBJECT_COLLECTION_MODEL_JS,
        )),
        "subject-kind-model.js" => Some(("text/javascript; charset=utf-8", SUBJECT_KIND_MODEL_JS)),
        "subject-explorer.js" => Some(("text/javascript; charset=utf-8", SUBJECT_EXPLORER_JS)),
        "subject-facet.js" => Some(("text/javascript; charset=utf-8", SUBJECT_FACET_JS)),
        _ => None,
    };

    let Some((content_type, body)) = asset else {
        return StatusCode::NOT_FOUND.into_response();
    };
    ([(CONTENT_TYPE, content_type)], body).into_response()
}
