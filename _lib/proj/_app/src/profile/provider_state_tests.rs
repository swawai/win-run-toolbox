use super::*;
use serde_json::Value;
use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicU64, Ordering};
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    data_root: PathBuf,
    store: EntryProfileStore,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-provider-state-{}-{sequence}",
            std::process::id()
        ));
        let home = root.join("home");
        let data_root = home.join("data/proj.fixture");
        fs::create_dir_all(&data_root).expect("create fixture directories");
        let store = EntryProfileStore::new(&home, &data_root);
        Self {
            root,
            data_root,
            store,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn dev_setup_inputs_are_an_explicit_normalized_subset_of_profile_variables() {
    assert_eq!(
        EntryProfileRecord::dev_setup_input_variable_names()
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "SWAWKIT_PROJ_BUN_MODE",
            "SWAWKIT_PROJ_BUN_SHA256",
            "SWAWKIT_PROJ_BUN_VERSION",
            "SWAWKIT_PROJ_MSVC_CHANNEL",
            "SWAWKIT_PROJ_MSVC_MODE",
            "SWAWKIT_PROJ_PWSH_MODE",
            "SWAWKIT_PROJ_PWSH_SHA256",
            "SWAWKIT_PROJ_PWSH_VERSION",
            "SWAWKIT_PROJ_RUST_HOST",
            "SWAWKIT_PROJ_RUST_MODE",
            "SWAWKIT_PROJ_RUST_PROFILE",
            "SWAWKIT_PROJ_RUST_TOOLCHAIN",
        ])
    );

    let baseline = EntryProfileRecord::default();
    let baseline_revision = baseline.environment_input_revision();
    assert_revision_format(&baseline_revision);

    let mut non_provider = baseline.clone();
    non_provider.target_project_root = fixture_absolute_path("unrelated-target");
    non_provider.git.name = "Fixture User".to_owned();
    non_provider.language = "en".to_owned();
    non_provider.development.go.version = "1.25".to_owned();
    non_provider.development.gh.mode = "disabled".to_owned();
    assert_eq!(non_provider.environment_input_revision(), baseline_revision);

    let mut uppercase = baseline.clone();
    uppercase.development.bun.sha256 = "A".repeat(64);
    uppercase.development.rust.toolchain = "STABLE".to_owned();
    let mut lowercase = baseline;
    lowercase.development.bun.sha256 = "a".repeat(64);
    lowercase.development.rust.toolchain = "stable".to_owned();
    assert_eq!(
        uppercase.environment_input_revision(),
        lowercase.environment_input_revision()
    );
}

#[test]
fn profile_transactions_invalidate_the_dev_setup_provider_only_when_inputs_change() {
    let fixture = Fixture::new();
    let first_profile = fixture
        .store
        .save(EntryProfileRecord::default())
        .expect("save the missing profile");
    let state_path = provider_state::state_path(&fixture.data_root);
    let first_state_bytes = fs::read(&state_path).expect("read initial provider state");
    let first_state: Value = serde_json::from_slice(&first_state_bytes).unwrap();
    assert_eq!(first_state["schema"], "swawkit.command-provider-state/v1");
    assert_eq!(first_state["status"], "unavailable");
    assert_eq!(
        first_state["inputRevision"],
        first_profile.environment_input_revision()
    );
    assert_eq!(
        first_profile.profile_revision(),
        fixture.store.document().revision
    );
    assert_revision_format(first_state["inputRevision"].as_str().unwrap());
    assert_token_format(first_state["token"].as_str().unwrap());
    assert!(
        state_path
            .parent()
            .unwrap()
            .join("locks/state.lock")
            .is_file()
    );

    fixture
        .store
        .save(EntryProfileRecord::default())
        .expect("save the same provider inputs");
    assert_eq!(fs::read(&state_path).unwrap(), first_state_bytes);

    fixture
        .store
        .update_setting("..entry.git.name", "Fixture User".to_owned())
        .expect("update a non-provider variable");
    assert_eq!(fs::read(&state_path).unwrap(), first_state_bytes);

    let other_target = fixture.root.join("other-target");
    fs::create_dir(&other_target).expect("create another target project");
    fixture
        .store
        .update_setting(
            "..entry.project.root",
            other_target.to_string_lossy().into_owned(),
        )
        .expect("update the target project without changing setup inputs");
    assert_eq!(fs::read(&state_path).unwrap(), first_state_bytes);

    fixture
        .store
        .update_setting(".dev.bun.version", "1.2.16".to_owned())
        .expect("update one provider input");
    let changed_state: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    assert_ne!(changed_state["inputRevision"], first_state["inputRevision"]);
    assert_ne!(changed_state["token"], first_state["token"]);
    assert_token_format(changed_state["token"].as_str().unwrap());
}

#[test]
fn replace_uses_the_same_provider_invalidation_transaction() {
    let fixture = Fixture::new();
    fixture
        .store
        .save(EntryProfileRecord::default())
        .expect("save initial profile");
    let state_path = provider_state::state_path(&fixture.data_root);
    let before: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();

    let mut replacement = EntryProfileRecord::default();
    replacement.development.rust.toolchain = "beta".to_owned();
    let document = fixture.store.replace(replacement).expect("replace profile");
    let after: Value = serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();

    assert_eq!(
        after["inputRevision"],
        document.profile.environment_input_revision()
    );
    assert_ne!(after["token"], before["token"]);
}

#[test]
fn provider_state_failure_prevents_profile_publication() {
    let fixture = Fixture::new();
    let state_path = provider_state::state_path(&fixture.data_root);
    fs::create_dir_all(&state_path).expect("block the provider state file with a directory");

    let error = fixture
        .store
        .save(EntryProfileRecord::default())
        .expect_err("provider state failure must abort the profile transaction");

    assert!(error.to_string().contains("must be a regular file"));
    assert!(!fixture.store.path().exists());
}

#[test]
fn profile_publication_failure_restores_the_previous_provider_state() {
    let fixture = Fixture::new();
    fixture
        .store
        .save(EntryProfileRecord::default())
        .expect("save initial profile");
    let profile_path = fixture.store.path();
    let state_path = provider_state::state_path(&fixture.data_root);
    let profile_before = fs::read(&profile_path).unwrap();
    let state_before = fs::read(&state_path).unwrap();
    let _profile_reader = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&profile_path)
        .expect("hold a reader that prevents replacement");

    let error = fixture
        .store
        .update_setting(".dev.bun.version", "1.2.16".to_owned())
        .expect_err("locked profile publication must fail");

    assert!(error.to_string().contains("cannot publish entry profile"));
    assert_eq!(fs::read(&profile_path).unwrap(), profile_before);
    assert_eq!(fs::read(&state_path).unwrap(), state_before);
}

#[test]
fn profile_publication_failure_removes_a_new_provider_state() {
    let fixture = Fixture::new();
    fixture
        .store
        .save(EntryProfileRecord::default())
        .expect("save initial profile");
    let profile_path = fixture.store.path();
    let state_path = provider_state::state_path(&fixture.data_root);
    let profile_before = fs::read(&profile_path).unwrap();
    fs::remove_file(&state_path).expect("remove the existing provider state");
    let _profile_reader = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(&profile_path)
        .expect("hold a reader that prevents replacement");

    let error = fixture
        .store
        .update_setting(".dev.bun.version", "1.2.16".to_owned())
        .expect_err("locked profile publication must fail");

    assert!(error.to_string().contains("cannot publish entry profile"));
    assert_eq!(fs::read(&profile_path).unwrap(), profile_before);
    assert!(!state_path.exists());
}

#[test]
fn non_provider_profile_updates_do_not_wait_for_the_provider_state_lock() {
    let fixture = Fixture::new();
    fixture
        .store
        .save(EntryProfileRecord::default())
        .expect("save initial profile");
    let state_path = provider_state::state_path(&fixture.data_root);
    let state_before = fs::read(&state_path).unwrap();
    let _state_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(state_path.parent().unwrap().join("locks/state.lock"))
        .expect("hold the provider state lock");

    let document = fixture
        .store
        .update_setting("..entry.git.name", "Fixture User".to_owned())
        .expect("non-provider update must not acquire the provider state lock");

    assert_eq!(document.profile.git.name, "Fixture User");
    assert_eq!(fs::read(&state_path).unwrap(), state_before);
}

fn assert_revision_format(value: &str) {
    assert_eq!(value.len(), 71);
    assert!(value.starts_with("sha256-"));
    assert!(
        value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
}

fn assert_token_format(value: &str) {
    assert_eq!(value.len(), 32);
    assert!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
}

fn fixture_absolute_path(name: &str) -> String {
    std::env::temp_dir()
        .join(name)
        .to_string_lossy()
        .into_owned()
}
