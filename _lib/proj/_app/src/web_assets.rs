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
const CLAIM_CSS: &str = include_str!("../web/styles/claim.css");
const COMMAND_RUN_CSS: &str = include_str!("../web/styles/command-run.css");

const APP_JS: &str = include_str!("../web/app.js");
const CATALOG_MODEL_JS: &str = include_str!("../web/catalog-model.js");
const COMMAND_ACTIVITY_JS: &str = include_str!("../web/command-activity.js");
const NAVIGATION_JS: &str = include_str!("../web/navigation.js");
const EXPLORER_JS: &str = include_str!("../web/explorer.js");
const EXPLORER_MODEL_JS: &str = include_str!("../web/explorer-model.js");
const DETAIL_JS: &str = include_str!("../web/detail.js");
const ENTRY_PROFILE_JS: &str = include_str!("../web/entry-profile.js");
const HOST_CONTROL_JS: &str = include_str!("../web/host-control.js");
const CLAIM_JS: &str = include_str!("../web/claim.js");
const COMMAND_RUN_JS: &str = include_str!("../web/command-run.js");
const COMMAND_RUN_CLIENT_JS: &str = include_str!("../web/command-run-client.js");
const COMMAND_RUN_MODEL_JS: &str = include_str!("../web/command-run-model.js");
const COMMAND_RUN_OUTPUT_JS: &str = include_str!("../web/command-run-output.js");

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
        "styles/claim.css" => Some(("text/css; charset=utf-8", CLAIM_CSS)),
        "styles/command-run.css" => Some(("text/css; charset=utf-8", COMMAND_RUN_CSS)),
        "app.js" => Some(("text/javascript; charset=utf-8", APP_JS)),
        "catalog-model.js" => Some(("text/javascript; charset=utf-8", CATALOG_MODEL_JS)),
        "command-activity.js" => Some(("text/javascript; charset=utf-8", COMMAND_ACTIVITY_JS)),
        "navigation.js" => Some(("text/javascript; charset=utf-8", NAVIGATION_JS)),
        "explorer.js" => Some(("text/javascript; charset=utf-8", EXPLORER_JS)),
        "explorer-model.js" => Some(("text/javascript; charset=utf-8", EXPLORER_MODEL_JS)),
        "detail.js" => Some(("text/javascript; charset=utf-8", DETAIL_JS)),
        "entry-profile.js" => Some(("text/javascript; charset=utf-8", ENTRY_PROFILE_JS)),
        "host-control.js" => Some(("text/javascript; charset=utf-8", HOST_CONTROL_JS)),
        "claim.js" => Some(("text/javascript; charset=utf-8", CLAIM_JS)),
        "command-run.js" => Some(("text/javascript; charset=utf-8", COMMAND_RUN_JS)),
        "command-run-client.js" => Some(("text/javascript; charset=utf-8", COMMAND_RUN_CLIENT_JS)),
        "command-run-model.js" => Some(("text/javascript; charset=utf-8", COMMAND_RUN_MODEL_JS)),
        "command-run-output.js" => Some(("text/javascript; charset=utf-8", COMMAND_RUN_OUTPUT_JS)),
        _ => None,
    };

    let Some((content_type, body)) = asset else {
        return StatusCode::NOT_FOUND.into_response();
    };
    ([(CONTENT_TYPE, content_type)], body).into_response()
}
