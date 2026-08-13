use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(25);
const MAX_STREAM_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub(crate) struct CapturedProcess {
    pub(crate) exit_code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) fn run(mut command: Command, label: &str) -> Result<CapturedProcess, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start {label}: {error}"))?;
    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| abort(&mut child, format!("{label} stdout is unavailable")))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| abort(&mut child, format!("{label} stderr is unavailable")))?;
    let (failure_sender, failure_receiver) = mpsc::channel();
    let stdout = read_pipe(stdout_pipe, "stdout", failure_sender.clone());
    let stderr = read_pipe(stderr_pipe, "stderr", failure_sender);
    let started = Instant::now();
    let status = loop {
        if let Ok(error) = failure_receiver.try_recv() {
            let error = abort(&mut child, error);
            join_after_abort(stdout, stderr);
            return Err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < TIMEOUT => thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                let error = abort(
                    &mut child,
                    format!("{label} timed out after {} seconds", TIMEOUT.as_secs()),
                );
                join_after_abort(stdout, stderr);
                return Err(error);
            }
            Err(error) => {
                let error = abort(&mut child, format!("cannot wait for {label}: {error}"));
                join_after_abort(stdout, stderr);
                return Err(error);
            }
        }
    };
    let stdout = join_reader(stdout, "stdout");
    let stderr = join_reader(stderr, "stderr");
    let (stdout, stderr) = match (stdout, stderr) {
        (Ok(stdout), Ok(stderr)) => (stdout, stderr),
        (Err(error), _) | (_, Err(error)) => return Err(error),
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
    failure_sender: mpsc::Sender<String>,
) -> thread::JoinHandle<Result<Vec<u8>, String>> {
    thread::spawn(move || {
        let mut content = Vec::with_capacity(64 * 1024);
        let result = pipe
            .take((MAX_STREAM_BYTES + 1) as u64)
            .read_to_end(&mut content)
            .map_err(|error| format!("cannot read probe {name}: {error}"))
            .and_then(|_| {
                if content.len() > MAX_STREAM_BYTES {
                    Err(format!(
                        "probe {name} exceeded the {MAX_STREAM_BYTES} byte output limit"
                    ))
                } else {
                    Ok(content)
                }
            });
        if let Err(error) = &result {
            let _ = failure_sender.send(error.clone());
        }
        result
    })
}

fn join_reader(
    reader: thread::JoinHandle<Result<Vec<u8>, String>>,
    name: &str,
) -> Result<Vec<u8>, String> {
    reader
        .join()
        .map_err(|_| format!("probe {name} reader panicked"))?
}

fn join_after_abort(
    stdout: thread::JoinHandle<Result<Vec<u8>, String>>,
    stderr: thread::JoinHandle<Result<Vec<u8>, String>>,
) {
    let _ = stdout.join();
    let _ = stderr.join();
}

fn abort(child: &mut std::process::Child, error: String) -> String {
    let kill_error = child.kill().err();
    match child.wait() {
        Ok(_) => error,
        Err(wait_error) => format!(
            "{error}; additionally failed to reap the probe: {wait_error}{}",
            kill_error
                .map(|cause| format!(" (termination also failed: {cause})"))
                .unwrap_or_default()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn excessive_output_is_bounded_and_the_process_is_reaped() {
        let root = std::env::temp_dir().join(format!(
            "swawkit-process-probe-limit-{}",
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
        let executable = std::env::var_os("COMSPEC")
            .unwrap_or_else(|| OsString::from(r"C:\Windows\System32\cmd.exe"));
        let mut command = Command::new(executable);
        command
            .args(["/d", "/q", "/c"])
            .arg(&script)
            .current_dir(&root);

        let error = run(command, "fixture probe").unwrap_err();

        assert!(error.contains("exceeded"), "{error}");
        assert!(!root.join("survived.txt").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
