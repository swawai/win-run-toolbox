use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    context: EntryContext,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-runtime-release-{}-{sequence}",
            std::process::id()
        ));
        let runtime_root = root.join("_lib/proj/_bin");
        fs::create_dir_all(runtime_root.join("releases")).expect("create Runtime root");
        let running = "a".repeat(64);
        let selected = "b".repeat(64);
        fs::write(runtime_root.join("current"), format!("{selected}\n")).expect("write selector");
        Self {
            context: EntryContext {
                swawkit_home: root.clone(),
                entry_file: root.join("entry.exe"),
                entry_name: "entry".to_owned(),
                invocation_directory: root.clone(),
                product_executable: runtime_root
                    .join("releases")
                    .join(&running)
                    .join("swawkit-proj-host.exe"),
                release_id: running,
            },
            root,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn reads_the_selected_release_independently_of_the_running_release() {
    let fixture = Fixture::new();
    assert_eq!(
        selected_release_id(&fixture.context).unwrap(),
        "b".repeat(64)
    );
    assert_eq!(fixture.context.release_id, "a".repeat(64));
}

#[test]
fn rejects_noncanonical_selector_content() {
    let fixture = Fixture::new();
    fs::write(
        fixture.context.kernel_root().join("_bin/current"),
        format!("{}\r\n", "B".repeat(64)),
    )
    .expect("replace selector");
    assert!(selected_release_id(&fixture.context).is_err());
}
