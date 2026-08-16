use super::*;

fn context_command_set(fixture: &Fixture) {
    for (address, handler) in [
        (".context.new", "context.new"),
        (".context.add", "context.add"),
        (".context.remove", "context.remove"),
        (".context.note", "context.note"),
        (".context.prompt", "context.prompt"),
        (".context.render", "context.render"),
        (".context.show", "context.show"),
        (".context.list", "context.list"),
        (".context.delete", "context.delete"),
    ] {
        fixture.core_command(address, handler);
    }
    fs::write(
        fixture.context.kernel_root().join(".context/_module.json"),
        include_str!("../../../../.context/_module.json"),
    )
    .expect("write Context module contract");
}

#[test]
fn context_commands_share_one_parent_store_and_resolve_catalog_addresses() {
    let fixture = Fixture::new();
    context_command_set(&fixture);
    fixture.command(".dev.status", "run.cmd", "@exit /b 0\r\n");
    let action = fixture.target_project_root.join(".swaw/build/app");
    fs::create_dir_all(&action).unwrap();
    fs::write(action.join("run.cmd"), "@exit /b 0\r\n").unwrap();
    fixture.bind();
    let mut unexpected =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));

    for invocation in [
        argv(&[".context.new", "mycontext01"]),
        argv(&[".context.add", "mycontext01", ".dev.status", "build.app"]),
        argv(&[
            ".context.note",
            "mycontext01",
            "检查编译环境",
            "然后构建 app",
        ]),
        argv(&[".context.prompt", "mycontext01", "执行上述步骤"]),
        argv(&[".context.show", "mycontext01"]),
        argv(&[".context.render", "mycontext01"]),
        argv(&[".context.list"]),
        argv(&[".context.list", "--json"]),
    ] {
        assert_eq!(
            run_with_approver(&fixture.context, &invocation, &mut unexpected).unwrap(),
            0
        );
    }

    let path = fixture
        .data_root()
        .join("modules/kernel/.context/mycontext01/_resource.json");
    let record: swawkit_proj::context_store::ContextRecord =
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(record.commands.len(), 2);
    assert_eq!(
        record.commands[0].source,
        swawkit_proj::catalog::CommandSource::Kernel
    );
    assert_eq!(
        record.commands[1].source,
        swawkit_proj::catalog::CommandSource::Action
    );
    assert_eq!(record.notes, ["检查编译环境 然后构建 app"]);
    assert_eq!(record.prompt, "执行上述步骤");
}

#[test]
fn remove_does_not_require_the_referenced_command_to_still_exist() {
    let fixture = Fixture::new();
    context_command_set(&fixture);
    let command_directory = fixture.command(".temporary", "run.cmd", "@exit /b 0\r\n");
    fixture.bind();
    let mut unexpected =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));
    for invocation in [
        argv(&[".context.new", "cleanup"]),
        argv(&[".context.add", "cleanup", ".temporary"]),
    ] {
        run_with_approver(&fixture.context, &invocation, &mut unexpected).unwrap();
    }
    fs::remove_dir_all(command_directory).unwrap();

    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&[".context.remove", "cleanup", ".temporary"]),
            &mut unexpected,
        )
        .unwrap(),
        0
    );
}

#[test]
fn invalid_context_shapes_fail_before_mutation() {
    let fixture = Fixture::new();
    context_command_set(&fixture);
    fixture.bind();
    let mut unexpected =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));

    let invalid_id = run_with_approver(
        &fixture.context,
        &argv(&[".context.new", "../outside"]),
        &mut unexpected,
    )
    .unwrap_err();
    assert!(invalid_id.to_string().contains("Context ID must match"));

    let ambiguous_remove = run_with_approver(
        &fixture.context,
        &argv(&[".context.remove", "mycontext01"]),
        &mut unexpected,
    )
    .unwrap_err();
    assert_eq!(
        ambiguous_remove.to_string(),
        "usage: .context.remove <context-id> <command-address>..."
    );

    let missing_command = run_with_approver(
        &fixture.context,
        &argv(&[".context.add", "mycontext01", ".missing"]),
        &mut unexpected,
    )
    .unwrap_err();
    assert_eq!(missing_command.to_string(), "command not found: .missing");

    let removed_show_mode = run_with_approver(
        &fixture.context,
        &argv(&[".context.show", "mycontext01", "--json"]),
        &mut unexpected,
    )
    .unwrap_err();
    assert_eq!(
        removed_show_mode.to_string(),
        "usage: .context.show <context-id>"
    );
}
