use axum::http::header::{ETAG, IF_MATCH};

use super::*;

async fn send_claim(app: Router, confirmation: &str, revision: Option<&HeaderValue>) -> Response {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri("/api/v2/data-root/claim")
        .header(HOST, AUTHORITY)
        .header(CONTENT_TYPE, "application/json");
    if let Some(revision) = revision {
        request = request.header(IF_MATCH, revision);
    }
    app.oneshot(
        request
            .body(Body::from(
                json!({ "confirmation": confirmation }).to_string(),
            ))
            .expect("claim request"),
    )
    .await
    .expect("claim response")
}

#[tokio::test]
async fn pending_claim_gates_the_host_and_transitions_to_ready() {
    let fixture = Fixture::new();
    fixture.replace_entry(b"copied entry");
    let record_path = fixture.root.join("home/data/proj.swawkit/_entry.json");
    let before = fs::read(&record_path).expect("existing entry record");
    let app = fixture.app();

    let pending = send(
        app.clone(),
        Method::GET,
        "/api/v2/data-root/claim",
        Some(AUTHORITY),
    )
    .await;
    assert_eq!(pending.status(), StatusCode::OK);
    let revision = pending.headers().get(ETAG).expect("claim ETag").clone();
    let body = to_bytes(pending.into_body(), usize::MAX)
        .await
        .expect("claim body");
    let document: Value = serde_json::from_slice(&body).expect("claim JSON");
    assert_eq!(document["protocol"], "swawkit.data-root-claim/v2");
    assert_eq!(document["status"], "claimRequired");
    assert_eq!(document["claim"]["entryName"], "swawkit");
    assert_eq!(document["claim"]["kind"], "current");
    assert!(
        document["claim"]["reason"]
            .as_str()
            .unwrap()
            .contains("File ID")
    );

    assert_eq!(
        send(app.clone(), Method::GET, "/api/v2/profile", Some(AUTHORITY))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        send_claim(app.clone(), "wrong", Some(&revision))
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(fs::read(&record_path).unwrap(), before);
    assert_eq!(
        send_claim(app.clone(), "swawkit", None).await.status(),
        StatusCode::PRECONDITION_REQUIRED
    );
    assert_eq!(fs::read(&record_path).unwrap(), before);

    let malformed_revision = HeaderValue::from_static("unquoted");
    let malformed = send_claim(app.clone(), "swawkit", Some(&malformed_revision)).await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    let malformed_body = to_bytes(malformed.into_body(), usize::MAX)
        .await
        .expect("malformed If-Match body");
    let malformed_document: Value =
        serde_json::from_slice(&malformed_body).expect("malformed If-Match JSON");
    assert!(
        malformed_document["error"]
            .as_str()
            .unwrap()
            .contains("DataRoot claim")
    );
    assert!(
        !malformed_document["error"]
            .as_str()
            .unwrap()
            .contains("entry profile")
    );

    let claimed = send_claim(app.clone(), "swawkit", Some(&revision)).await;
    assert_eq!(claimed.status(), StatusCode::NO_CONTENT);
    let claimed_body = to_bytes(claimed.into_body(), usize::MAX)
        .await
        .expect("claimed body");
    assert!(claimed_body.is_empty());
    assert_ne!(fs::read(&record_path).unwrap(), before);
    assert_eq!(
        send(
            app.clone(),
            Method::GET,
            "/api/v2/data-root/claim",
            Some(AUTHORITY)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(app, Method::GET, "/api/v2/profile", Some(AUTHORITY))
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn stale_claim_revision_cannot_overwrite_a_changed_binding_record() {
    let fixture = Fixture::new();
    fixture.replace_entry(b"first copy");
    let record_path = fixture.root.join("home/data/proj.swawkit/_entry.json");
    let app = fixture.app();
    let pending = send(
        app.clone(),
        Method::GET,
        "/api/v2/data-root/claim",
        Some(AUTHORITY),
    )
    .await;
    let stale_revision = pending.headers().get(ETAG).expect("claim ETag").clone();

    let concurrent_record = b"concurrent binding update\n";
    fs::write(&record_path, concurrent_record).expect("change binding record concurrently");
    assert_eq!(
        send_claim(app, "swawkit", Some(&stale_revision))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(fs::read(record_path).unwrap(), concurrent_record);
}

#[tokio::test]
async fn ready_probe_keeps_module_data_opaque() {
    let fixture = Fixture::new();
    let export = fixture.directory("home/data/proj.swawkit/modules/kernel/.dev/setup/export");
    fs::write(export.join("sentinel.bin"), b"opaque").expect("opaque module publication");
    let app = fixture.app();

    for _ in 0..2 {
        let response = send(
            app.clone(),
            Method::GET,
            "/api/v2/data-root/claim",
            Some(AUTHORITY),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(fs::read(export.join("sentinel.bin")).unwrap(), b"opaque");
    }
}

#[tokio::test]
async fn claim_result_and_retry_keep_module_data_opaque() {
    let fixture = Fixture::new();
    fixture.replace_entry(b"copied entry");
    let export = fixture.directory("home/data/proj.swawkit/modules/kernel/.dev/setup/export");
    fs::write(export.join("sentinel.bin"), b"opaque").expect("opaque module publication");
    let app = fixture.app();
    let pending = send(
        app.clone(),
        Method::GET,
        "/api/v2/data-root/claim",
        Some(AUTHORITY),
    )
    .await;
    let revision = pending.headers().get(ETAG).expect("claim ETag").clone();

    let claimed = send_claim(app.clone(), "swawkit", Some(&revision)).await;
    assert_eq!(claimed.status(), StatusCode::NO_CONTENT);

    let retry = send(app, Method::GET, "/api/v2/data-root/claim", Some(AUTHORITY)).await;
    assert_eq!(retry.status(), StatusCode::NO_CONTENT);
    assert_eq!(fs::read(export.join("sentinel.bin")).unwrap(), b"opaque");
}
