use super::*;

#[test]
fn runtime_status_is_available_before_data_root_and_profile_gating() {
    let fixture = Fixture::new();
    fixture.core_command("..runtime", "runtime.status");
    let mut unexpected_claim =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));

    let exit_code = run_with_approver(
        &fixture.context,
        &argv(&["..runtime", "--json"]),
        &mut unexpected_claim,
    )
    .unwrap();

    assert_eq!(exit_code, 0);
    assert!(!fixture.data_root().exists());
}

#[test]
fn profile_settings_are_independent_typed_catalog_commands() {
    let fixture = Fixture::new();
    for address in EntryProfileRecord::profile_setting_addresses() {
        fixture.core_command(address, "entry.profile.set");
    }

    let snapshot = CatalogSnapshot::discover(&fixture.context, None).unwrap();
    let setters = snapshot
        .commands
        .iter()
        .filter(|command| command.handler.as_deref() == Some("entry.profile.set"))
        .collect::<Vec<_>>();

    assert_eq!(setters.len(), 18);
    assert!(setters.iter().all(|command| {
        let expected_parent = command.address.rsplit_once('.').map(|(parent, _)| parent);
        command.parent.as_deref() == expected_parent
            && EntryProfileRecord::is_profile_setting_address(&command.address)
    }));
}

#[test]
fn entry_control_commands_create_and_update_a_profile_before_profile_gating() {
    let fixture = Fixture::new();
    fixture.core_command("..entry", "entry.profile");
    fixture.core_command("..entry.git.name", "entry.profile.set");
    fixture.core_command(".dev.bun.mode", "entry.profile.set");
    fixture.core_command("..entry.apply", "entry.profile.apply");
    fs::create_dir_all(fixture.context.kernel_root().join("..entry/git/_help")).unwrap();
    fs::write(
        fixture
            .context
            .kernel_root()
            .join("..entry/git/_help/zh-CN.txt"),
        "Set Entry Profile Git settings",
    )
    .unwrap();
    let global_guard = fixture.context.kernel_root().join("_global");
    fs::create_dir_all(&global_guard).unwrap();
    fs::write(
        global_guard.join("run.core.json"),
        r#"{"schema":"swawkit.core-command/v1","handler":"runtime.status"}"#,
    )
    .unwrap();
    let mut unexpected_claim =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));

    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&["..entry", "--json"]),
            &mut unexpected_claim,
        )
        .unwrap(),
        0
    );
    assert!(!fixture.data_root().join("_profile.json").exists());

    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&["..entry.git", ".h"]),
            &mut unexpected_claim,
        )
        .unwrap(),
        0
    );
    assert!(!fixture.data_root().join("_profile.json").exists());

    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&["..entry.git.name", "Fixture User"]),
            &mut unexpected_claim,
        )
        .unwrap(),
        0
    );
    let EntryProfileState::Ready(profile) =
        EntryProfileStore::new(&fixture.context.swawkit_home, fixture.data_root()).read()
    else {
        panic!("expected ready profile");
    };
    assert_eq!(profile.record().git.name, "Fixture User");

    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&[".dev.bun.mode", "disabled"]),
            &mut unexpected_claim,
        )
        .unwrap(),
        0
    );
    let EntryProfileState::Ready(profile) =
        EntryProfileStore::new(&fixture.context.swawkit_home, fixture.data_root()).read()
    else {
        panic!("expected ready profile");
    };
    assert_eq!(profile.record().development.bun.mode, "disabled");

    let before_invalid_update = fs::read(fixture.data_root().join("_profile.json")).unwrap();
    let invalid_update = run_with_approver(
        &fixture.context,
        &argv(&["..entry.git.unknown", "value"]),
        &mut unexpected_claim,
    )
    .unwrap_err();
    assert!(invalid_update.to_string().contains("command not found"));
    assert_eq!(
        fs::read(fixture.data_root().join("_profile.json")).unwrap(),
        before_invalid_update
    );

    let mut replacement = profile.record().clone();
    replacement.git.name = "Applied User".to_owned();
    let input = fixture.target_project_root.join("profile.json");
    fs::write(&input, serde_json::to_string(&replacement).unwrap()).unwrap();
    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&["..entry.apply", "--file", "profile.json"]),
            &mut unexpected_claim,
        )
        .unwrap(),
        0
    );
    let EntryProfileState::Ready(profile) =
        EntryProfileStore::new(&fixture.context.swawkit_home, fixture.data_root()).read()
    else {
        panic!("expected applied profile");
    };
    assert_eq!(profile.record().git.name, "Applied User");
}
