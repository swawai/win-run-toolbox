use super::*;
use crate::binding::SWAWKIT_HOME_PLACEHOLDER;
use crate::profile::EntryProfileState;

async fn send_setting(app: Router, address: &str, value: &str, revision: Option<&str>) -> Response {
    let mut request = Request::builder()
        .method(Method::PUT)
        .uri(format!("/api/v2/profile/settings/{address}"))
        .header(HOST, AUTHORITY)
        .header(CONTENT_TYPE, "application/json");
    if let Some(revision) = revision {
        request = request.header(IF_MATCH, format!("\"{revision}\""));
    }
    app.oneshot(
        request
            .body(Body::from(json!({ "value": value }).to_string()))
            .expect("valid JSON request"),
    )
    .await
    .expect("router response")
}

async fn response_document(response: Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("JSON response body");
    serde_json::from_slice(&body).expect("response JSON")
}

#[tokio::test]
async fn publishes_one_validated_setting_and_enables_actions() {
    let fixture = Fixture::new();
    fixture.directory("home/_lib/proj");
    fixture.file("home/.swaw/demo/run.ps1", "");
    let app = fixture.app();

    let before = send(app.clone(), Method::GET, "/api/v2/profile", Some(AUTHORITY)).await;
    assert_eq!(before.status(), StatusCode::OK);
    assert_eq!(
        before
            .headers()
            .get(ETAG)
            .and_then(|value| value.to_str().ok()),
        Some("\"missing\"")
    );
    let document = response_document(before).await;
    assert_eq!(
        document["protocol"],
        crate::profile::PROFILE_DOCUMENT_PROTOCOL
    );
    let initial_revision = document["revision"].as_str().expect("profile revision");
    assert_eq!(document["status"], "setupRequired");
    assert_eq!(document["requiredComplete"], false);
    assert_eq!(
        document["settings"]["..entry.project.root"],
        SWAWKIT_HOME_PLACEHOLDER
    );
    assert_eq!(document["settings"].as_object().unwrap().len(), 18);
    assert!(command(&catalog_document(app.clone()).await, "demo").is_none());

    let invalid = send_setting(
        app.clone(),
        "..entry.project.root",
        "relative/project",
        Some(initial_revision),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        response_document(invalid).await["error"]
            .as_str()
            .is_some_and(|error| error.contains("must be absolute"))
    );

    let saved = send_setting(
        app.clone(),
        "..entry.project.root",
        SWAWKIT_HOME_PLACEHOLDER,
        Some(initial_revision),
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let document = response_document(saved).await;
    assert_eq!(document["status"], "ready");
    assert_eq!(document["requiredComplete"], true);
    assert_eq!(
        document["settings"]["..entry.project.root"],
        SWAWKIT_HOME_PLACEHOLDER
    );
    assert_eq!(
        command(&catalog_document(app).await, "demo").and_then(|node| node["source"].as_str()),
        Some("action")
    );
}

#[tokio::test]
async fn requires_a_revision_and_rejects_a_stale_setting_without_overwriting() {
    let fixture = Fixture::new();
    fixture.directory("home/_lib/proj");
    let app = fixture.app();

    let missing_precondition =
        send_setting(app.clone(), "..entry.git.name", "Web Writer", None).await;
    assert_eq!(
        missing_precondition.status(),
        StatusCode::PRECONDITION_REQUIRED
    );

    let initial = send(app.clone(), Method::GET, "/api/v2/profile", Some(AUTHORITY)).await;
    let initial = response_document(initial).await;
    let initial_revision = initial["revision"].as_str().unwrap();
    let saved = send_setting(
        app.clone(),
        "..entry.git.name",
        "Web Writer",
        Some(initial_revision),
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);
    let saved = response_document(saved).await;
    let stale_revision = saved["revision"].as_str().unwrap();

    fixture
        .profile_store()
        .update_setting("..entry.git.name", "CLI Writer".to_owned())
        .expect("concurrent CLI update");

    let conflict = send_setting(
        app,
        "..entry.git.email",
        "web@example.com",
        Some(stale_revision),
    )
    .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert!(
        response_document(conflict).await["error"]
            .as_str()
            .is_some_and(|error| error.contains("changed since it was loaded"))
    );

    let document = fixture.profile_store().document();
    assert_eq!(document.profile.git.name, "CLI Writer");
    assert_eq!(document.profile.git.email, "");
}

#[tokio::test]
async fn entry_language_selects_the_catalog_help_document() {
    let fixture = Fixture::new();
    fixture.file("home/_lib/proj/_help/zh-CN.txt", "中文帮助");
    fixture.file("home/_lib/proj/_help/en.txt", "English help");
    let app = fixture.app();

    let initial = send(app.clone(), Method::GET, "/api/v2/profile", Some(AUTHORITY)).await;
    let initial = response_document(initial).await;
    let saved = send_setting(
        app.clone(),
        "..entry.language",
        "en",
        initial["revision"].as_str(),
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);

    let catalog = catalog_document(app).await;
    assert_eq!(catalog["language"], "en");
    assert_eq!(
        command(&catalog, "").and_then(|node| node["help"]["summary"].as_str()),
        Some("English help")
    );
}

#[tokio::test]
async fn web_profile_updates_share_the_provider_invalidation_transaction() {
    let fixture = Fixture::new();
    fixture.directory("home/_lib/proj");
    let app = fixture.app();
    let initial = send(app.clone(), Method::GET, "/api/v2/profile", Some(AUTHORITY)).await;
    let initial = response_document(initial).await;

    let created = send_setting(
        app.clone(),
        "..entry.git.name",
        "Web Writer",
        initial["revision"].as_str(),
    )
    .await;
    assert_eq!(created.status(), StatusCode::OK);
    let created = response_document(created).await;
    let state_path = fixture
        .root
        .join("home/data/proj.swawkit/modules/kernel/.dev/setup/_state.json");
    let first_state = fs::read(&state_path).expect("read initial provider state");

    let non_provider = send_setting(
        app.clone(),
        "..entry.git.email",
        "web@example.com",
        created["revision"].as_str(),
    )
    .await;
    assert_eq!(non_provider.status(), StatusCode::OK);
    let non_provider = response_document(non_provider).await;
    assert_eq!(fs::read(&state_path).unwrap(), first_state);

    let provider = send_setting(
        app.clone(),
        ".dev.bun.version",
        "1.2.16",
        non_provider["revision"].as_str(),
    )
    .await;
    assert_eq!(provider.status(), StatusCode::OK);
    let state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    let EntryProfileState::Ready(profile) = fixture.profile_store().read() else {
        panic!("expected ready profile");
    };
    assert_eq!(state["inputRevision"], profile.environment_input_revision());
    let state_after_provider_update = fs::read(&state_path).unwrap();

    let stale = send_setting(
        app,
        ".dev.bun.version",
        "1.2.17",
        non_provider["revision"].as_str(),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    assert_eq!(fs::read(&state_path).unwrap(), state_after_provider_update);
}
