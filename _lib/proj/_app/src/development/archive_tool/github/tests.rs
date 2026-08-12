use super::*;
use crate::development::{BUN, PWSH};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn release_json(tag: &str, asset: &str, url: &str, digest: Option<&str>) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "tag_name": tag,
        "assets": [{
            "name": asset,
            "browser_download_url": url,
            "digest": digest
        }]
    }))
    .unwrap()
}

fn exact(tool: &'static ArchiveToolContract, version: &str, project: &str) -> ResolvedDefinition {
    super::super::ArchiveToolStore::new(std::path::Path::new("unused"), tool)
        .resolve(&ArchiveToolRequest::new(tool, version, project).unwrap())
        .unwrap()
        .unwrap()
}

#[test]
fn latest_becomes_an_exact_github_definition() {
    let coordinates = BUN.release_coordinates("1.2.16");
    let digest = "A".repeat(64);
    let document = release_json(
        &coordinates.tag,
        &coordinates.asset,
        &coordinates.download_url,
        Some(&format!("sha256:{digest}")),
    );
    let request = ArchiveToolRequest::new(&BUN, "latest", "").unwrap();

    let release = resolve_latest_document(&BUN, &request, &document).unwrap();

    assert!(release.definition().requested_latest());
    assert_eq!(release.definition().version(), "1.2.16");
    let normalized = "a".repeat(64);
    assert_eq!(
        release.definition().source_sha256(),
        Some(normalized.as_str())
    );
    assert_eq!(
        release.definition().verification(),
        ResolvedVerification::Published(SourceVerification::Github)
    );
    assert_eq!(release.source().url(), coordinates.download_url);
}

#[test]
fn project_digest_wins_only_when_it_agrees_with_github() {
    let project = "1".repeat(64);
    let definition = exact(&PWSH, "7.6.4", &project);
    let coordinates = PWSH.release_coordinates("7.6.4");
    let matching = release_json(
        &coordinates.tag,
        &coordinates.asset,
        &coordinates.download_url,
        Some(&format!("sha256:{project}")),
    );
    let release = resolve_document(&PWSH, &definition, &coordinates, &matching).unwrap();
    assert_eq!(release.source().verification(), SourceVerification::Project);

    let conflicting = release_json(
        &coordinates.tag,
        &coordinates.asset,
        &coordinates.download_url,
        Some(&format!("sha256:{}", "2".repeat(64))),
    );
    let error = resolve_document(&PWSH, &definition, &coordinates, &conflicting).unwrap_err();
    assert_eq!(error.kind(), ArchiveToolErrorKind::GithubReleaseInvalid);
    assert!(error.to_string().contains("does not match"));
}

#[test]
fn missing_digest_is_explicitly_unverified() {
    let definition = exact(&BUN, "1.2.16", "");
    let coordinates = BUN.release_coordinates("1.2.16");
    let document = release_json(
        &coordinates.tag,
        &coordinates.asset,
        &coordinates.download_url,
        None,
    );

    let release = resolve_document(&BUN, &definition, &coordinates, &document).unwrap();

    assert_eq!(
        release.definition().verification(),
        ResolvedVerification::Unresolved
    );
    assert_eq!(
        release.source().verification(),
        SourceVerification::Unverified
    );
    assert_eq!(release.source().expected_sha256(), None);

    for digest in [Some(""), Some("  \t ")] {
        let document = release_json(
            &coordinates.tag,
            &coordinates.asset,
            &coordinates.download_url,
            digest,
        );
        let release = resolve_document(&BUN, &definition, &coordinates, &document).unwrap();
        assert_eq!(
            release.source().verification(),
            SourceVerification::Unverified
        );
    }
}

#[test]
fn tag_asset_and_url_are_all_strict() {
    let definition = exact(&BUN, "1.2.16", "");
    let coordinates = BUN.release_coordinates("1.2.16");
    for document in [
        release_json(
            "bun-v1.2.17",
            &coordinates.asset,
            &coordinates.download_url,
            None,
        ),
        release_json(
            &coordinates.tag,
            "BUN-WINDOWS-X64.ZIP",
            &coordinates.download_url,
            None,
        ),
        release_json(
            &coordinates.tag,
            &coordinates.asset,
            &format!("{}?download=1", coordinates.download_url),
            None,
        ),
    ] {
        assert!(resolve_document(&BUN, &definition, &coordinates, &document).is_err());
    }
}

#[test]
fn duplicate_asset_and_malformed_digest_are_rejected() {
    let definition = exact(&BUN, "1.2.16", "");
    let coordinates = BUN.release_coordinates("1.2.16");
    let duplicate = serde_json::to_vec(&serde_json::json!({
        "tag_name": coordinates.tag,
        "assets": [
            {"name": coordinates.asset, "browser_download_url": coordinates.download_url, "digest": null},
            {"name": coordinates.asset, "browser_download_url": coordinates.download_url, "digest": null}
        ]
    }))
    .unwrap();
    assert!(resolve_document(&BUN, &definition, &coordinates, &duplicate).is_err());

    let invalid = release_json(
        &coordinates.tag,
        &coordinates.asset,
        &coordinates.download_url,
        Some("sha256:not-a-digest"),
    );
    assert!(resolve_document(&BUN, &definition, &coordinates, &invalid).is_err());
}

#[test]
fn a_published_latest_source_is_reconstructed_without_transport() {
    let digest = "3".repeat(64);
    let definition = ResolvedDefinition {
        tool_name: BUN.name.to_owned(),
        requested_latest: true,
        version: "1.2.16".to_owned(),
        source_sha256: Some(digest.clone()),
        verification: ResolvedVerification::Published(SourceVerification::Github),
        project_sha256: String::new(),
    };

    let source = published_source(&BUN, &definition).unwrap();

    assert_eq!(source.url(), BUN.release_coordinates("1.2.16").download_url);
    assert_eq!(source.expected_sha256(), Some(digest.as_str()));
    assert_eq!(source.verification(), SourceVerification::Github);
}

#[test]
fn exact_project_sources_cannot_bypass_release_comparison() {
    let definition = exact(&PWSH, "7.6.4", &"3".repeat(64));
    let error = published_source(&PWSH, &definition).unwrap_err();
    assert_eq!(error.kind(), ArchiveToolErrorKind::GithubReleaseInvalid);
}

#[test]
fn endpoints_and_tls_policy_are_fixed() {
    assert_eq!(
        latest_endpoint(&BUN),
        "https://api.github.com/repos/oven-sh/bun/releases/latest"
    );
    assert_eq!(
        release_endpoint(&PWSH, "v7.6.4"),
        "https://api.github.com/repos/PowerShell/PowerShell/releases/tags/v7.6.4"
    );
    assert_eq!(
        request_headers(&BUN),
        [
            ("Accept", "application/vnd.github+json"),
            ("X-GitHub-Api-Version", "2026-03-10"),
            ("User-Agent", "swawkit-proj-v0"),
        ]
    );
    let agent = github_agent();
    assert!(matches!(
        agent.config().tls_config().root_certs(),
        RootCerts::PlatformVerifier
    ));
    assert_eq!(agent.config().max_redirects(), 0);
}

#[test]
fn transport_sends_the_contract_headers() {
    let coordinates = BUN.release_coordinates("1.2.16");
    let body = release_json(
        &coordinates.tag,
        &coordinates.asset,
        &coordinates.download_url,
        None,
    );
    let (endpoint, server) = serve_once("200 OK", &[], body);

    let received = request_release_with_agent(&BUN, &endpoint, &local_agent()).unwrap();
    let request = server.join().unwrap().to_ascii_lowercase();

    assert!(!received.is_empty());
    assert!(request.contains("accept: application/vnd.github+json\r\n"));
    assert!(request.contains("x-github-api-version: 2026-03-10\r\n"));
    assert!(request.contains("user-agent: swawkit-proj-v0\r\n"));
}

#[test]
fn transport_rejects_oversized_and_non_success_documents() {
    let (oversized, oversized_server) = serve_once(
        "200 OK",
        &[(
            "Content-Length",
            &(MAX_RELEASE_DOCUMENT_BYTES + 1).to_string(),
        )],
        Vec::new(),
    );
    let error = request_release_with_agent(&BUN, &oversized, &local_agent()).unwrap_err();
    oversized_server.join().unwrap();
    assert_eq!(error.kind(), ArchiveToolErrorKind::GithubReleaseInvalid);

    let (failed, failed_server) = serve_once("503 Service Unavailable", &[], b"{}".to_vec());
    let error = request_release_with_agent(&BUN, &failed, &local_agent()).unwrap_err();
    failed_server.join().unwrap();
    assert_eq!(error.kind(), ArchiveToolErrorKind::GithubUnavailable);
}

fn serve_once(
    status: &'static str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let headers = headers
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect::<Vec<_>>();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 1024];
        while !request.ends_with(b"\r\n\r\n") {
            let count = stream.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
            assert!(request.len() < 64 * 1024);
        }
        let has_length = headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-length"));
        write!(stream, "HTTP/1.1 {status}\r\nConnection: close\r\n").unwrap();
        for (name, value) in headers {
            write!(stream, "{name}: {value}\r\n").unwrap();
        }
        if !has_length {
            write!(stream, "Content-Length: {}\r\n", body.len()).unwrap();
        }
        stream.write_all(b"\r\n").unwrap();
        let _ = stream.write_all(&body);
        String::from_utf8(request).unwrap()
    });
    (format!("http://{address}/release"), server)
}

fn local_agent() -> Agent {
    Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .proxy(None)
        .build()
        .into()
}
