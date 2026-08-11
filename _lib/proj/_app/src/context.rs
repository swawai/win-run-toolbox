use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::launch::LaunchRequest;

/// Entry identity and installation facts after launch transport has been removed.
#[derive(Debug, Clone)]
pub struct EntryContext {
    pub swawkit_home: PathBuf,
    pub entry_file: PathBuf,
    pub entry_name: String,
    pub invocation_directory: PathBuf,
    pub product_executable: PathBuf,
}

impl EntryContext {
    pub fn from_launch(request: &LaunchRequest) -> Result<Self, ContextError> {
        Self::from_product_launch(request, "swawkit-proj.exe")
    }

    pub fn from_host_launch(request: &LaunchRequest) -> Result<Self, ContextError> {
        Self::from_product_launch(request, "swawkit-proj-host.exe")
    }

    fn from_product_launch(
        request: &LaunchRequest,
        executable_name: &str,
    ) -> Result<Self, ContextError> {
        let executable = env::current_exe().map_err(|error| {
            ContextError::new(format!("cannot locate the shared Proj executable: {error}"))
        })?;
        Self::from_sources(request, &executable, executable_name)
    }

    pub fn kernel_root(&self) -> PathBuf {
        self.swawkit_home.join("_lib").join("proj")
    }

    pub fn sibling_product_executable(&self, name: &str) -> PathBuf {
        self.product_executable.with_file_name(name)
    }

    fn from_sources(
        request: &LaunchRequest,
        executable: &Path,
        executable_name: &str,
    ) -> Result<Self, ContextError> {
        let swawkit_home = derive_swawkit_home(executable, executable_name)?;

        let entry_file = absolute_path(&request.entry_file, "project entry file")?;
        if !entry_file.is_file() {
            return Err(ContextError::new(format!(
                "declared project entry file does not exist: {}",
                entry_file.display()
            )));
        }
        let entry_name = entry_file
            .file_stem()
            .and_then(OsStr::to_str)
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| {
                ContextError::new(format!(
                    "the project entry file has no usable Unicode name: {}",
                    entry_file.display()
                ))
            })?
            .to_owned();

        let invocation_directory = absolute_path(&request.invocation_dir, "invocation directory")?;
        if !invocation_directory.is_dir() {
            return Err(ContextError::new(format!(
                "invocation directory does not exist: {}",
                invocation_directory.display()
            )));
        }

        Ok(Self {
            swawkit_home,
            entry_file,
            entry_name,
            invocation_directory,
            product_executable: executable.to_path_buf(),
        })
    }
}

fn derive_swawkit_home(executable: &Path, executable_name: &str) -> Result<PathBuf, ContextError> {
    let executable = absolute_path(executable, "shared Proj executable")?;
    if executable.file_name() != Some(OsStr::new(executable_name)) {
        return Err(invalid_layout(&executable, executable_name));
    }
    let release_directory = executable
        .parent()
        .ok_or_else(|| invalid_layout(&executable, executable_name))?;
    let release_id = release_directory
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|value| is_release_id(value))
        .ok_or_else(|| invalid_layout(&executable, executable_name))?;
    debug_assert_eq!(release_id.len(), 64);
    let releases_directory = expected_parent(release_directory, "releases", executable_name)?;
    let runtime_directory = expected_parent(releases_directory, "_bin", executable_name)?;
    let kernel_root = expected_parent(runtime_directory, "proj", executable_name)?;
    let library_root = expected_parent(kernel_root, "_lib", executable_name)?;
    let swawkit_home = library_root
        .parent()
        .ok_or_else(|| invalid_layout(&executable, executable_name))?;
    if !swawkit_home.is_dir() {
        return Err(ContextError::new(format!(
            "derived SWAWKIT_HOME does not exist: {}",
            swawkit_home.display()
        )));
    }
    Ok(swawkit_home.to_path_buf())
}

fn expected_parent<'a>(
    path: &'a Path,
    name: &str,
    executable_name: &str,
) -> Result<&'a Path, ContextError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_layout(path, executable_name))?;
    if parent.file_name() != Some(OsStr::new(name)) {
        return Err(invalid_layout(path, executable_name));
    }
    Ok(parent)
}

fn invalid_layout(path: &Path, executable_name: &str) -> ContextError {
    ContextError::new(format!(
        "shared Proj executable must belong to '_lib\\proj\\_bin\\releases\\<release-id>\\{executable_name}': {}",
        path.display()
    ))
}

fn is_release_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn absolute_path(path: &Path, label: &str) -> Result<PathBuf, ContextError> {
    std::path::absolute(path).map_err(|error| {
        ContextError::new(format!(
            "invalid {label} path '{}': {error}",
            path.display()
        ))
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextError {
    message: String,
}

impl ContextError {
    fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for ContextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ContextError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::LaunchMode;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        executable: PathBuf,
        entry_file: PathBuf,
        invocation_dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root =
                env::temp_dir().join(format!("swawkit-context-{}-{sequence}", std::process::id()));
            let executable = root.join(format!(
                "_lib/proj/_bin/releases/{}/swawkit-proj.exe",
                "a".repeat(64)
            ));
            let entry_file = root.join("Favorites/project-one.exe");
            let invocation_dir = root.join("work");
            for directory in [
                executable.parent().expect("executable parent"),
                entry_file.parent().expect("entry parent"),
                &invocation_dir,
            ] {
                fs::create_dir_all(directory).expect("create fixture directory");
            }
            fs::write(&executable, "fixture").expect("write executable");
            fs::write(&entry_file, "fixture").expect("write entry file");

            Self {
                root,
                executable,
                entry_file,
                invocation_dir,
            }
        }

        fn request(&self) -> LaunchRequest {
            LaunchRequest {
                mode: LaunchMode::Cli,
                entry_file: self.entry_file.clone(),
                invocation_dir: self.invocation_dir.clone(),
                argv: Vec::new(),
            }
        }

        fn context(&self) -> Result<EntryContext, ContextError> {
            EntryContext::from_sources(&self.request(), &self.executable, "swawkit-proj.exe")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn derives_owned_identity_instead_of_trusting_spoofable_names() {
        let fixture = Fixture::new();
        let context = fixture.context().expect("entry context");

        assert_eq!(context.swawkit_home, fixture.root);
        assert_eq!(context.entry_name, "project-one");
        assert_eq!(context.entry_file, fixture.entry_file);
        assert_eq!(context.invocation_directory, fixture.invocation_dir);
    }

    #[test]
    fn rejects_an_executable_outside_the_shared_runtime_layout() {
        let fixture = Fixture::new();
        let misplaced = fixture.root.join("swawkit-proj.exe");
        let error = EntryContext::from_sources(&fixture.request(), &misplaced, "swawkit-proj.exe")
            .unwrap_err();

        assert!(error.to_string().contains("_lib\\proj\\_bin"));
    }

    #[test]
    fn validates_owned_filesystem_facts_without_project_declarations() {
        let fixture = Fixture::new();
        fs::remove_file(&fixture.entry_file).expect("remove entry fixture");
        assert!(
            fixture
                .context()
                .unwrap_err()
                .to_string()
                .contains("entry file does not exist")
        );
    }
}
