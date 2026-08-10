use std::ffi::OsString;
use std::io;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, header::LOCATION};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::entry_runner::{
    EntryOutputStream, EntryRunControl, EntryRunObserver, EntryRunOutcome, EntryRunSpec,
    EntryRunner,
};
use crate::profile::EntryProfileRecord;
use crate::server::command_run::CommandRuns;

#[derive(Default)]
struct FakeRunner {
    specs: Mutex<Vec<EntryRunSpec>>,
    runs: Mutex<Vec<Arc<FakeRun>>>,
}

impl FakeRunner {
    fn specs(&self) -> Vec<EntryRunSpec> {
        self.specs.lock().expect("fake specs").clone()
    }

    fn run(&self, index: usize) -> Arc<FakeRun> {
        Arc::clone(&self.runs.lock().expect("fake runs")[index])
    }
}

impl EntryRunner for FakeRunner {
    fn start(
        &self,
        spec: EntryRunSpec,
        observer: Arc<dyn EntryRunObserver>,
    ) -> io::Result<Arc<dyn EntryRunControl>> {
        let run = Arc::new(FakeRun {
            observer: Mutex::new(Some(observer)),
            canceled: AtomicBool::new(false),
            joined: AtomicBool::new(false),
        });
        self.specs.lock().expect("fake specs").push(spec);
        self.runs.lock().expect("fake runs").push(Arc::clone(&run));
        let control: Arc<dyn EntryRunControl> = run;
        Ok(control)
    }
}

struct FakeRun {
    observer: Mutex<Option<Arc<dyn EntryRunObserver>>>,
    canceled: AtomicBool,
    joined: AtomicBool,
}

impl FakeRun {
    fn output(&self, stream: EntryOutputStream, text: &str) {
        let observer = self.observer.lock().expect("fake observer").clone();
        if let Some(observer) = observer {
            observer.output(stream, text.to_owned());
        }
    }

    fn complete(&self, outcome: EntryRunOutcome) {
        let observer = self.observer.lock().expect("fake observer").take();
        if let Some(observer) = observer {
            observer.completed(outcome);
        }
    }
}

impl EntryRunControl for FakeRun {
    fn cancel(&self) -> io::Result<()> {
        self.canceled.store(true, Ordering::Release);
        self.complete(EntryRunOutcome::Exited(1223));
        Ok(())
    }

    fn join(&self) -> Result<(), String> {
        self.joined.store(true, Ordering::Release);
        Ok(())
    }
}

fn ready_fixture(fixture: &Fixture) {
    fixture.directory("home/_lib/proj");
    fixture.file("home/_lib/proj/.demo/run.ps1", "");
    fixture
        .profile_store()
        .save(EntryProfileRecord::default())
        .expect("save ready fixture profile");
}

fn command_app(fixture: &Fixture, runner: Arc<FakeRunner>) -> (Router, CommandRuns) {
    let runner: Arc<dyn EntryRunner> = runner;
    let runs = CommandRuns::new(runner);
    let app = router_with_runs(
        AUTHORITY.to_owned(),
        fixture.context(),
        fixture.data_root_session(),
        runs.clone(),
        test_host_runtime(),
        HostControl::new(),
    );
    (app, runs)
}

async fn post_run(app: Router, document: Value) -> Response {
    app.oneshot(
        Request::builder()
            .method(Method::POST)
            .uri("/api/v2/command-runs")
            .header(HOST, AUTHORITY)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(document.to_string()))
            .expect("valid command run request"),
    )
    .await
    .expect("command run response")
}

async fn response_json(response: Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("command run response body");
    serde_json::from_slice(&body).expect("command run response JSON")
}

#[tokio::test]
async fn publishes_the_contract_and_incremental_output_cursor() {
    let fixture = Fixture::new();
    ready_fixture(&fixture);
    let runner = Arc::new(FakeRunner::default());
    let (app, runs) = command_app(&fixture, Arc::clone(&runner));

    let response = post_run(
        app.clone(),
        json!({"address": ".demo", "arguments": ["alpha"]}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let location = response
        .headers()
        .get(LOCATION)
        .expect("command run Location")
        .to_str()
        .expect("command run Location text")
        .to_owned();
    let created = response_json(response).await;
    let id = created["id"].as_str().expect("command run id");
    assert_eq!(location, format!("/api/v2/command-runs/{id}"));
    assert_eq!(created["protocol"], "swawkit.command-run/v1");
    assert_eq!(created["address"], ".demo");
    assert_eq!(created["state"], "running");
    assert_eq!(created["exitCode"], Value::Null);
    assert_eq!(created["error"], Value::Null);
    assert_eq!(created["nextCursor"], 0);
    assert_eq!(created["events"], json!([]));
    assert_eq!(created["truncated"], false);
    assert!(created.get("arguments").is_none());

    let specs = runner.specs();
    let spec = &specs[0];
    assert_eq!(spec.id, id);
    assert_eq!(spec.entry_file, fixture.context().entry_file);
    assert_eq!(spec.working_directory, fixture.root.join("home"));
    assert_eq!(
        spec.argv,
        [OsString::from(".demo"), OsString::from("alpha")]
    );

    let run = runner.run(0);
    run.output(EntryOutputStream::Stdout, "first\n");
    run.output(EntryOutputStream::Stderr, "second\n");
    let all = response_json(
        send(
            app.clone(),
            Method::GET,
            &format!("{location}?after=0"),
            Some(AUTHORITY),
        )
        .await,
    )
    .await;
    assert_eq!(all["nextCursor"], 2);
    assert_eq!(
        all["events"],
        json!([
            {"sequence": 1, "stream": "stdout", "text": "first\n"},
            {"sequence": 2, "stream": "stderr", "text": "second\n"}
        ])
    );

    let incremental = response_json(
        send(
            app.clone(),
            Method::GET,
            &format!("{location}?after=1"),
            Some(AUTHORITY),
        )
        .await,
    )
    .await;
    assert_eq!(incremental["nextCursor"], 2);
    assert_eq!(
        incremental["events"],
        json!([{"sequence": 2, "stream": "stderr", "text": "second\n"}])
    );
    assert_eq!(
        send(
            app.clone(),
            Method::GET,
            &format!("{location}?before=1"),
            Some(AUTHORITY),
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );

    run.complete(EntryRunOutcome::Exited(7));
    let exited = response_json(send(app, Method::GET, &location, Some(AUTHORITY)).await).await;
    assert_eq!(exited["state"], "exited");
    assert_eq!(exited["exitCode"], 7);
    runs.shutdown().expect("shutdown command runs");
}

#[tokio::test]
async fn cancels_a_run_and_joins_it_during_shutdown() {
    let fixture = Fixture::new();
    ready_fixture(&fixture);
    let runner = Arc::new(FakeRunner::default());
    let (app, runs) = command_app(&fixture, Arc::clone(&runner));
    let created =
        response_json(post_run(app.clone(), json!({"address": ".demo", "arguments": []})).await)
            .await;
    let location = format!(
        "/api/v2/command-runs/{}",
        created["id"].as_str().expect("command run id")
    );

    let canceled = send(app.clone(), Method::DELETE, &location, Some(AUTHORITY)).await;
    assert_eq!(canceled.status(), StatusCode::NO_CONTENT);
    let document = response_json(send(app, Method::GET, &location, Some(AUTHORITY)).await).await;
    assert_eq!(document["state"], "canceled");
    assert_eq!(document["exitCode"], Value::Null);

    let run = runner.run(0);
    assert!(run.canceled.load(Ordering::Acquire));
    runs.shutdown().expect("shutdown command runs");
    assert!(run.joined.load(Ordering::Acquire));
}

#[tokio::test]
async fn limits_active_runs_to_four() {
    let fixture = Fixture::new();
    ready_fixture(&fixture);
    let runner = Arc::new(FakeRunner::default());
    let (app, runs) = command_app(&fixture, Arc::clone(&runner));

    for _ in 0..4 {
        assert_eq!(
            post_run(app.clone(), json!({"address": ".demo"}))
                .await
                .status(),
            StatusCode::CREATED
        );
    }
    assert_eq!(
        post_run(app, json!({"address": ".demo"})).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(runner.specs().len(), 4);
    runs.shutdown().expect("shutdown command runs");
}

#[tokio::test]
async fn resolves_a_fresh_profile_working_directory_for_every_run() {
    let fixture = Fixture::new();
    ready_fixture(&fixture);
    let runner = Arc::new(FakeRunner::default());
    let (app, runs) = command_app(&fixture, Arc::clone(&runner));
    let first_root = fixture.directory("first-project");
    let second_root = fixture.directory("second-project");

    let mut profile = EntryProfileRecord::default();
    profile.target_project_root = first_root.to_string_lossy().into_owned();
    fixture
        .profile_store()
        .save(profile.clone())
        .expect("save first project root");
    assert_eq!(
        post_run(app.clone(), json!({"address": ".demo"}))
            .await
            .status(),
        StatusCode::CREATED
    );

    profile.target_project_root = second_root.to_string_lossy().into_owned();
    fixture
        .profile_store()
        .save(profile)
        .expect("save second project root");
    assert_eq!(
        post_run(app, json!({"address": ".demo"})).await.status(),
        StatusCode::CREATED
    );

    let specs = runner.specs();
    assert_eq!(specs[0].working_directory, first_root);
    assert_eq!(specs[1].working_directory, second_root);
    runs.shutdown().expect("shutdown command runs");
}

#[tokio::test]
async fn rejects_unrepresentable_or_oversized_arguments_before_starting() {
    let fixture = Fixture::new();
    let runner = Arc::new(FakeRunner::default());
    let (app, runs) = command_app(&fixture, Arc::clone(&runner));
    let too_many = (0..129).map(|_| "x").collect::<Vec<_>>();

    for request in [
        json!({"address": ".demo", "arguments": too_many}),
        json!({"address": ".demo", "arguments": ["x".repeat(4097)]}),
        json!({"address": ".demo", "arguments": ["x".repeat(4096), "y".repeat(4096)]}),
        json!({"address": ".demo", "arguments": ["contains\0nul"]}),
    ] {
        assert_eq!(
            post_run(app.clone(), request).await.status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    assert!(runner.specs().is_empty());
    runs.shutdown().expect("shutdown command runs");
}

#[tokio::test]
async fn accepts_only_exact_runnable_non_control_catalog_commands() {
    let fixture = Fixture::new();
    ready_fixture(&fixture);
    fixture.directory("home/_lib/proj/.group");
    fixture.file(
        "home/_lib/proj/..entry/claim/run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"entry.claim"}"#,
    );
    let runner = Arc::new(FakeRunner::default());
    let (app, runs) = command_app(&fixture, Arc::clone(&runner));

    for (address, expected) in [
        ("", StatusCode::UNPROCESSABLE_ENTITY),
        (".missing", StatusCode::NOT_FOUND),
        (".group", StatusCode::UNPROCESSABLE_ENTITY),
        ("..entry.claim", StatusCode::UNPROCESSABLE_ENTITY),
    ] {
        assert_eq!(
            post_run(app.clone(), json!({"address": address}))
                .await
                .status(),
            expected,
            "{address}"
        );
    }
    assert!(runner.specs().is_empty());
    runs.shutdown().expect("shutdown command runs");
}

#[tokio::test]
async fn bounds_retained_output_without_stopping_the_stream_cursor() {
    let fixture = Fixture::new();
    ready_fixture(&fixture);
    let runner = Arc::new(FakeRunner::default());
    let (app, runs) = command_app(&fixture, Arc::clone(&runner));
    let created = response_json(post_run(app.clone(), json!({"address": ".demo"})).await).await;
    let location = format!(
        "/api/v2/command-runs/{}",
        created["id"].as_str().expect("command run id")
    );
    let run = runner.run(0);
    let chunk = "x".repeat(8192);
    for _ in 0..129 {
        run.output(EntryOutputStream::Stdout, &chunk);
    }

    let document = response_json(send(app, Method::GET, &location, Some(AUTHORITY)).await).await;
    assert_eq!(document["truncated"], true);
    assert_eq!(document["nextCursor"], 129);
    let events = document["events"].as_array().expect("output events");
    assert_eq!(events.len(), 128);
    assert_eq!(events[0]["sequence"], 2);
    assert_eq!(events.last().expect("last output event")["sequence"], 129);

    run.complete(EntryRunOutcome::Exited(0));
    runs.shutdown().expect("shutdown command runs");
}

#[tokio::test]
async fn retains_only_the_latest_thirty_two_terminal_runs() {
    let fixture = Fixture::new();
    ready_fixture(&fixture);
    let runner = Arc::new(FakeRunner::default());
    let (app, runs) = command_app(&fixture, Arc::clone(&runner));
    let mut locations = Vec::new();

    for index in 0..33 {
        let response = post_run(app.clone(), json!({"address": ".demo"})).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let document = response_json(response).await;
        locations.push(format!(
            "/api/v2/command-runs/{}",
            document["id"].as_str().expect("command run id")
        ));
        runner.run(index).complete(EntryRunOutcome::Exited(0));
    }

    assert_eq!(
        send(app.clone(), Method::GET, &locations[0], Some(AUTHORITY))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(app, Method::GET, &locations[1], Some(AUTHORITY))
            .await
            .status(),
        StatusCode::OK
    );
    runs.shutdown().expect("shutdown command runs");
}
