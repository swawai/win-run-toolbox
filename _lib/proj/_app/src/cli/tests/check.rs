use std::fs;

use swawkit_proj::data_root::{ClaimApprovalError, DataRootClaim};

use super::super::*;
use super::{Fixture, argv};

#[test]
fn module_check_uses_declared_provider_state_and_returns_a_machine_exit_code() {
    let fixture = Fixture::new();
    fixture.core_command(".check", "meta.check");
    let provider = fixture.command(".provider", "run.exe", "fixture");
    fs::write(
        provider.join("_module.json"),
        r#"{"schema":"swawkit.command-module/v4","provides":[{"contract":"swawkit.fixture/v1"}]}"#,
    )
    .unwrap();
    let consumer = fixture.command(".consumer", "run.exe", "fixture");
    fs::write(
        consumer.join("_module.json"),
        r#"{"schema":"swawkit.command-module/v4","requires":[{"provider":".provider","contract":"swawkit.fixture/v1"}]}"#,
    )
    .unwrap();
    fixture.bind();
    let provider_data = fixture.data_root().join("modules/kernel/.provider");
    fs::create_dir_all(provider_data.join("export")).unwrap();
    fs::write(provider_data.join("export/sentinel.txt"), "ready").unwrap();
    fs::write(
        provider_data.join("_state.json"),
        r#"{"schema":"swawkit.command-provider-state/v1","status":"ready","inputRevision":"sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","token":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","producerContract":"swawkit.fixture/v1"}"#,
    )
    .unwrap();
    let mut unexpected =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));

    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&[".check", ".consumer", "--json"]),
            &mut unexpected,
        )
        .unwrap(),
        0
    );

    fs::remove_file(provider_data.join("_state.json")).unwrap();
    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&[".check", ".consumer"]),
            &mut unexpected,
        )
        .unwrap(),
        1
    );
}

#[test]
fn module_check_rejects_ambiguous_arguments() {
    let fixture = Fixture::new();
    fixture.core_command(".check", "meta.check");
    fixture.bind();
    let mut unexpected =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));
    let error = run_with_approver(
        &fixture.context,
        &argv(&[".check", ".consumer", "--json", "extra"]),
        &mut unexpected,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "usage: .check <command-address> [--json]"
    );
}
