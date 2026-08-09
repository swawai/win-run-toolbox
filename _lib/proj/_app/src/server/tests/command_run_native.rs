use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::time::{Duration, Instant};

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, header::LOCATION};
use serde_json::{Value, json};
use tower::ServiceExt;
use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};

use super::*;
use crate::profile::EntryProfileRecord;
use crate::server::command_run::CommandRuns;

const NORMAL_ACTION: &str = "webnativeworkerfixture";
const CANCEL_ACTION: &str = "webnativeworkercancelfixture";
const NORMAL_MARKER: &str = "web-native-worker.marker";
const CANCEL_PID_MARKER: &str = "web-native-worker-descendant.pid";
const STDOUT_SENTINEL: &str = "SWAWKIT_WEB_NATIVE_STDOUT_SENTINEL";
const STDERR_SENTINEL: &str = "SWAWKIT_WEB_NATIVE_STDERR_SENTINEL";
const TEST_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn executes_and_cancels_native_workers_through_the_http_router() {
    let fixture = Fixture::new();
    install_current_test_executable(&fixture);
    fixture.directory("home/_lib/proj");
    fixture.file(&format!("home/.swaw/{NORMAL_ACTION}/run.exe"), "fixture");
    fixture.file(&format!("home/.swaw/{CANCEL_ACTION}/run.exe"), "fixture");
    fixture
        .profile_store()
        .save(EntryProfileRecord::default())
        .expect("save native worker fixture profile");

    // Keep both runs in one registry: run IDs form part of the named Job and
    // Event boundary, so two fresh registries in one process would both start
    // at the same PID+sequence identifier.
    let runs = CommandRuns::native();
    let app = router_with_runs(
        AUTHORITY.to_owned(),
        fixture.context(),
        fixture.data_root_session(),
        runs.clone(),
    );

    let (normal_location, normal_created) = start_native_run(&app, NORMAL_ACTION).await;
    assert_eq!(normal_created["address"], NORMAL_ACTION);
    let normal = wait_for_terminal(&app, &normal_location).await;
    assert_eq!(normal["state"], "exited");
    assert_eq!(normal["exitCode"], 0);
    assert_eq!(normal["error"], Value::Null);
    assert_eq!(
        fs::read_to_string(fixture.root.join("home").join(NORMAL_MARKER))
            .expect("read native worker cwd marker"),
        "worker cwd reached\n"
    );
    let (stdout, stderr) = output_text(&normal);
    assert!(stdout.contains(STDOUT_SENTINEL), "stdout was: {stdout:?}");
    assert!(stderr.contains(STDERR_SENTINEL), "stderr was: {stderr:?}");

    let (cancel_location, cancel_created) = start_native_run(&app, CANCEL_ACTION).await;
    assert_eq!(cancel_created["state"], "running");
    let descendant_pid = wait_for_pid_file(&fixture.root.join("home").join(CANCEL_PID_MARKER));
    let descendant = open_process_for_wait(descendant_pid);
    assert_eq!(
        unsafe { WaitForSingleObject(raw_handle(&descendant), 0) },
        WAIT_TIMEOUT,
        "native worker descendant exited before cancellation"
    );

    let canceled = send(
        app.clone(),
        Method::DELETE,
        &cancel_location,
        Some(AUTHORITY),
    )
    .await;
    assert_eq!(canceled.status(), StatusCode::NO_CONTENT);
    let canceled = wait_for_terminal(&app, &cancel_location).await;
    assert_eq!(canceled["state"], "canceled");
    assert_eq!(canceled["exitCode"], Value::Null);
    assert_eq!(canceled["error"], Value::Null);
    assert_eq!(
        unsafe { WaitForSingleObject(raw_handle(&descendant), TEST_TIMEOUT.as_millis() as u32,) },
        WAIT_OBJECT_0,
        "DELETE did not terminate the native worker descendant"
    );

    runs.shutdown().expect("shut down native command runs");
}

fn install_current_test_executable(fixture: &Fixture) {
    let source_path = std::env::current_exe().expect("resolve current libtest executable");
    let mut source = File::open(&source_path).expect("open current libtest executable");
    let entry_path = fixture.context().entry_file;
    let mut entry = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&entry_path)
        .expect("open bound fixture Entry without replacing its file identity");
    io::copy(&mut source, &mut entry).expect("copy current libtest executable into fixture Entry");
    entry
        .sync_all()
        .expect("flush copied fixture Entry executable");
}

async fn start_native_run(app: &Router, address: &str) -> (String, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/v2/command-runs")
                .header(HOST, AUTHORITY)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "address": address,
                        "arguments": ["--nocapture", "--test-threads=1"]
                    })
                    .to_string(),
                ))
                .expect("valid native command run request"),
        )
        .await
        .expect("native command run response");
    assert_eq!(response.status(), StatusCode::CREATED);
    let location = response
        .headers()
        .get(LOCATION)
        .expect("native command run Location")
        .to_str()
        .expect("native command run Location text")
        .to_owned();
    let document = response_json(response).await;
    (location, document)
}

async fn wait_for_terminal(app: &Router, location: &str) -> Value {
    let deadline = Instant::now() + TEST_TIMEOUT;
    loop {
        let response = send(app.clone(), Method::GET, location, Some(AUTHORITY)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let document = response_json(response).await;
        if matches!(
            document["state"].as_str(),
            Some("exited" | "canceled" | "failed")
        ) {
            return document;
        }
        assert!(
            Instant::now() < deadline,
            "native command run did not reach a terminal state: {document}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

async fn response_json(response: Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("native command run response body");
    serde_json::from_slice(&body).expect("native command run response JSON")
}

fn output_text(document: &Value) -> (String, String) {
    let mut stdout = String::new();
    let mut stderr = String::new();
    for event in document["events"].as_array().expect("native output events") {
        let text = event["text"].as_str().expect("native output event text");
        match event["stream"].as_str() {
            Some("stdout") => stdout.push_str(text),
            Some("stderr") => stderr.push_str(text),
            stream => panic!("unexpected native output stream: {stream:?}"),
        }
    }
    (stdout, stderr)
}

fn wait_for_pid_file(path: &std::path::Path) -> u32 {
    let deadline = Instant::now() + TEST_TIMEOUT;
    loop {
        match fs::read_to_string(path) {
            Ok(pid) => return pid.parse().expect("native worker descendant PID"),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => panic!(
                "read native worker descendant PID '{}': {error}",
                path.display()
            ),
        }
        assert!(
            Instant::now() < deadline,
            "native worker did not publish its descendant PID"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn open_process_for_wait(pid: u32) -> OwnedHandle {
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    assert!(
        !handle.is_null(),
        "open native worker descendant {pid}: {}",
        io::Error::last_os_error()
    );
    unsafe { OwnedHandle::from_raw_handle(handle) }
}

fn raw_handle(handle: &OwnedHandle) -> HANDLE {
    use std::os::windows::io::AsRawHandle;
    handle.as_raw_handle() as HANDLE
}
