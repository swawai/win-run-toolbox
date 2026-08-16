use super::*;

#[test]
fn runs_read_history_and_latest_after_the_target_stops_being_runnable() {
    let fixture = Fixture::new();
    let runs_directory = fixture.command(
        ".runs",
        "run.core.json",
        r#"{"schema":"swawkit.core-command/v1","handler":"meta.runs"}"#,
    );
    fs::write(
        runs_directory.join("_module.json"),
        include_str!("../../../../.runs/_module.json"),
    )
    .expect("write Runs module contract");
    let command_directory = fixture.command(
        ".demo",
        "run.cmd",
        "@echo off\r\necho journal fixture\r\nexit /b 0\r\n",
    );
    fixture.bind();
    let mut unexpected_claim =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));

    for _ in 0..3 {
        assert_eq!(
            run_with_approver(&fixture.context, &argv(&[".demo"]), &mut unexpected_claim).unwrap(),
            0
        );
    }
    let runs_root = fixture.data_root().join("modules/kernel/.demo/_runs");
    let run_id = fs::read_dir(&runs_root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name()
        .to_string_lossy()
        .into_owned();
    fs::remove_file(command_directory.join("run.cmd")).unwrap();

    for arguments in [
        vec![".runs"],
        vec![".runs", "--json"],
        vec![".runs", "--json", "kernel/.demo"],
        vec![".runs", "--run", &run_id],
        vec![".runs", ".demo"],
        vec![".runs", ".demo", "--latest", "1"],
        vec![".runs", ".demo", "--latest", "1..3"],
        vec![".runs", ".demo", "--run", &run_id, "--after", "0"],
    ] {
        assert_eq!(
            run_with_approver(&fixture.context, &argv(&arguments), &mut unexpected_claim).unwrap(),
            0
        );
    }

    let missing = run_with_approver(
        &fixture.context,
        &argv(&[".runs", ".missing"]),
        &mut unexpected_claim,
    )
    .unwrap_err();
    assert!(missing.to_string().contains("command not found"));

    let removed =
        run_with_approver(&fixture.context, &argv(&[".logs"]), &mut unexpected_claim).unwrap_err();
    assert!(removed.to_string().contains("command not found: .logs"));
}
