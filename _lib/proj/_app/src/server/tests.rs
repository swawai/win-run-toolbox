use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, header::CONTENT_TYPE},
};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::{
    context::EntryContext,
    data_root::{DataRootClaim, DataRootSession, ResolveDataRootRequest, resolve_data_root},
    profile::EntryProfileStore,
};

mod claim;
mod command_run;
mod command_run_native;
mod profile;
mod runtime;

const AUTHORITY: &str = "127.0.0.1:43127";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("swawkit-server-{}-{sequence}", std::process::id()));
        fs::create_dir_all(root.join("home")).expect("create fixture root");
        fs::write(root.join("swawkit.exe"), b"fixture").expect("create fixture entry");
        let fixture = Self { root };
        let context = fixture.context();
        let mut approve = |_claim: &DataRootClaim| Ok(true);
        resolve_data_root(
            ResolveDataRootRequest {
                swawkit_home: &context.swawkit_home,
                entry_file: &context.entry_file,
            },
            &mut approve,
        )
        .expect("bind fixture DataRoot");
        fixture
    }

    fn directory(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(&path).expect("create fixture directory");
        path
    }

    fn file(&self, relative: &str, text: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().expect("fixture file parent"))
            .expect("create fixture file parent");
        fs::write(path, text).expect("write fixture file");
    }

    fn context(&self) -> EntryContext {
        EntryContext {
            swawkit_home: self.root.join("home"),
            entry_file: self.root.join("swawkit.exe"),
            entry_name: "swawkit".to_owned(),
            invocation_directory: self.root.clone(),
        }
    }

    fn data_root_session(&self) -> DataRootSession {
        let context = self.context();
        DataRootSession::new(ResolveDataRootRequest {
            swawkit_home: &context.swawkit_home,
            entry_file: &context.entry_file,
        })
        .expect("pin fixture Entry for DataRoot session")
    }

    fn replace_entry(&self, content: &[u8]) {
        let path = self.context().entry_file;
        fs::remove_file(&path).expect("remove fixture entry");
        fs::write(path, content).expect("replace fixture entry");
    }

    fn profile_store(&self) -> EntryProfileStore {
        let data_root = self.root.join("home/data/proj.swawkit");
        fs::create_dir_all(&data_root).expect("create fixture DataRoot");
        EntryProfileStore::new(self.root.join("home"), data_root)
    }

    fn app(&self) -> Router {
        router(
            AUTHORITY.to_owned(),
            self.context(),
            self.data_root_session(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

async fn send(app: Router, method: Method, path: &str, authority: Option<&str>) -> Response {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(authority) = authority {
        builder = builder.header(HOST, authority);
    }

    app.oneshot(builder.body(Body::empty()).expect("valid request"))
        .await
        .expect("router response")
}

async fn catalog_document(app: Router) -> Value {
    let response = send(app, Method::GET, "/api/v2/catalog", Some(AUTHORITY)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("catalog body");
    serde_json::from_slice(&body).expect("catalog JSON")
}

#[tokio::test]
async fn serves_only_the_declared_local_surface() {
    let fixture = Fixture::new();
    fixture.directory("home/_lib/proj");
    let app = fixture.app();

    let index = send(app.clone(), Method::GET, "/", Some(AUTHORITY)).await;
    assert_eq!(index.status(), StatusCode::OK);
    assert_eq!(
        index.headers().get(CACHE_CONTROL),
        Some(&HeaderValue::from_static("no-store"))
    );
    assert_eq!(
        index
            .headers()
            .get(HeaderName::from_static("x-content-type-options")),
        Some(&HeaderValue::from_static("nosniff"))
    );
    assert_eq!(
        index.headers().get(CONTENT_TYPE),
        Some(&HeaderValue::from_static("text/html; charset=utf-8"))
    );
    let body = to_bytes(index.into_body(), usize::MAX)
        .await
        .expect("index body");
    let index_html = String::from_utf8_lossy(&body);
    assert!(index_html.contains("Swaw Kit Proj"));
    assert!(index_html.contains("class=\"command-run-output\" id=\"command-run-output\""));

    for (path, content_type) in [
        ("/assets/app.css", "text/css; charset=utf-8"),
        ("/assets/styles/theme.css", "text/css; charset=utf-8"),
        ("/assets/styles/base.css", "text/css; charset=utf-8"),
        ("/assets/styles/shell.css", "text/css; charset=utf-8"),
        ("/assets/styles/explorer.css", "text/css; charset=utf-8"),
        ("/assets/styles/detail.css", "text/css; charset=utf-8"),
        (
            "/assets/styles/entry-profile.css",
            "text/css; charset=utf-8",
        ),
        ("/assets/styles/claim.css", "text/css; charset=utf-8"),
        ("/assets/styles/command-run.css", "text/css; charset=utf-8"),
        ("/assets/app.js", "text/javascript; charset=utf-8"),
        ("/assets/catalog-model.js", "text/javascript; charset=utf-8"),
        ("/assets/explorer.js", "text/javascript; charset=utf-8"),
        ("/assets/detail.js", "text/javascript; charset=utf-8"),
        ("/assets/entry-profile.js", "text/javascript; charset=utf-8"),
        ("/assets/claim.js", "text/javascript; charset=utf-8"),
        (
            "/assets/command-run-client.js",
            "text/javascript; charset=utf-8",
        ),
        (
            "/assets/command-run-output.js",
            "text/javascript; charset=utf-8",
        ),
        (
            "/assets/command-run-model.js",
            "text/javascript; charset=utf-8",
        ),
        ("/assets/command-run.js", "text/javascript; charset=utf-8"),
    ] {
        let response = send(app.clone(), Method::GET, path, Some(AUTHORITY)).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            response.headers().get(CONTENT_TYPE),
            Some(&HeaderValue::from_static(content_type)),
            "{path}"
        );
    }
    assert_eq!(
        send(
            app.clone(),
            Method::GET,
            "/assets/not-published.js",
            Some(AUTHORITY)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let document = catalog_document(app.clone()).await;
    assert_eq!(document["protocol"], crate::catalog::CATALOG_PROTOCOL);
    assert_eq!(document["entryName"], "swawkit");
    assert_eq!(document["commands"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        send(app.clone(), Method::GET, "/healthz", Some(AUTHORITY))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(app.clone(), Method::POST, "/", Some(AUTHORITY))
            .await
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        send(app.clone(), Method::GET, "/api/v1/run", Some(AUTHORITY))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(app.clone(), Method::GET, "/api/v1/profile", Some(AUTHORITY))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(app, Method::GET, "/_lib/proj/run.ps1", Some(AUTHORITY))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn rescans_the_catalog_on_each_request() {
    let fixture = Fixture::new();
    fixture.directory("home/_lib/proj");
    let app = fixture.app();

    let before = catalog_document(app.clone()).await;
    assert!(command(&before, ".dynamic").is_none());

    fixture.file("home/_lib/proj/.dynamic/run.ps1", "");
    let after = catalog_document(app).await;
    assert_eq!(
        command(&after, ".dynamic").and_then(|node| node["runnable"].as_bool()),
        Some(true)
    );
}

#[tokio::test]
async fn returns_a_safe_error_when_catalog_discovery_fails() {
    let fixture = Fixture::new();
    let response = send(
        fixture.app(),
        Method::GET,
        "/api/v2/catalog",
        Some(AUTHORITY),
    )
    .await;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("error body");
    let document: Value = serde_json::from_slice(&body).expect("error JSON");
    assert_eq!(document["error"], "catalog discovery failed");
}

#[tokio::test]
async fn serializes_the_complete_catalog_node_contract() {
    let fixture = Fixture::new();
    fixture.directory("home/_lib/proj");
    fixture.file("home/_lib/proj/.dev/status/run.cmd", "");
    fixture.file(
        "home/_lib/proj/.dev/_view/web.json",
        r#"{"schema":"swawkit.command-view/web/v1","childrenColumn":{"width":"wide"}}"#,
    );
    fixture.file(
        "home/_lib/proj/.dev/status/_help/zh-CN.txt",
        "Show {{ADDRESS}}\nUse {{INVOCATION}}",
    );
    fixture.file("home/_lib/proj/.help/run.ps1", "");
    fixture.file("home/_lib/proj/.h/run.ps1", "");
    fixture.file("home/_lib/proj/.broken/run.ps1", "");
    fixture.file("home/_lib/proj/.broken/run.cmd", "");

    let document = catalog_document(fixture.app()).await;
    assert_eq!(
        command(&document, ".dev").expect("group node"),
        &json!({
            "address": ".dev",
            "source": "kernel",
            "parent": "",
            "aliasOf": null,
            "runnable": false,
            "entry": null,
            "adapter": null,
            "handler": null,
            "help": null,
            "view": {
                "childrenColumn": {
                    "width": "wide"
                }
            },
            "diagnostic": null
        })
    );
    assert_eq!(
        command(&document, ".dev.status").expect("runnable node"),
        &json!({
            "address": ".dev.status",
            "source": "kernel",
            "parent": ".dev",
            "aliasOf": null,
            "runnable": true,
            "entry": "run.cmd",
            "adapter": "cmd",
            "handler": null,
            "help": {
                "summary": "Show .dev.status",
                "text": "Show .dev.status\nUse swawkit .dev.status"
            },
            "view": null,
            "diagnostic": null
        })
    );
    assert_eq!(
        command(&document, ".h").and_then(|node| node["aliasOf"].as_str()),
        Some(".help")
    );
    assert!(
        command(&document, ".broken")
            .and_then(|node| node["diagnostic"].as_str())
            .is_some_and(|message| message.contains("multiple run entries"))
    );
    assert!(
        document["commands"]
            .as_array()
            .expect("commands array")
            .iter()
            .all(|node| node.get("directory").is_none())
    );
}

#[tokio::test]
async fn rejects_missing_or_foreign_host_headers() {
    let fixture = Fixture::new();
    fixture.directory("home/_lib/proj");
    let app = fixture.app();

    assert_eq!(
        send(app.clone(), Method::GET, "/", None).await.status(),
        StatusCode::MISDIRECTED_REQUEST
    );
    assert_eq!(
        send(app, Method::GET, "/", Some("attacker.example"))
            .await
            .status(),
        StatusCode::MISDIRECTED_REQUEST
    );
}

fn command<'a>(document: &'a Value, address: &str) -> Option<&'a Value> {
    document["commands"]
        .as_array()?
        .iter()
        .find(|node| node["address"] == address)
}
