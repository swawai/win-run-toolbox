use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::mpsc,
    time::Duration,
};

use super::*;

#[tokio::test]
async fn runtime_status_is_one_typed_control_document() {
    let fixture = Fixture::new();
    let response = send(
        fixture.app(),
        Method::GET,
        "/api/v2/runtime",
        Some(AUTHORITY),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let document: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("Runtime status body"),
    )
    .expect("Runtime status JSON");
    assert_eq!(document["protocol"], "swawkit.runtime-status/v1");
    assert_eq!(document["selectedReleaseId"], "1".repeat(64));
    assert_eq!(document["releaseCount"], 0);
    assert_eq!(document["host"]["protocol"], "swawkit.host-status/v1");
    assert_eq!(document["host"]["updateAvailable"], false);
}

#[tokio::test]
async fn runtime_cleanup_requires_an_explicit_control_action() {
    let fixture = Fixture::new();
    let response = send(
        fixture.app(),
        Method::POST,
        "/api/v2/runtime/cleanup",
        Some(AUTHORITY),
    )
    .await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn binds_independent_random_ports_on_ipv4_loopback() {
    let first = bind_loopback().await.expect("first listener");
    let second = bind_loopback().await.expect("second listener");
    let first_address = first.local_addr().expect("first address");
    let second_address = second.local_addr().expect("second address");

    assert_eq!(first_address.ip(), Ipv4Addr::LOCALHOST);
    assert_eq!(second_address.ip(), Ipv4Addr::LOCALHOST);
    assert_ne!(first_address.port(), 0);
    assert_ne!(second_address.port(), 0);
    assert_ne!(first_address.port(), second_address.port());
}

#[test]
fn shutdown_signal_stops_the_live_http_server() {
    let fixture = Fixture::new();
    fixture.directory("home/_lib/proj");
    let (events, received_events) = mpsc::channel();
    let (shutdown, shutdown_receiver) = oneshot::channel();
    let host_runtime = test_host_runtime();
    let server_thread = spawn(
        fixture.context(),
        fixture.data_root_session(),
        host_runtime.identity(),
        move |event| events.send(event).map_err(|error| error.to_string()),
        shutdown_receiver,
    )
    .expect("server thread");

    let ready = received_events
        .recv_timeout(Duration::from_secs(5))
        .expect("ready event");
    let ServerEvent::Ready(document) = ready else {
        panic!("expected ready event");
    };
    let authority = document
        .url
        .strip_prefix("http://")
        .and_then(|value| value.strip_suffix('/'))
        .expect("loopback URL");

    let mut stream = TcpStream::connect(authority).expect("HTTP connection");
    write!(
        stream,
        "GET /healthz HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
    )
    .expect("HTTP request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("HTTP response");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(response.to_ascii_lowercase().contains(&format!(
        "{}: {}\r\n",
        crate::host_runtime::HOST_BOOT_HEADER,
        document.boot_id
    )));
    assert!(response.to_ascii_lowercase().contains(&format!(
        "{}: {}\r\n",
        crate::host_runtime::HOST_ENTRY_HEADER,
        document.entry_key_sha256
    )));
    assert!(response.ends_with("\r\nok\n"));

    shutdown.send(()).expect("shutdown signal");
    let stopped = received_events
        .recv_timeout(Duration::from_secs(5))
        .expect("stopped event");
    assert!(matches!(stopped, ServerEvent::Stopped(Ok(()))));
    server_thread.join().expect("clean server thread");
}

#[test]
fn authenticated_web_shutdown_stops_the_live_http_server() {
    let fixture = Fixture::new();
    fixture.directory("home/_lib/proj");
    let (events, received_events) = mpsc::channel();
    let (_shutdown, shutdown_receiver) = oneshot::channel();
    let host_runtime = test_host_runtime();
    let server_thread = spawn(
        fixture.context(),
        fixture.data_root_session(),
        host_runtime.identity(),
        move |event| events.send(event).map_err(|error| error.to_string()),
        shutdown_receiver,
    )
    .expect("server thread");

    let ready = received_events
        .recv_timeout(Duration::from_secs(5))
        .expect("ready event");
    let ServerEvent::Ready(document) = ready else {
        panic!("expected ready event");
    };
    let authority = document
        .url
        .strip_prefix("http://")
        .and_then(|value| value.strip_suffix('/'))
        .expect("loopback URL");

    let mut stream = TcpStream::connect(authority).expect("HTTP connection");
    write!(
        stream,
        "POST /api/v2/host/shutdown HTTP/1.1\r\nHost: {authority}\r\n\
         X-SwawKit-Control: shutdown\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
    .expect("HTTP shutdown request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("HTTP response");
    assert!(response.starts_with("HTTP/1.1 202 Accepted\r\n"));

    let stopped = received_events
        .recv_timeout(Duration::from_secs(5))
        .expect("stopped event");
    assert!(matches!(stopped, ServerEvent::Stopped(Ok(()))));
    server_thread.join().expect("clean server thread");
}
