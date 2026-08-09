use super::*;

#[test]
fn control_web_command_launches_the_entry_before_profile_gating() {
    let fixture = Fixture::new();
    fixture.core_command("..web", "host.start");
    fs::create_dir_all(fixture.data_root()).unwrap();
    let mut unexpected_claim =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));
    let mut launched = false;
    let mut launch_host = |context: &EntryContext| {
        launched = context.entry_file == fixture.context.entry_file;
        Ok(0)
    };

    let exit_code = run_with_host_launcher(
        &fixture.context,
        &argv(&["..web"]),
        &mut unexpected_claim,
        &mut launch_host,
    )
    .unwrap();

    assert_eq!(exit_code, 0);
    assert!(launched);
    assert!(
        read_entry_record(&fixture.data_root())
            .valid_record()
            .is_none()
    );
    assert!(!fixture.data_root().join("_profile.json").exists());
}

#[test]
fn host_process_uses_the_entry_launcher_as_the_process_boundary() {
    let fixture = Fixture::new();
    let command = host_process_command(&fixture.context);
    assert_eq!(
        command.get_program(),
        fixture.context.entry_file.as_os_str()
    );
    assert_eq!(
        command.get_current_dir(),
        Some(fixture.context.invocation_directory.as_path())
    );
    assert_eq!(command.get_args().count(), 0);

    assert_eq!(command.get_envs().count(), 0);
}

#[test]
fn entry_env_variables_are_independent_catalog_commands() {
    let fixture = Fixture::new();
    for (group, name) in EntryProfileRecord::environment_variable_commands() {
        fixture.core_command(&format!("..entry.env.{group}.{name}"), "entry.profile.set");
    }

    let snapshot = CatalogSnapshot::discover(&fixture.context, None).unwrap();
    let setters = snapshot
        .commands
        .iter()
        .filter(|command| command.handler.as_deref() == Some("entry.profile.set"))
        .collect::<Vec<_>>();

    assert_eq!(setters.len(), 32);
    assert!(setters.iter().all(|command| {
        let Some(path) = command.address.strip_prefix("..entry.env.") else {
            return false;
        };
        let Some((group, name)) = path.split_once('.') else {
            return false;
        };
        let expected_parent = format!("..entry.env.{group}");
        command.parent.as_deref() == Some(expected_parent.as_str())
            && EntryProfileRecord::environment_variable_commands().contains(&(group, name))
    }));
}

#[test]
fn entry_control_commands_create_and_update_a_profile_before_profile_gating() {
    let fixture = Fixture::new();
    fixture.core_command("..entry", "entry.profile");
    fixture.core_command(
        "..entry.env.git.SWAWKIT_PROJ_GIT_ID_NAME",
        "entry.profile.set",
    );
    fixture.core_command("..entry.apply", "entry.profile.apply");
    fs::create_dir_all(fixture.context.kernel_root().join("..entry/env/_help")).unwrap();
    fs::write(
        fixture
            .context
            .kernel_root()
            .join("..entry/env/_help/zh-CN.txt"),
        "Set Entry Profile variables",
    )
    .unwrap();
    let global_guard = fixture.context.kernel_root().join("_global");
    fs::create_dir_all(&global_guard).unwrap();
    fs::write(
        global_guard.join("run.core.json"),
        r#"{"schema":"swawkit.core-command/v1","handler":"host.start"}"#,
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
            &argv(&["..entry.env", ".h"]),
            &mut unexpected_claim,
        )
        .unwrap(),
        0
    );
    assert!(!fixture.data_root().join("_profile.json").exists());

    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&["..entry.env.git.SWAWKIT_PROJ_GIT_ID_NAME", "Fixture User",]),
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

    let before_invalid_update = fs::read(fixture.data_root().join("_profile.json")).unwrap();
    let invalid_update = run_with_approver(
        &fixture.context,
        &argv(&["..entry.env.git.SWAWKIT_PROJ_UNKNOWN", "value"]),
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
