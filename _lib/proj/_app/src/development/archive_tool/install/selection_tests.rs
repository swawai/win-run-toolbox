use std::fs;
use std::path::PathBuf;

use super::tests::{FailingRecipe, Fixture, FixtureRecipe};
use super::*;
use crate::development::archive_tool::ArchiveToolRequest;
use crate::development::{BUN, PWSH};

fn first_latest(
    tool: &'static ArchiveToolContract,
    version: &str,
    digest: Option<&str>,
    verification: SourceVerification,
) -> ResolvedDefinition {
    ResolvedDefinition {
        tool_name: tool.name.to_owned(),
        requested_latest: true,
        version: version.to_owned(),
        source_sha256: digest.map(str::to_owned),
        verification: digest.map_or(ResolvedVerification::Unresolved, |_| {
            ResolvedVerification::Published(verification)
        }),
        project_sha256: String::new(),
    }
}

fn selection_path(fixture: &Fixture, tool: &ArchiveToolContract) -> PathBuf {
    fixture
        .data_root
        .join("modules/kernel/.dev/setup/export")
        .join(tool.name)
        .join(".swawkit-dev-selection.json")
}

#[test]
fn successful_latest_installations_publish_power_shell_compatible_selections() {
    let cases = [
        (&BUN, "1.2.15", SourceVerification::Github),
        (&PWSH, "7.6.4", SourceVerification::Unverified),
    ];
    for (tool, version, verification) in cases {
        let fixture = Fixture::new(tool, version, b"fixture");
        let digest = fixture.digest();
        let expected = (verification == SourceVerification::Github).then_some(digest.as_str());
        let resolved = first_latest(tool, version, expected, verification);
        let source = ArchiveSource::new(
            &resolved,
            fixture.archive.to_string_lossy(),
            expected,
            verification,
        )
        .unwrap();
        let request = InstallRequest::new(
            &fixture.data_root,
            &fixture.cache_root,
            tool,
            resolved.clone(),
        )
        .unwrap();

        ensure_installed_with(request, |_| Ok(source), &FixtureRecipe, &mut |_, _| {}).unwrap();

        let path = selection_path(&fixture, tool);
        let content = fs::read(&path).unwrap();
        assert!(!content.starts_with(&[0xef, 0xbb, 0xbf]));
        let value: serde_json::Value = serde_json::from_slice(&content).unwrap();
        assert_eq!(value["schema"], tool.selection_schema);
        assert_eq!(value["selector"], "latest");
        assert_eq!(value["version"], version);
        assert_eq!(value["sourceSha256"], digest);
        assert_eq!(value["sourceVerification"], verification.as_str());

        // PowerShell may use different insignificant JSON whitespace. An
        // idempotent setup must read it without rewriting the file.
        let powershell_style = format!(
            "{{\r\n    \"schema\": \"{}\",\r\n    \"selector\": \"latest\",\r\n    \
             \"version\": \"{}\",\r\n    \"sourceSha256\": \"{}\",\r\n    \
             \"sourceVerification\": \"{}\"\r\n}}\r\n",
            tool.selection_schema,
            version,
            digest,
            verification.as_str()
        )
        .into_bytes();
        fs::write(&path, &powershell_style).unwrap();
        let request =
            InstallRequest::new(&fixture.data_root, &fixture.cache_root, tool, resolved).unwrap();
        ensure_installed_with(
            request,
            |_| panic!("a ready latest installation must resolve offline"),
            &FixtureRecipe,
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(fs::read(path).unwrap(), powershell_style);
    }
}

#[test]
fn a_different_latest_selection_is_never_advanced_implicitly() {
    let fixture = Fixture::new(&BUN, "1.3.0", b"fixture");
    let digest = fixture.digest();
    fixture.publish_latest_selection(&BUN, "1.2.15", &"b".repeat(64), SourceVerification::Github);
    let resolved = first_latest(&BUN, "1.3.0", Some(&digest), SourceVerification::Github);
    let source = ArchiveSource::new(
        &resolved,
        fixture.archive.to_string_lossy(),
        Some(&digest),
        SourceVerification::Github,
    )
    .unwrap();
    let request = InstallRequest::new(
        &fixture.data_root,
        &fixture.cache_root,
        &BUN,
        resolved.clone(),
    )
    .unwrap();

    let error = ensure_installed_with(request, |_| Ok(source), &FixtureRecipe, &mut |_, _| {})
        .err()
        .expect("a different selection must conflict");

    assert_eq!(error.kind(), ArchiveToolErrorKind::SelectionConflict);
    let selection = ArchiveToolStore::new(&fixture.data_root, &BUN)
        .read_selection()
        .unwrap()
        .unwrap();
    assert_eq!(selection.version(), "1.2.15");
    // Installation and selection are separate transactions. The valid new
    // installation remains reusable even though implicit selection failed.
    ArchiveToolStore::new(&fixture.data_root, &BUN)
        .read_installation(&resolved)
        .unwrap();
}

#[test]
fn a_failed_latest_installation_does_not_publish_a_selection() {
    let fixture = Fixture::new(&PWSH, "7.6.4", b"fixture");
    let resolved = first_latest(&PWSH, "7.6.4", None, SourceVerification::Unverified);
    let source = ArchiveSource::new(
        &resolved,
        fixture.archive.to_string_lossy(),
        None,
        SourceVerification::Unverified,
    )
    .unwrap();
    let request =
        InstallRequest::new(&fixture.data_root, &fixture.cache_root, &PWSH, resolved).unwrap();

    let error = ensure_installed_with(request, |_| Ok(source), &FailingRecipe, &mut |_, _| {})
        .err()
        .expect("a failed installation must fail");

    assert_eq!(error.kind(), ArchiveToolErrorKind::ProbeFailed);
    assert!(!selection_path(&fixture, &PWSH).exists());
}

#[test]
fn an_exact_installation_never_publishes_a_latest_selection() {
    let fixture = Fixture::new(&BUN, "1.2.15", b"fixture");
    let digest = fixture.digest();
    let request = ArchiveToolRequest::new(&BUN, "1.2.15", &digest).unwrap();
    let resolved = ArchiveToolStore::new(&fixture.data_root, &BUN)
        .resolve(&request)
        .unwrap()
        .unwrap();
    let source = ArchiveSource::new(
        &resolved,
        fixture.archive.to_string_lossy(),
        Some(&digest),
        SourceVerification::Project,
    )
    .unwrap();
    let request =
        InstallRequest::new(&fixture.data_root, &fixture.cache_root, &BUN, resolved).unwrap();

    ensure_installed_with(request, |_| Ok(source), &FixtureRecipe, &mut |_, _| {}).unwrap();

    assert!(!selection_path(&fixture, &BUN).exists());
}
