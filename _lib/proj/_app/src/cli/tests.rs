use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use swawkit_proj::context::EntryContext;
use swawkit_proj::data_root::{
    ClaimApprovalError, DataRootClaim, ResolveDataRootRequest, read_entry_record, resolve_data_root,
};
use swawkit_proj::profile::{EntryProfileRecord, EntryProfileStore};

use super::*;

mod control;
mod logs;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    context: EntryContext,
    target_project_root: PathBuf,
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
            .join(format!("swawkit-cli-{}-{sequence}", std::process::id()));
        let kernel_root = root.join("_lib/proj");
        let project_root = root.join("project");
        let action_root = project_root.join(".swaw");
        let entry_file = root.join("launchers/fixture.exe");
        for directory in [
            &kernel_root,
            &project_root,
            &action_root,
            entry_file.parent().unwrap(),
        ] {
            fs::create_dir_all(directory).expect("create fixture directory");
        }
        fs::write(&entry_file, "fixture").expect("write entry file");
        let context = EntryContext {
            swawkit_home: root.clone(),
            entry_file,
            entry_name: "fixture".to_owned(),
            invocation_directory: project_root.clone(),
        };
        Self {
            root,
            context,
            target_project_root: project_root,
        }
    }

    fn data_root(&self) -> PathBuf {
        self.root.join("data/proj.fixture")
    }

    fn bind(&self) {
        let mut approve = |_claim: &DataRootClaim| Ok(true);
        let resolved = resolve_data_root(
            ResolveDataRootRequest {
                swawkit_home: &self.context.swawkit_home,
                entry_file: &self.context.entry_file,
            },
            &mut approve,
        )
        .expect("resolve fixture DataRoot");
        let mut profile = EntryProfileRecord::default();
        profile.target_project_root = self
            .target_project_root
            .to_str()
            .expect("Unicode fixture path")
            .to_owned();
        EntryProfileStore::new(&self.context.swawkit_home, resolved.path())
            .save(profile)
            .expect("save fixture profile");
    }

    fn command(&self, address: &str, entry_name: &str, body: &str) -> PathBuf {
        let mut directory = self.context.kernel_root();
        if !address.is_empty() {
            let mut segments = address.trim_start_matches('.').split('.');
            directory.push(format!(".{}", segments.next().unwrap()));
            for segment in segments {
                directory.push(segment);
            }
        }
        fs::create_dir_all(&directory).expect("create command directory");
        fs::write(directory.join(entry_name), body).expect("write command entry");
        directory
    }

    fn core_command(&self, address: &str, handler: &str) -> PathBuf {
        let suffix = address
            .strip_prefix("..")
            .expect("Control address must begin with '..'");
        let mut segments = suffix.split('.');
        let mut directory = self
            .context
            .kernel_root()
            .join(format!("..{}", segments.next().unwrap()));
        for segment in segments {
            directory.push(segment);
        }
        fs::create_dir_all(&directory).expect("create Core command directory");
        fs::write(
            directory.join("run.core.json"),
            format!("{{\"schema\":\"swawkit.core-command/v1\",\"handler\":\"{handler}\"}}"),
        )
        .expect("write Core command manifest");
        directory
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn protocol_help_initializes_the_entry_without_requiring_an_entry_profile() {
    let fixture = Fixture::new();
    fixture.command("", "run.ps1", "exit 0");
    fs::create_dir_all(fixture.context.kernel_root().join("_help")).unwrap();
    fs::write(
        fixture.context.kernel_root().join("_help/zh-CN.txt"),
        "Root help",
    )
    .unwrap();
    let mut unexpected =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));

    let exit_code =
        run_with_approver(&fixture.context, &argv(&["--help"]), &mut unexpected).unwrap();

    assert_eq!(exit_code, 0);
    assert!(
        read_entry_record(&fixture.data_root())
            .valid_record()
            .is_some()
    );
}

#[test]
fn local_help_is_read_only_but_command_owned_help_executes() {
    let fixture = Fixture::new();
    let local = fixture.command(".local", "run.ps1", "exit 99");
    fs::create_dir_all(local.join("_help")).unwrap();
    fs::write(local.join("_help/zh-CN.txt"), "Local help").unwrap();
    fixture.command(
        ".owned",
        "run.cmd",
        "@echo off\r\nif \"%~1\"==\"--help\" exit /b 13\r\nexit /b 99\r\n",
    );
    let mut unexpected =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));

    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&[".help", ".local"]),
            &mut unexpected,
        )
        .unwrap(),
        0
    );
    fixture.bind();
    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&[".local", "--help"]),
            &mut unexpected,
        )
        .unwrap(),
        99
    );
    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&[".owned", "--help"]),
            &mut unexpected,
        )
        .unwrap(),
        13
    );
    let unavailable = run_with_approver(
        &fixture.context,
        &argv(&[".help", ".owned"]),
        &mut unexpected,
    )
    .unwrap_err();
    assert!(unavailable.to_string().contains("not enabled"));
    assert!(
        read_entry_record(&fixture.data_root())
            .valid_record()
            .is_some()
    );
}

#[test]
fn command_execution_creates_and_reuses_the_entry_data_root() {
    let fixture = Fixture::new();
    fixture.command(".tool", "run.cmd", "@exit /b 29\r\n");
    fixture.bind();
    let mut unexpected =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));

    for _ in 0..2 {
        assert_eq!(
            run_with_approver(&fixture.context, &argv(&[".tool"]), &mut unexpected,).unwrap(),
            29
        );
    }
    assert!(
        read_entry_record(&fixture.data_root())
            .valid_record()
            .is_some()
    );
}

#[test]
fn invalid_or_unsupported_commands_fail_before_process_execution() {
    let fixture = Fixture::new();
    fixture.command(".future", "run.ts", "");
    fixture.bind();
    let mut unexpected =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));

    let missing =
        run_with_approver(&fixture.context, &argv(&[".missing"]), &mut unexpected).unwrap_err();
    assert!(missing.to_string().contains("command not found"));
    let unsupported =
        run_with_approver(&fixture.context, &argv(&[".future"]), &mut unexpected).unwrap_err();
    assert!(unsupported.to_string().contains("does not yet support"));
    assert!(
        read_entry_record(&fixture.data_root())
            .valid_record()
            .is_some()
    );
}

#[test]
fn an_unbound_candidate_requires_approval_before_execution() {
    let fixture = Fixture::new();
    fixture.command(".tool", "run.cmd", "@exit /b 0\r\n");
    fs::create_dir_all(fixture.data_root()).unwrap();
    let mut saw_claim = false;
    let mut approve = |claim: &DataRootClaim| {
        saw_claim = claim.data_root == fixture.data_root() && claim.entry_name == "fixture";
        Ok(true)
    };

    let error = run_with_approver(&fixture.context, &argv(&[".tool"]), &mut approve).unwrap_err();
    assert!(error.to_string().contains("no profile"));
    assert!(saw_claim);
    assert!(
        read_entry_record(&fixture.data_root())
            .valid_record()
            .is_some()
    );

    fixture.bind();
    let mut unexpected =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim was not expected"));
    assert_eq!(
        run_with_approver(&fixture.context, &argv(&[".tool"]), &mut unexpected,).unwrap(),
        0
    );
}

#[test]
fn ordinary_cli_rejects_a_claim_immediately_with_dedicated_commands() {
    let fixture = Fixture::new();
    fixture.command(".tool", "run.cmd", "@exit /b 0\r\n");
    fs::create_dir_all(fixture.data_root()).unwrap();

    let mut reject = |pending: &DataRootClaim| Err(claim::rejection(&fixture.context, pending));
    let error = run_with_approver(&fixture.context, &argv(&[".tool"]), &mut reject)
        .expect_err("ordinary command must not claim DataRoot");
    let message = error.to_string();
    assert!(message.contains("Status: claimRequired"));
    assert!(message.contains("Review: fixture ..entry.claim"));
    assert!(message.contains("Apply: fixture ..entry.claim --yes"));
    assert!(
        read_entry_record(&fixture.data_root())
            .valid_record()
            .is_none()
    );
}

#[test]
fn dedicated_claim_preview_is_read_only_and_yes_applies_it() {
    let fixture = Fixture::new();
    fixture.core_command("..entry.claim", "entry.claim");
    fs::create_dir_all(fixture.data_root()).unwrap();
    let record_path = fixture.data_root().join("_entry.json");
    let mut unexpected =
        |_claim: &DataRootClaim| Err(ClaimApprovalError::new("claim callback was not expected"));

    assert_eq!(
        run_with_approver(&fixture.context, &argv(&["..entry.claim"]), &mut unexpected,).unwrap(),
        0
    );
    assert!(!record_path.exists());
    assert!(!fixture.root.join("data/_proj-entry.lock").exists());

    assert_eq!(
        run_with_approver(
            &fixture.context,
            &argv(&["..entry.claim", "--yes"]),
            &mut unexpected,
        )
        .unwrap(),
        0
    );
    assert!(
        read_entry_record(&fixture.data_root())
            .valid_record()
            .is_some()
    );
}

#[test]
fn help_shape_keeps_non_help_invocations_for_the_executor() {
    assert_eq!(help_target(&argv(&[".tool"])).unwrap(), None);
    assert_eq!(help_target(&argv(&[".tool", "value"])).unwrap(), None);
    assert_eq!(help_target(&argv(&[".tool", "--help"])).unwrap(), None);
    assert_eq!(
        help_target(&argv(&[".help", ".tool"])).unwrap(),
        Some(".tool".to_owned())
    );
}

fn argv(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}
