use super::*;
use crate::binding::SWAWKIT_HOME_PLACEHOLDER;
use crate::data_root::DataRootLock;
use serde_json::Value;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::thread;
use std::time::Duration;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    data_root: PathBuf,
    store: EntryProfileStore,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("swawkit-profile-{}-{sequence}", std::process::id()));
        let home = root.join("home");
        let data_root = home.join("data/proj.fixture");
        fs::create_dir_all(&data_root).expect("create fixture directories");
        let store = EntryProfileStore::new(&home, &data_root);
        Self {
            root,
            home,
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
fn distinguishes_missing_invalid_and_ready_profiles() {
    let fixture = Fixture::new();
    assert!(matches!(
        fixture.store.read(),
        EntryProfileState::Missing { .. }
    ));

    fs::write(fixture.store.path(), "not-json").expect("write invalid profile");
    assert!(matches!(
        fixture.store.read(),
        EntryProfileState::Invalid { .. }
    ));

    fixture
        .store
        .save(EntryProfileRecord::default())
        .expect("save profile");
    let EntryProfileState::Ready(profile) = fixture.store.read() else {
        panic!("expected ready profile");
    };
    assert_eq!(profile.binding().target_project_root(), fixture.home);
    assert_eq!(profile.binding().action_root(), fixture.home.join(".swaw"));
}

#[test]
fn rejects_a_profile_document_without_an_explicit_schema() {
    let mut document = serde_json::to_value(EntryProfileRecord::default()).unwrap();
    document.as_object_mut().unwrap().remove("schema");

    let error = serde_json::from_value::<EntryProfileRecord>(document).unwrap_err();

    assert!(error.to_string().contains("missing field `schema`"));
}

#[test]
fn maps_every_mutable_profile_field_to_one_environment_variable() {
    let fields = EntryProfileRecord::mutable_string_field_paths();
    let mapped_fields = EntryProfileRecord::environment_variable_fields();
    let names = EntryProfileRecord::environment_variable_names();
    let commands = EntryProfileRecord::environment_variable_commands();
    let values = EntryProfileRecord::default().environment_variable_values();

    assert_eq!(fields.len(), 32);
    assert_eq!(mapped_fields.len(), fields.len());
    assert_eq!(names.len(), fields.len());
    assert_eq!(commands.len(), fields.len());
    assert_eq!(values.len(), fields.len());
    assert_eq!(
        mapped_fields
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        fields
            .iter()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>()
    );
    assert!(names.iter().all(|name| name.starts_with("SWAWKIT_PROJ_")));
    assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        commands
            .iter()
            .map(|(group, _)| *group)
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "bun",
            "git",
            "go",
            "msvc",
            "preferences",
            "project",
            "pwsh",
            "python",
            "rust",
            "system",
            "uv",
        ])
    );
    assert_eq!(
        commands
            .iter()
            .map(|(_, name)| *name)
            .collect::<std::collections::BTreeSet<_>>(),
        names.into_iter().collect::<std::collections::BTreeSet<_>>()
    );
}

#[test]
fn profile_document_and_variable_updates_share_the_atomic_store() {
    let fixture = Fixture::new();
    let missing = fixture.store.document();
    assert_eq!(missing.protocol, "swawkit.entry-profile-state/v3");
    assert_eq!(missing.revision, "missing");
    assert_eq!(missing.status, "setupRequired");
    assert!(!missing.required_complete);

    let ready = fixture
        .store
        .update_environment_variable("SWAWKIT_PROJ_GIT_ID_EMAIL", "dev@example.com".to_owned())
        .expect("update known environment variable");
    assert_eq!(ready.status, "ready");
    assert!(ready.revision.starts_with("sha256-"));
    assert_eq!(ready.profile.git.email, "dev@example.com");

    let before = fs::read(fixture.store.path()).unwrap();
    assert!(
        fixture
            .store
            .update_environment_variable("SWAWKIT_PROJ_UNKNOWN", "value".to_owned())
            .unwrap_err()
            .to_string()
            .contains("unknown Entry Profile environment variable")
    );
    assert_eq!(fs::read(fixture.store.path()).unwrap(), before);

    fs::write(fixture.store.path(), "not-json").unwrap();
    let invalid_before = fs::read(fixture.store.path()).unwrap();
    assert!(
        fixture
            .store
            .update_environment_variable("SWAWKIT_PROJ_GIT_ID_NAME", "User".to_owned())
            .unwrap_err()
            .to_string()
            .contains("current profile is unreadable")
    );
    assert_eq!(fs::read(fixture.store.path()).unwrap(), invalid_before);
}

#[test]
fn revision_is_stable_for_the_same_file_and_detects_stale_replacements() {
    let fixture = Fixture::new();
    let first = fixture
        .store
        .replace(EntryProfileRecord::default())
        .expect("create profile");
    assert_eq!(fixture.store.document().revision, first.revision);

    let second = fixture
        .store
        .update_environment_variable("SWAWKIT_PROJ_GIT_ID_NAME", "CLI Writer".to_owned())
        .expect("update profile");
    assert_ne!(second.revision, first.revision);

    assert!(matches!(
        fixture.store.update_environment_variable_if_revision(
            &first.revision,
            "SWAWKIT_PROJ_GIT_ID_EMAIL",
            "stale@example.com".to_owned(),
        ),
        Err(ProfileUpdateError::Conflict { current_revision })
            if current_revision == second.revision
    ));
    assert_eq!(fixture.store.document().profile.git.name, "CLI Writer");
}

#[test]
fn variable_updates_wait_for_the_cross_process_data_lock() {
    let fixture = Fixture::new();
    let data_directory = fixture.data_root.parent().unwrap();
    let lock = DataRootLock::acquire_for_test(data_directory, 1, Duration::ZERO)
        .expect("hold DataRoot lock");
    let store = fixture.store.clone();
    let (finished, result) = mpsc::channel();
    let worker = thread::spawn(move || {
        let update = store.update_environment_variable(
            "SWAWKIT_PROJ_GIT_ID_NAME",
            "Serialized Writer".to_owned(),
        );
        finished.send(update).unwrap();
    });

    assert!(result.recv_timeout(Duration::from_millis(30)).is_err());
    drop(lock);
    result
        .recv_timeout(Duration::from_secs(1))
        .expect("update completes after lock release")
        .expect("update succeeds");
    worker.join().unwrap();
    assert_eq!(
        fixture.store.document().profile.git.name,
        "Serialized Writer"
    );
}

#[test]
fn saves_the_complete_explicit_profile_atomically() {
    let fixture = Fixture::new();
    let profile = EntryProfileRecord::default();
    fixture.store.save(profile).expect("save profile");
    let document: Value = serde_json::from_slice(&fs::read(fixture.store.path()).unwrap()).unwrap();

    assert_eq!(document["schema"], PROFILE_SCHEMA);
    assert_eq!(document["targetProjectRoot"], SWAWKIT_HOME_PLACEHOLDER);
    assert_eq!(document["development"]["rust"]["profile"], "minimal");
    assert_eq!(document["git"]["name"], "");
    assert!(fs::read_dir(&fixture.data_root).unwrap().all(|item| {
        !item
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".swawkit.")
    }));
}

#[test]
fn rejects_invalid_conditional_fields_without_overwriting() {
    let fixture = Fixture::new();
    fixture
        .store
        .save(EntryProfileRecord::default())
        .expect("save initial profile");
    let original = fs::read(fixture.store.path()).expect("read initial profile");

    let mut invalid = EntryProfileRecord::default();
    invalid.development.bun.version.clear();
    assert!(fixture.store.save(invalid).is_err());
    assert_eq!(fs::read(fixture.store.path()).unwrap(), original);

    let mut invalid = EntryProfileRecord::default();
    invalid.development.rust.host = "aarch64-pc-windows-msvc".to_owned();
    assert!(fixture.store.save(invalid).is_err());
    assert_eq!(fs::read(fixture.store.path()).unwrap(), original);

    let mut invalid = EntryProfileRecord::default();
    invalid.development.uv.mode = "managed".to_owned();
    assert!(fixture.store.save(invalid).is_err());
    assert_eq!(fs::read(fixture.store.path()).unwrap(), original);
}

#[test]
fn powershell_uses_one_explicit_three_mode_contract() {
    let mut profile = EntryProfileRecord::default();
    profile.development.pwsh.mode = "system".to_owned();
    profile.validate().expect("system PowerShell 7 mode");

    profile.development.pwsh.mode = "disabled".to_owned();
    profile.validate().expect("disabled PowerShell mode");

    profile.development.pwsh.mode = "windows-powershell".to_owned();
    let error = profile.validate().unwrap_err().to_string();
    assert!(error.contains("managed, system, disabled"), "{error}");
}

#[test]
fn preserves_a_valid_document_when_its_target_moves() {
    let fixture = Fixture::new();
    let external = fixture.root.join("movable-project");
    fs::create_dir(&external).expect("create external project");
    let mut record = EntryProfileRecord::default();
    record.target_project_root = external.to_string_lossy().into_owned();
    fixture.store.save(record).expect("save external profile");
    fs::remove_dir(&external).expect("move external project away");

    let EntryProfileState::Invalid {
        record: Some(record),
        error,
        ..
    } = fixture.store.read()
    else {
        panic!("expected invalid profile");
    };
    assert_eq!(record.target_project_root, external.to_string_lossy());
    assert!(error.contains("does not exist"));
}
