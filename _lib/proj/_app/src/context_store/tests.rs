use super::*;
use std::collections::BTreeSet;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    store: ContextStore,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-context-store-{}-{sequence}",
            std::process::id()
        ));
        let data_root = root.join("data/proj.fixture");
        fs::create_dir_all(&data_root).expect("create Context fixture DataRoot");
        let module_data_root = data_root.join("modules/kernel/.context");
        Self {
            root,
            store: ContextStore::new(data_root, module_data_root, Default::default()),
        }
    }

    fn path(&self, id: &str) -> PathBuf {
        self.store.directory().join(id).join("_resource.json")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn command(source: CommandSource, address: &str) -> ContextCommand {
    ContextCommand {
        source,
        address: address.to_owned(),
    }
}

#[test]
fn creates_updates_reads_and_deletes_one_strict_json_record() {
    let fixture = Fixture::new();
    fixture.store.create("build-app").expect("create Context");
    fixture
        .store
        .add_commands(
            "build-app",
            vec![
                command(CommandSource::Kernel, ".dev.status"),
                command(CommandSource::Action, "proj.build.app"),
            ],
        )
        .expect("add commands");
    fixture
        .store
        .append_note("build-app", "检查开发环境。".to_owned())
        .expect("append note");
    let record = fixture
        .store
        .set_prompt("build-app", "编译 app。".to_owned())
        .expect("set prompt");

    assert_eq!(record.commands.len(), 2);
    assert_eq!(record.notes[0], "检查开发环境。");
    assert_eq!(record.prompt, "编译 app。");
    assert_eq!(fixture.store.read("build-app").unwrap(), record);
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.path("build-app")).unwrap()).unwrap();
    assert_eq!(json["schema"], CONTEXT_SCHEMA);
    assert_eq!(json["commands"][0]["source"], "kernel");

    fixture.store.delete("build-app").expect("delete Context");
    assert!(!fixture.path("build-app").exists());
}

#[test]
fn add_is_ordered_and_idempotent_while_remove_reports_a_miss() {
    let fixture = Fixture::new();
    fixture.store.create("ordered").unwrap();
    let status = command(CommandSource::Kernel, ".dev.status");
    let setup = command(CommandSource::Kernel, ".dev.setup");
    fixture
        .store
        .add_commands("ordered", vec![status.clone(), setup.clone(), status])
        .unwrap();
    fixture.store.add_commands("ordered", vec![setup]).unwrap();

    let record = fixture.store.read("ordered").unwrap();
    assert_eq!(
        record
            .commands
            .iter()
            .map(|command| command.address.as_str())
            .collect::<Vec<_>>(),
        [".dev.status", ".dev.setup"]
    );
    assert!(
        fixture
            .store
            .remove_commands("ordered", &[".missing".to_owned()])
            .unwrap_err()
            .to_string()
            .contains("none of")
    );
    assert_eq!(fixture.store.read("ordered").unwrap(), record);
}

#[test]
fn list_is_sorted_and_rejects_corrupted_records() {
    let fixture = Fixture::new();
    fixture.store.create("zeta").unwrap();
    fixture.store.create("alpha").unwrap();
    assert_eq!(
        fixture
            .store
            .list()
            .unwrap()
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        ["alpha", "zeta"]
    );

    fs::write(fixture.path("alpha"), r#"{"schema":"wrong"}"#).unwrap();
    assert!(
        fixture
            .store
            .list()
            .unwrap_err()
            .to_string()
            .contains("invalid Context JSON")
    );
}

#[test]
fn static_child_data_is_ignored_but_its_resource_id_is_reserved() {
    let fixture = Fixture::new();
    let store = ContextStore::new(
        fixture.store.data_root.clone(),
        fixture.store.module_data_root.clone(),
        BTreeSet::from(["add".to_owned(), "list".to_owned()]),
    );
    let command_data = store.directory().join("add");
    fs::create_dir_all(&command_data).unwrap();
    fs::write(command_data.join("run.jsonl"), "{}").unwrap();

    assert!(store.list().unwrap().is_empty());
    assert!(
        store
            .create("add")
            .unwrap_err()
            .to_string()
            .contains("reserved")
    );

    fs::create_dir(store.directory().join("unknown")).unwrap();
    assert!(store.list().is_err());
}

#[test]
fn validates_ids_text_limits_and_publication_boundaries() {
    let fixture = Fixture::new();
    for id in ["", "Upper", "has_underscore", "con", &"a".repeat(65)] {
        assert!(
            fixture.store.create(id).is_err(),
            "unexpected valid ID: {id}"
        );
    }
    fixture.store.create("safe-id1").unwrap();
    assert!(
        fixture
            .store
            .append_note("safe-id1", " \n ".to_owned())
            .is_err()
    );
    assert!(
        fixture
            .store
            .set_prompt("safe-id1", "x".repeat(MAX_PROMPT_BYTES + 1))
            .is_err()
    );

    let path = fixture.path("safe-id1");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(
        fixture
            .store
            .read("safe-id1")
            .unwrap_err()
            .to_string()
            .contains("regular file")
    );
}

#[test]
fn concurrent_updates_are_serialized_without_losing_notes() {
    let fixture = Fixture::new();
    fixture.store.create("concurrent").unwrap();
    let store = Arc::new(fixture.store.clone());
    let barrier = Arc::new(Barrier::new(4));
    let threads = (0..4)
        .map(|index| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .append_note("concurrent", format!("note-{index}"))
                    .unwrap();
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().unwrap();
    }

    let mut notes = store.read("concurrent").unwrap().notes;
    notes.sort();
    assert_eq!(notes, ["note-0", "note-1", "note-2", "note-3"]);
}
