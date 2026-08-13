use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

use crate::context::EntryContext;

const SELECTOR_BYTES: u64 = 65;

pub fn selected_release_id(context: &EntryContext) -> io::Result<String> {
    let runtime_root = context.kernel_root().join("_bin");
    regular_directory(&runtime_root, "Runtime root")?;
    regular_directory(&runtime_root.join("releases"), "Runtime releases directory")?;

    let selector = runtime_root.join("current");
    let mut file = open_regular_file(&selector)?;
    if file.metadata()?.len() != SELECTOR_BYTES {
        return Err(invalid_data(format!(
            "Runtime selector must contain exactly 64 lowercase hexadecimal bytes and a newline: {}",
            selector.display()
        )));
    }
    let mut bytes = Vec::with_capacity(SELECTOR_BYTES as usize);
    file.read_to_end(&mut bytes)?;
    let release_id = bytes
        .strip_suffix(b"\n")
        .filter(|value| is_release_id(value))
        .ok_or_else(|| invalid_data("Runtime selector content is invalid"))?;
    String::from_utf8(release_id.to_vec())
        .map_err(|_| invalid_data("Runtime selector is not UTF-8"))
}

fn regular_directory(path: &std::path::Path, label: &str) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || is_reparse(&metadata) {
        return Err(invalid_data(format!(
            "{label} must be a regular directory: {}",
            path.display()
        )));
    }
    Ok(())
}

fn open_regular_file(path: &std::path::Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || is_reparse(&metadata) {
        return Err(invalid_data(format!(
            "Runtime selector must be a regular file: {}",
            path.display()
        )));
    }
    Ok(file)
}

fn is_release_id(value: &[u8]) -> bool {
    value.len() == 64
        && value
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
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
            fs::write(runtime_root.join("current"), format!("{selected}\n"))
                .expect("write selector");
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
}
