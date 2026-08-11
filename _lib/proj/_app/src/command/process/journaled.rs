use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, Stdio};
use std::thread::{self, JoinHandle};

use crate::catalog::CommandAdapter;
use crate::run_journal::{RunJournal, RunJournalPhase, RunJournalStream};
use crate::utf8_output::Utf8LossyDecoder;

use super::prepare_command;
use crate::command::{CommandError, CommandProcessMode, CommandResult, ProcessEnvironment};

const OUTPUT_READ_BUFFER_SIZE: usize = 8192;

pub(crate) fn run_process_journaled(
    adapter: CommandAdapter,
    entry_path: &Path,
    arguments: &[OsString],
    working_directory: &Path,
    environment: &ProcessEnvironment,
    process_mode: CommandProcessMode,
    journal: &RunJournal,
    phase: RunJournalPhase,
) -> CommandResult<i32> {
    let mut command = prepare_command(
        adapter,
        entry_path,
        arguments,
        working_directory,
        environment,
        process_mode,
    )?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        CommandError::new(format!(
            "cannot start command entry '{}': {error}",
            entry_path.display()
        ))
    })?;
    monitor_child(&mut child, journal, phase)
}

fn monitor_child(
    child: &mut Child,
    journal: &RunJournal,
    phase: RunJournalPhase,
) -> CommandResult<i32> {
    let Some(stdout) = child.stdout.take() else {
        stop_unmonitored_child(child);
        return Err(CommandError::new("command stdout pipe is unavailable"));
    };
    let Some(stderr) = child.stderr.take() else {
        stop_unmonitored_child(child);
        return Err(CommandError::new("command stderr pipe is unavailable"));
    };
    let stdout_thread = match spawn_reader(stdout, RunJournalStream::Stdout, journal.clone(), phase)
    {
        Ok(thread) => thread,
        Err(error) => {
            stop_unmonitored_child(child);
            return Err(CommandError::new(format!(
                "cannot start stdout reader: {error}"
            )));
        }
    };
    let stderr_thread = match spawn_reader(stderr, RunJournalStream::Stderr, journal.clone(), phase)
    {
        Ok(thread) => thread,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_reader(stdout_thread, "stdout");
            return Err(CommandError::new(format!(
                "cannot start stderr reader: {error}"
            )));
        }
    };

    let wait_result = child
        .wait()
        .map_err(|error| CommandError::new(format!("cannot wait for command entry: {error}")));
    let stdout = join_reader(stdout_thread, "stdout");
    let stderr = join_reader(stderr_thread, "stderr");
    let status = wait_result?;
    stdout?;
    stderr?;
    Ok(status.code().unwrap_or(1))
}

fn stop_unmonitored_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn_reader<R>(
    reader: R,
    stream: RunJournalStream,
    journal: RunJournal,
    phase: RunJournalPhase,
) -> io::Result<JoinHandle<io::Result<()>>>
where
    R: Read + Send + 'static,
{
    thread::Builder::new()
        .name(
            match stream {
                RunJournalStream::Stdout => "swawkit-cli-stdout",
                RunJournalStream::Stderr => "swawkit-cli-stderr",
            }
            .to_owned(),
        )
        .spawn(move || read_output(reader, stream, journal, phase))
}

fn read_output(
    mut reader: impl Read,
    stream: RunJournalStream,
    journal: RunJournal,
    phase: RunJournalPhase,
) -> io::Result<()> {
    let mut console = ConsoleWriter::new(stream);
    let mut decoder = Utf8LossyDecoder::default();
    let mut first_error = None;
    let mut journal_available = true;
    let mut buffer = [0_u8; OUTPUT_READ_BUFFER_SIZE];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(count) => count,
            Err(error) => {
                remember_first(&mut first_error, error);
                break;
            }
        };
        if count == 0 {
            if journal_available
                && let Some(text) = decoder.decode(&[], true)
                && let Err(error) = journal.output(phase, stream, text)
            {
                remember_first(&mut first_error, error);
            }
            break;
        }
        if let Err(error) = console.write(&buffer[..count]) {
            remember_first(&mut first_error, error);
        }
        if journal_available
            && let Some(text) = decoder.decode(&buffer[..count], false)
            && let Err(error) = journal.output(phase, stream, text)
        {
            journal_available = false;
            remember_first(&mut first_error, error);
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn join_reader(thread: JoinHandle<io::Result<()>>, stream: &str) -> CommandResult<()> {
    thread
        .join()
        .map_err(|_| CommandError::new(format!("command {stream} reader panicked")))?
        .map_err(|error| CommandError::new(format!("command {stream} reader failed: {error}")))
}

enum ConsoleWriter {
    Stdout(io::Stdout),
    Stderr(io::Stderr),
}

impl ConsoleWriter {
    fn new(stream: RunJournalStream) -> Self {
        match stream {
            RunJournalStream::Stdout => Self::Stdout(io::stdout()),
            RunJournalStream::Stderr => Self::Stderr(io::stderr()),
        }
    }

    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Self::Stdout(writer) => writer.write_all(bytes).and_then(|()| writer.flush()),
            Self::Stderr(writer) => writer.write_all(bytes).and_then(|()| writer.flush()),
        }
    }
}

fn remember_first(first: &mut Option<io::Error>, error: io::Error) {
    if first.is_none() {
        *first = Some(error);
    }
}
