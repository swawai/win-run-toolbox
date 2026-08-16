mod environment;
mod query;

use std::ffi::OsString;
use std::io::{self, Read};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};

use crate::command_event::{CapturedCommandEvent, CommandEventFrameDecoder, CommandProgress};
use crate::launch::{WORKER_PROTOCOL_ENV, WORKER_PROTOCOL_VERSION};
use crate::process_job::OwnedProcessJob;
use crate::utf8_output::Utf8LossyDecoder;
use environment::current_user_environment;
pub(crate) use query::{EntryQueryOutput, run_entry_query};
const OUTPUT_READ_BUFFER_SIZE: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EntryRunSpec {
    pub entry_file: PathBuf,
    pub working_directory: PathBuf,
    pub argv: Vec<OsString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryOutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EntryRunOutcome {
    Exited(i32),
    Failed(String),
}

pub(crate) trait EntryRunObserver: Send + Sync {
    fn output(&self, stream: EntryOutputStream, text: String);
    fn progress(&self, progress: CommandProgress);
    fn completed(&self, outcome: EntryRunOutcome);
}

pub(crate) trait EntryRunControl: Send + Sync {
    fn cancel(&self) -> io::Result<()>;
    fn join(&self) -> Result<(), String>;
}

pub(crate) trait EntryRunner: Send + Sync {
    /// Starts one run. On `Err`, the runner must not later report completion.
    fn start(
        &self,
        spec: EntryRunSpec,
        observer: Arc<dyn EntryRunObserver>,
    ) -> io::Result<Arc<dyn EntryRunControl>>;
}

#[derive(Debug, Default)]
pub(crate) struct NativeEntryRunner;

impl EntryRunner for NativeEntryRunner {
    fn start(
        &self,
        spec: EntryRunSpec,
        observer: Arc<dyn EntryRunObserver>,
    ) -> io::Result<Arc<dyn EntryRunControl>> {
        let job = Arc::new(OwnedProcessJob::create()?);
        let mut command = entry_command(&spec)?;
        let mut child = command.spawn().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "cannot start the Entry Launcher '{}': {error}",
                    spec.entry_file.display()
                ),
            )
        })?;
        if let Err(error) = job.assign_and_resume(&mut child) {
            return Err(failed_before_start(
                child,
                &job,
                &format!("cannot establish the Entry Launcher process boundary: {error}"),
            ));
        }
        start_monitored(child, job, observer)
    }
}

fn entry_command(spec: &EntryRunSpec) -> io::Result<Command> {
    let environment = current_user_environment()?;
    let mut command = Command::new(&spec.entry_file);
    command
        .args(&spec.argv)
        .current_dir(&spec.working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
        .env_clear()
        .envs(environment)
        .env(WORKER_PROTOCOL_ENV, WORKER_PROTOCOL_VERSION);
    Ok(command)
}

fn start_monitored(
    mut child: Child,
    job: Arc<OwnedProcessJob>,
    observer: Arc<dyn EntryRunObserver>,
) -> io::Result<Arc<dyn EntryRunControl>> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Entry Launcher stdout pipe is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| io::Error::other("Entry Launcher stderr pipe is unavailable"))?;
    let stdout_thread = spawn_reader(stdout, EntryOutputStream::Stdout, Arc::clone(&observer))?;
    let stderr_thread = match spawn_reader(stderr, EntryOutputStream::Stderr, Arc::clone(&observer))
    {
        Ok(thread) => thread,
        Err(error) => {
            let _ = job.cancel();
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_thread.join();
            return Err(error);
        }
    };

    let monitor_job = Arc::clone(&job);
    let monitor = thread::Builder::new()
        .name("swawkit-command-monitor".to_owned())
        .spawn(move || {
            let wait = child.wait();
            let cleanup = monitor_job.terminate_remaining();
            let stdout = join_reader(stdout_thread, "stdout");
            let stderr = join_reader(stderr_thread, "stderr");
            let outcome = match (wait, cleanup, stdout, stderr) {
                (Ok(status), Ok(()), Ok(()), Ok(())) => {
                    EntryRunOutcome::Exited(status.code().unwrap_or(1))
                }
                (wait, cleanup, stdout, stderr) => EntryRunOutcome::Failed(monitor_error(
                    wait.err(),
                    cleanup.err(),
                    stdout.err(),
                    stderr.err(),
                )),
            };
            observer.completed(outcome);
        })
        .map_err(|error| {
            let _ = job.cancel();
            error
        })?;

    Ok(Arc::new(NativeEntryRunControl {
        job,
        monitor: Mutex::new(Some(monitor)),
    }))
}

fn failed_before_start(mut child: Child, job: &OwnedProcessJob, reason: &str) -> io::Error {
    let _ = job.cancel();
    let _ = child.kill();
    let output = child.wait_with_output();
    let detail = output
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stderr).trim().to_owned())
        .filter(|detail| !detail.is_empty());
    io::Error::other(match detail {
        Some(detail) => format!("{reason}: {detail}"),
        None => reason.to_owned(),
    })
}

fn spawn_reader<R>(
    reader: R,
    stream: EntryOutputStream,
    observer: Arc<dyn EntryRunObserver>,
) -> io::Result<JoinHandle<io::Result<()>>>
where
    R: Read + Send + 'static,
{
    thread::Builder::new()
        .name(
            match stream {
                EntryOutputStream::Stdout => "swawkit-command-stdout",
                EntryOutputStream::Stderr => "swawkit-command-stderr",
            }
            .to_owned(),
        )
        .spawn(move || read_output(reader, stream, observer))
}

fn read_output(
    mut reader: impl Read,
    stream: EntryOutputStream,
    observer: Arc<dyn EntryRunObserver>,
) -> io::Result<()> {
    // The Web command-run wire format is UTF-8 text. Adapters and executable
    // modules own that output contract; invalid bytes remain visible as U+FFFD
    // instead of being guessed with a machine-specific legacy code page.
    let mut buffer = [0_u8; OUTPUT_READ_BUFFER_SIZE];
    let mut decoder = Utf8LossyDecoder::default();
    let mut frame_decoder = CommandEventFrameDecoder::default();
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            if let Some(text) = decoder.decode(&[], true) {
                dispatch_output(&observer, stream, frame_decoder.push(&text));
            }
            dispatch_output(&observer, stream, frame_decoder.finish());
            return Ok(());
        }
        if let Some(text) = decoder.decode(&buffer[..count], false) {
            dispatch_output(&observer, stream, frame_decoder.push(&text));
        }
    }
}

fn dispatch_output(
    observer: &Arc<dyn EntryRunObserver>,
    stream: EntryOutputStream,
    events: Vec<CapturedCommandEvent>,
) {
    for event in events {
        match event {
            CapturedCommandEvent::Output(text) => observer.output(stream, text),
            CapturedCommandEvent::Progress(progress) => observer.progress(progress),
        }
    }
}

fn join_reader(thread: JoinHandle<io::Result<()>>, stream: &str) -> Result<(), String> {
    thread
        .join()
        .map_err(|_| format!("Entry Launcher {stream} reader panicked"))?
        .map_err(|error| format!("Entry Launcher {stream} reader failed: {error}"))
}

fn monitor_error(
    wait: Option<io::Error>,
    cleanup: Option<io::Error>,
    stdout: Option<String>,
    stderr: Option<String>,
) -> String {
    let mut errors = Vec::new();
    if let Some(error) = wait {
        errors.push(format!("cannot wait for the Entry Launcher: {error}"));
    }
    if let Some(error) = cleanup {
        errors.push(error.to_string());
    }
    errors.extend(stdout);
    errors.extend(stderr);
    errors.join("; ")
}

struct NativeEntryRunControl {
    job: Arc<OwnedProcessJob>,
    monitor: Mutex<Option<JoinHandle<()>>>,
}

impl EntryRunControl for NativeEntryRunControl {
    fn cancel(&self) -> io::Result<()> {
        self.job.cancel()
    }

    fn join(&self) -> Result<(), String> {
        let monitor = self
            .monitor
            .lock()
            .map_err(|_| "command worker monitor is unavailable".to_owned())?
            .take();
        if let Some(monitor) = monitor {
            monitor
                .join()
                .map_err(|_| "command worker monitor panicked".to_owned())?;
        }
        Ok(())
    }
}

impl Drop for NativeEntryRunControl {
    fn drop(&mut self) {
        let _ = self.job.cancel();
    }
}

#[cfg(test)]
mod tests;
