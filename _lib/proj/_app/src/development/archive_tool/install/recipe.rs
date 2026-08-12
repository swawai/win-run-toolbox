use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::development::{ArchiveToolContract, BUN, PWSH};

use super::super::{ArchiveToolError, ArchiveToolErrorKind, ResolvedDefinition};

const PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const PROBE_POLL_INTERVAL: Duration = Duration::from_millis(25);
const MAX_PROBE_STREAM_BYTES: usize = 1024 * 1024;
const BUNX_CONTENT: &[u8] = b"@echo off\r\n\"%~dp0bun.exe\" x %*\r\n";

pub(super) trait Recipe {
    fn prepare(
        &self,
        tool: &ArchiveToolContract,
        staged_root: &Path,
    ) -> Result<(), ArchiveToolError>;

    fn validate(
        &self,
        tool: &ArchiveToolContract,
        resolved: &ResolvedDefinition,
        staged_root: &Path,
    ) -> Result<(), ArchiveToolError>;
}

pub(super) struct NativeRecipe;

impl Recipe for NativeRecipe {
    fn prepare(
        &self,
        tool: &ArchiveToolContract,
        staged_root: &Path,
    ) -> Result<(), ArchiveToolError> {
        match tool.name {
            name if name == BUN.name => write_bunx(staged_root),
            name if name == PWSH.name => Ok(()),
            name => Err(unsupported(name)),
        }
    }

    fn validate(
        &self,
        tool: &ArchiveToolContract,
        resolved: &ResolvedDefinition,
        staged_root: &Path,
    ) -> Result<(), ArchiveToolError> {
        let executable = staged_root.join(tool.executable);
        let arguments: &[&str] = match tool.name {
            name if name == BUN.name => &["--version"],
            name if name == PWSH.name => &["-Version"],
            name => return Err(unsupported(name)),
        };
        let mut command = Command::new(&executable);
        command.args(arguments).current_dir(staged_root);
        let output = run_bounded_process(command)?;
        if output.exit_code != 0 {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::ProbeFailed,
                format!(
                    "staged {} version probe failed with exit code {}: {}",
                    tool.display_name, output.exit_code, output.stderr
                ),
            ));
        }
        let actual = if tool.name == BUN.name {
            output.stdout.as_str()
        } else {
            output.stdout.strip_prefix("PowerShell ").unwrap_or("")
        };
        if actual != resolved.version() {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::ProbeFailed,
                format!(
                    "staged {} reports '{}', expected '{}'.",
                    tool.display_name,
                    output.stdout,
                    if tool.name == PWSH.name {
                        format!("PowerShell {}", resolved.version())
                    } else {
                        resolved.version().to_owned()
                    }
                ),
            ));
        }
        Ok(())
    }
}

fn write_bunx(staged_root: &Path) -> Result<(), ArchiveToolError> {
    let path = staged_root.join("bunx.cmd");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|error| install_error("write the Bun shim", &path, error))?;
    file.write_all(BUNX_CONTENT)
        .map_err(|error| install_error("write the Bun shim", &path, error))?;
    file.sync_all()
        .map_err(|error| install_error("flush the Bun shim", &path, error))
}

#[derive(Debug)]
pub(crate) struct CapturedProcess {
    pub(crate) exit_code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) fn run_bounded_process(
    mut command: Command,
) -> Result<CapturedProcess, ArchiveToolError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            ArchiveToolError::new(
                ArchiveToolErrorKind::ProbeFailed,
                format!("cannot start staged executable: {error}"),
            )
        })?;
    let stdout_pipe = match child.stdout.take() {
        Some(pipe) => pipe,
        None => {
            return Err(abort_probe(
                &mut child,
                probe_pipe_error("probe stdout is unavailable"),
            ));
        }
    };
    let stderr_pipe = match child.stderr.take() {
        Some(pipe) => pipe,
        None => {
            return Err(abort_probe(
                &mut child,
                probe_pipe_error("probe stderr is unavailable"),
            ));
        }
    };
    let (failure_sender, failure_receiver) = mpsc::channel();
    let stdout = read_pipe(stdout_pipe, "stdout", failure_sender.clone());
    let stderr = read_pipe(stderr_pipe, "stderr", failure_sender);
    let started = Instant::now();
    let status = loop {
        if let Ok(error) = failure_receiver.try_recv() {
            let error = abort_probe(&mut child, error);
            join_after_abort(stdout, stderr);
            return Err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < PROBE_TIMEOUT => {
                thread::sleep(PROBE_POLL_INTERVAL);
            }
            Ok(None) => {
                let error = ArchiveToolError::new(
                    ArchiveToolErrorKind::ProbeFailed,
                    format!(
                        "the staged executable probe timed out after {} seconds",
                        PROBE_TIMEOUT.as_secs()
                    ),
                );
                let error = abort_probe(&mut child, error);
                join_after_abort(stdout, stderr);
                return Err(error);
            }
            Err(error) => {
                let error = ArchiveToolError::new(
                    ArchiveToolErrorKind::ProbeFailed,
                    format!("cannot wait for the staged executable: {error}"),
                );
                let error = abort_probe(&mut child, error);
                join_after_abort(stdout, stderr);
                return Err(error);
            }
        }
    };
    let stdout = join_reader(stdout, "stdout");
    let stderr = join_reader(stderr, "stderr");
    let (stdout, stderr) = match (stdout, stderr) {
        (Ok(stdout), Ok(stderr)) => (stdout, stderr),
        (Err(error), _) | (_, Err(error)) => return Err(abort_probe(&mut child, error)),
    };
    Ok(CapturedProcess {
        exit_code: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&stdout).trim().to_owned(),
        stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
    })
}

fn read_pipe(
    pipe: impl Read + Send + 'static,
    name: &'static str,
    failure_sender: mpsc::Sender<ArchiveToolError>,
) -> thread::JoinHandle<Result<Vec<u8>, ArchiveToolError>> {
    thread::spawn(move || {
        let result = read_pipe_bounded(pipe, name);
        if let Err(error) = &result {
            let _ = failure_sender.send(error.clone());
        }
        result
    })
}

fn read_pipe_bounded(pipe: impl Read, name: &'static str) -> Result<Vec<u8>, ArchiveToolError> {
    let mut content = Vec::with_capacity(64 * 1024);
    pipe.take((MAX_PROBE_STREAM_BYTES + 1) as u64)
        .read_to_end(&mut content)
        .map_err(|error| {
            ArchiveToolError::new(
                ArchiveToolErrorKind::ProbeFailed,
                format!("cannot read probe {name}: {error}"),
            )
        })?;
    if content.len() > MAX_PROBE_STREAM_BYTES {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::ProbeFailed,
            format!(
                "probe {name} exceeded the {} byte output limit",
                MAX_PROBE_STREAM_BYTES
            ),
        ));
    }
    Ok(content)
}

fn join_reader(
    reader: thread::JoinHandle<Result<Vec<u8>, ArchiveToolError>>,
    name: &str,
) -> Result<Vec<u8>, ArchiveToolError> {
    reader
        .join()
        .map_err(|_| probe_pipe_error(&format!("{name} reader panicked")))?
}

fn join_after_abort(
    stdout: thread::JoinHandle<Result<Vec<u8>, ArchiveToolError>>,
    stderr: thread::JoinHandle<Result<Vec<u8>, ArchiveToolError>>,
) {
    let _ = stdout.join();
    let _ = stderr.join();
}

fn abort_probe(child: &mut std::process::Child, error: ArchiveToolError) -> ArchiveToolError {
    let kill_error = child.kill().err();
    let wait_error = child.wait().err();
    if let Some(wait_error) = wait_error {
        return ArchiveToolError::new(
            ArchiveToolErrorKind::ProbeFailed,
            format!(
                "{error}; additionally failed to reap the staged executable: {wait_error}{}",
                kill_error
                    .map(|cause| format!(" (termination also failed: {cause})"))
                    .unwrap_or_default()
            ),
        );
    }
    error
}

fn unsupported(name: &str) -> ArchiveToolError {
    ArchiveToolError::new(
        ArchiveToolErrorKind::InvalidInstallRequest,
        format!("unsupported archive tool recipe '{name}'"),
    )
}

fn probe_pipe_error(message: &str) -> ArchiveToolError {
    ArchiveToolError::new(ArchiveToolErrorKind::ProbeFailed, message)
}

fn install_error(action: &str, path: &Path, error: std::io::Error) -> ArchiveToolError {
    ArchiveToolError::new(
        ArchiveToolErrorKind::InstallationFailed,
        format!("cannot {action} '{}': {error}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn bun_shim_is_byte_compatible_with_the_published_recipe() {
        let root =
            std::env::temp_dir().join(format!("swawkit-archive-recipe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();

        NativeRecipe.prepare(&BUN, &root).unwrap();

        assert_eq!(std::fs::read(root.join("bunx.cmd")).unwrap(), BUNX_CONTENT);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn excessive_probe_output_is_bounded_and_the_process_is_reaped() {
        let root = std::env::temp_dir().join(format!(
            "swawkit-archive-probe-limit-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        let script = root.join("flood.cmd");
        std::fs::write(
            &script,
            b"@echo off\r\nfor /L %%i in (1,1,20000) do @echo 012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789\r\nping.exe -n 6 127.0.0.1 >nul\r\n>\"%~dp0survived.txt\" echo survived\r\n",
        )
        .unwrap();
        let command = std::env::var_os("COMSPEC")
            .unwrap_or_else(|| OsString::from(r"C:\Windows\System32\cmd.exe"));
        let script = script.to_string_lossy();

        let mut process = Command::new(Path::new(&command));
        process
            .args(["/d", "/q", "/c", script.as_ref()])
            .current_dir(&root);
        let error = match run_bounded_process(process) {
            Ok(_) => panic!("excessive probe output must fail"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), ArchiveToolErrorKind::ProbeFailed);
        assert!(error.to_string().contains("exceeded"));
        assert!(!root.join("survived.txt").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
