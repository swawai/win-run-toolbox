use super::*;
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    context: EntryContext,
    identity: EntryIdentity,
}

impl Fixture {
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "swawkit-host-runtime-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create Host runtime fixture");
        Self {
            context: EntryContext {
                swawkit_home: root.clone(),
                entry_file: root.join("swawkit.exe"),
                entry_name: "swawkit".to_owned(),
                invocation_directory: root.clone(),
                product_executable: root.join("swawkit-proj-host.exe"),
            },
            identity: EntryIdentity::from_parts(
                r"\\?\volume{00000000-0000-0000-0000-000000000001}",
                "00000000000000000000000000000001",
            )
            .expect("fixture identity"),
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
fn publishes_and_removes_only_the_current_owner() {
    let fixture = Fixture::new();
    let locator = HostRuntimeLocator::new(&fixture.context, &fixture.identity);
    let first = locator.acquire_owner();
    let first_document = first.document("http://127.0.0.1:43127/").unwrap();
    first.publish(&first_document).unwrap();
    assert_eq!(locator.read().unwrap(), first_document);

    let second = locator.acquire_owner();
    let second_document = second.document("http://127.0.0.1:43128/").unwrap();
    second.publish(&second_document).unwrap();
    drop(first);
    assert_eq!(locator.read().unwrap(), second_document);

    drop(second);
    assert!(!locator.path().exists());
}

#[test]
fn rejects_non_loopback_or_mismatched_runtime_state() {
    let fixture = Fixture::new();
    let locator = HostRuntimeLocator::new(&fixture.context, &fixture.identity);
    let owner = locator.acquire_owner();
    assert!(owner.document("http://localhost:43127/").is_err());

    let mut document = owner.document("http://127.0.0.1:43127/").unwrap();
    document.entry_key_sha256 = "0".repeat(64);
    assert!(owner.publish(&document).is_err());
}

#[test]
fn health_probe_requires_the_published_boot_and_entry_identity() {
    let entry_key = "1".repeat(64);
    let valid = health_fixture(&entry_key, "boot-a", &entry_key, "boot-a");
    let valid_result = probe(&valid);
    assert!(valid_result.is_ok(), "{valid_result:?}");

    let mismatched = health_fixture(&entry_key, "boot-a", &entry_key, "boot-b");
    let error = probe(&mismatched).unwrap_err();
    assert!(error.to_string().contains("identity does not match"));
}

fn health_fixture(
    document_entry: &str,
    document_boot: &str,
    response_entry: &str,
    response_boot: &str,
) -> HostRuntimeDocument {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("health fixture listener");
    let address = listener.local_addr().expect("health fixture address");
    let response_entry = response_entry.to_owned();
    let response_boot = response_boot.to_owned();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("health fixture connection");
        let mut request = Vec::new();
        loop {
            let mut chunk = [0_u8; 256];
            let read = stream.read(&mut chunk).expect("health fixture request");
            request.extend_from_slice(&chunk[..read]);
            if read == 0 || request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        write!(
            stream,
            "HTTP/1.1 200 OK\r\n{HOST_BOOT_HEADER}: {response_boot}\r\n\
             {HOST_ENTRY_HEADER}: {response_entry}\r\nContent-Length: 3\r\n\
             Connection: close\r\n\r\nok\n"
        )
        .expect("health fixture response");
    });
    HostRuntimeDocument::new(
        document_entry,
        document_boot,
        std::process::id(),
        format!("http://{address}/"),
    )
    .expect("health fixture document")
}
