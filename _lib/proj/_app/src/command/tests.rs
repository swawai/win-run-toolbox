use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    catalog::{CatalogSnapshot, CommandAdapter},
    profile::EntryProfileRecord,
};

use super::{
    CommandExecutionContext, CommandExecutor, ExecutionPhase, GuardPlan, GuardScope, Invocation,
    ProcessEnvironment, ResolvedCommand, process::run_process,
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    kernel_root: PathBuf,
    target_project_root: PathBuf,
    action_root: PathBuf,
    data_root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("workspace root");
        let root = workspace_root
            .join("data/proj_cache/tests")
            .join(format!("swawkit-command-{}-{sequence}", std::process::id()));
        let kernel_root = root.join("_lib/proj");
        let target_project_root = root.join("project");
        let action_root = target_project_root.join(".swaw");
        let data_root = root.join("data");
        for directory in [&kernel_root, &target_project_root, &action_root, &data_root] {
            fs::create_dir_all(directory).expect("create fixture directory");
        }
        Self {
            root,
            kernel_root,
            target_project_root,
            action_root,
            data_root,
        }
    }

    fn command(&self, address: &str, script: &str) -> PathBuf {
        let directory = command_directory(&self.kernel_root, address);
        fs::create_dir_all(&directory).expect("create command directory");
        fs::write(directory.join("run.ps1"), script).expect("write command entry");
        directory
    }

    fn guard(&self, root: &Path, name: &str, script: &str) {
        let directory = root.join(name);
        fs::create_dir_all(&directory).expect("create guard directory");
        fs::write(directory.join("run.ps1"), script).expect("write guard entry");
    }

    fn catalog(&self) -> CatalogSnapshot {
        CatalogSnapshot::discover_roots(&self.kernel_root, &self.action_root, "fixture")
            .expect("discover catalog")
    }

    fn context(&self) -> CommandExecutionContext {
        CommandExecutionContext {
            swawkit_home: self.root.clone(),
            kernel_root: self.kernel_root.clone(),
            target_project_root: self.target_project_root.clone(),
            action_root: self.action_root.clone(),
            data_root: self.data_root.clone(),
            entry_name: "fixture".to_owned(),
            entry_file: self.root.join("fixture.exe"),
            invocation_directory: self.target_project_root.clone(),
            profile: EntryProfileRecord::default(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn invocation_redirects_only_protocol_owned_help() {
    let fixture = Fixture::new();
    fixture.command(".help", "exit 0");
    let local = fixture.command(".local", "exit 0");
    fs::create_dir_all(local.join("_help")).unwrap();
    fs::write(local.join("_help/zh-CN.txt"), "Local help").unwrap();
    fixture.command(".owned", "exit 0");
    let catalog = fixture.catalog();

    let local = Invocation::resolve(&catalog, &argv(&[".local", "--help"])).unwrap();
    assert_eq!(local.command.address, ".help");
    assert_eq!(local.help_target_address.as_deref(), Some(".local"));
    assert!(local.arguments.is_empty());

    let owned = Invocation::resolve(&catalog, &argv(&[".owned", "--help"])).unwrap();
    assert_eq!(owned.command.address, ".owned");
    assert_eq!(owned.arguments, argv(&["--help"]));
    assert_eq!(owned.help_target_address, None);
}

#[test]
fn guard_plan_is_global_then_command_and_rejects_unsafe_entries() {
    let fixture = Fixture::new();
    let command_directory = fixture.command(".tool", "exit 0");
    fixture.guard(&fixture.kernel_root, "_global", "exit 0");
    fixture.guard(&command_directory, "_guard", "exit 0");
    let command = ResolvedCommand::from_catalog(&fixture.catalog(), ".tool").unwrap();

    let plan = GuardPlan::discover(&fixture.kernel_root, &command).unwrap();
    assert_eq!(
        plan.guards
            .iter()
            .map(|guard| guard.scope)
            .collect::<Vec<_>>(),
        vec![GuardScope::Global, GuardScope::Command]
    );

    fs::remove_file(command_directory.join("_guard/run.ps1")).unwrap();
    fs::write(command_directory.join("_guard/run.ts"), "").unwrap();
    assert!(
        GuardPlan::discover(&fixture.kernel_root, &command)
            .unwrap_err()
            .to_string()
            .contains("not bootstrap-safe")
    );
}

#[test]
fn process_environment_is_declarative_and_phase_specific() {
    let fixture = Fixture::new();
    fixture.command(".tool", "exit 0");
    let command = ResolvedCommand::from_catalog(&fixture.catalog(), ".tool").unwrap();
    let context = fixture.context();

    let run = ProcessEnvironment::for_command(&context, &command, ExecutionPhase::Run, None)
        .expect("build run environment");
    assert_eq!(
        run.value("SWAWKIT_PROJ_COMMAND_PHASE"),
        Some(Some(OsStr::new("run")))
    );
    assert_eq!(run.value("SWAWKIT_PROJ_GUARD_SCOPE"), Some(None));
    assert_eq!(
        run.value("SWAWKIT_PROJ_COMMAND_ADDRESS"),
        Some(Some(OsStr::new(".tool")))
    );
    assert_eq!(
        run.value("SWAWKIT_PROJ_COMMAND_DATA_ROOT"),
        Some(Some(
            fixture
                .data_root
                .join("modules")
                .join("kernel")
                .join(".tool")
                .as_os_str()
        ))
    );
    assert_eq!(
        run.value("SWAWKIT_PROJ_TARGET_PROJECT_ROOT"),
        Some(Some(fixture.target_project_root.as_os_str()))
    );
    assert_eq!(
        run.value("SWAWKIT_HOME"),
        Some(Some(fixture.root.as_os_str()))
    );
    assert_eq!(
        run.value("SWAWKIT_PROJ_BUN_VERSION"),
        Some(Some(OsStr::new("1.2.15")))
    );
    assert_eq!(run.value("SWAWKIT_PROJ_GIT_ID_EMAIL"), Some(None));
    let guard = ProcessEnvironment::for_command(
        &context,
        &command,
        ExecutionPhase::Guard(GuardScope::Global),
        Some(".target"),
    )
    .expect("build guard environment");
    assert_eq!(
        guard.value("SWAWKIT_PROJ_GUARD_SCOPE"),
        Some(Some(OsStr::new("global")))
    );
    assert_eq!(
        guard.value("SWAWKIT_PROJ_HELP_TARGET_ADDRESS"),
        Some(Some(OsStr::new(".target")))
    );
}

#[test]
fn command_data_roots_are_isolated_by_catalog_source() {
    let fixture = Fixture::new();
    fixture.command(".tool", "exit 0");
    let control = fixture.kernel_root.join("..entry");
    fs::create_dir_all(&control).unwrap();
    fs::write(
        control.join("run.core.json"),
        r#"{"schema":"swawkit.core-command/v1","handler":"entry.profile"}"#,
    )
    .unwrap();
    let action = fixture.action_root.join("build");
    fs::create_dir_all(&action).unwrap();
    fs::write(action.join("run.ps1"), "exit 0").unwrap();
    let catalog = fixture.catalog();
    let context = fixture.context();

    for (address, source, relative) in [
        (".tool", "kernel", ".tool"),
        ("..entry", "control", "..entry"),
        ("build", "action", "build"),
    ] {
        let command = ResolvedCommand::from_catalog(&catalog, address).unwrap();
        let environment =
            ProcessEnvironment::for_command(&context, &command, ExecutionPhase::Run, None)
                .unwrap();
        assert_eq!(
            environment.value("SWAWKIT_PROJ_COMMAND_DATA_ROOT"),
            Some(Some(
                fixture
                    .data_root
                    .join("modules")
                    .join(source)
                    .join(relative)
                    .as_os_str()
            ))
        );
    }
}

#[test]
fn powershell_pipeline_preserves_arguments_environment_order_and_exit_code() {
    let fixture = Fixture::new();
    let trace = r#"
$encoded = @($args | ForEach-Object {
    [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes([string]$_))
}) -join ','
$line = '__LABEL__|' + $env:SWAWKIT_PROJ_COMMAND_PHASE + '|' +
    $env:SWAWKIT_PROJ_GUARD_SCOPE + '|' + $env:SWAWKIT_PROJ_COMMAND_ADDRESS + '|' + $encoded
$tracePath = Join-Path $env:SWAWKIT_PROJ_DATA_ROOT 'trace.txt'
[IO.File]::AppendAllText($tracePath, $line + [Environment]::NewLine)
__EXIT__
"#;
    let command_directory = fixture.command(
        ".tool",
        &trace
            .replace("__LABEL__", "target")
            .replace("__EXIT__", "exit 23"),
    );
    fixture.guard(
        &fixture.kernel_root,
        "_global",
        &trace
            .replace("__LABEL__", "global")
            .replace("__EXIT__", "exit 0"),
    );
    fixture.guard(
        &command_directory,
        "_guard",
        &trace
            .replace("__LABEL__", "command")
            .replace("__EXIT__", "exit 0"),
    );
    let catalog = fixture.catalog();
    let context = fixture.context();
    let before = env::var_os("SWAWKIT_PROJ_COMMAND_PHASE");

    let exit_code = CommandExecutor::new(&context, &catalog)
        .execute(&argv(&[".tool", "", "a b", "quote\"x"]))
        .unwrap();

    assert_eq!(exit_code, 23);
    assert_eq!(env::var_os("SWAWKIT_PROJ_COMMAND_PHASE"), before);
    let lines = fs::read_to_string(fixture.data_root.join("trace.txt")).unwrap();
    let lines: Vec<&str> = lines.lines().collect();
    assert_eq!(lines[0], "global|guard|global|.tool|");
    assert_eq!(lines[1], "command|guard|command|.tool|");
    assert_eq!(lines[2], "target|run||.tool|,YSBi,cXVvdGUieA==");
}

#[test]
fn a_failing_guard_stops_the_pipeline() {
    let fixture = Fixture::new();
    fixture.command(
        ".tool",
        "Set-Content (Join-Path $env:SWAWKIT_PROJ_DATA_ROOT 'target.txt') 'ran'; exit 0",
    );
    fixture.guard(&fixture.kernel_root, "_global", "exit 17");
    let catalog = fixture.catalog();

    let exit_code = CommandExecutor::new(&fixture.context(), &catalog)
        .execute(&argv(&[".tool"]))
        .unwrap();

    assert_eq!(exit_code, 17);
    assert!(!fixture.data_root.join("target.txt").exists());
}

#[test]
fn cmd_adapter_allows_only_one_standalone_help_selector() {
    let fixture = Fixture::new();
    let directory = command_directory(&fixture.kernel_root, ".batch");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("run.cmd"),
        "@echo off\r\n\
         > \"%SWAWKIT_PROJ_DATA_ROOT%\\cmd.txt\" \
         echo %~1^|%SWAWKIT_PROJ_COMMAND_ADDRESS%\r\n\
         exit /b 31\r\n",
    )
    .unwrap();
    let catalog = fixture.catalog();
    let context = fixture.context();
    let executor = CommandExecutor::new(&context, &catalog);

    assert_eq!(executor.execute(&argv(&[".batch", "--help"])).unwrap(), 31);
    assert_eq!(
        fs::read_to_string(fixture.data_root.join("cmd.txt"))
            .unwrap()
            .trim(),
        "--help|.batch"
    );
    let error = executor
        .execute(&argv(&[".batch", "one", "two"]))
        .unwrap_err();
    assert!(error.to_string().contains("one standalone help selector"));
}

#[test]
fn powershell_adapter_propagates_a_native_child_exit_code() {
    let fixture = Fixture::new();
    fixture.command(".native", "& $env:ComSpec /d /c exit 12");
    let catalog = fixture.catalog();

    let exit_code = CommandExecutor::new(&fixture.context(), &catalog)
        .execute(&argv(&[".native"]))
        .unwrap();

    assert_eq!(exit_code, 12);
}

#[test]
fn exe_adapter_returns_the_exact_child_exit_code() {
    let fixture = Fixture::new();
    let comspec = env::var_os("ComSpec").expect("ComSpec");
    let environment = ProcessEnvironment::default();
    let arguments = argv(&["/d", "/c", "exit", "9"]);

    let exit_code = run_process(
        CommandAdapter::Exe,
        Path::new(&comspec),
        &arguments,
        &fixture.target_project_root,
        &environment,
    )
    .unwrap();

    assert_eq!(exit_code, 9);
}

fn argv(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn command_directory(kernel_root: &Path, address: &str) -> PathBuf {
    if address.is_empty() {
        return kernel_root.to_owned();
    }
    let mut segments = address.trim_start_matches('.').split('.');
    let mut directory = kernel_root.join(format!(".{}", segments.next().unwrap()));
    for segment in segments {
        directory.push(segment);
    }
    directory
}
