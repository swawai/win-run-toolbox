use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, Stdio};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::catalog::CommandAdapter;
use crate::command::console_cancel;
use crate::command_event::{
    CapturedCommandEvent, CommandEventFrameDecoder, CommandProgress, CommandProgressState,
    CommandProgressUnit,
};
use crate::run_journal::{
    RunJournal, RunJournalEvent, RunJournalEventData, RunJournalPhase, RunJournalStream,
};
use crate::utf8_output::Utf8LossyDecoder;

use super::{AdapterLaunch, prepare_command};
use crate::command::{CommandError, CommandProcessMode, CommandResult, ProcessEnvironment};

const OUTPUT_READ_BUFFER_SIZE: usize = 8192;

pub(crate) fn run_process_journaled(
    adapter: CommandAdapter,
    entry_path: &Path,
    arguments: &[OsString],
    working_directory: &Path,
    adapter_launch: &AdapterLaunch,
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
        adapter_launch,
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

    let wait_result = wait_for_child_or_cancellation(child);
    let stdout = join_reader(stdout_thread, "stdout");
    let stderr = join_reader(stderr_thread, "stderr");
    let status = wait_result?;
    stdout?;
    stderr?;
    Ok(status.code().unwrap_or(1))
}

fn wait_for_child_or_cancellation(child: &mut Child) -> CommandResult<std::process::ExitStatus> {
    loop {
        if console_cancel::requested() {
            if let Err(error) = child.kill() {
                match child.try_wait() {
                    Ok(Some(status)) => return Ok(status),
                    _ => {
                        console_cancel::mark_termination_failed();
                        return Err(CommandError::new(format!(
                            "cannot terminate canceled command entry: {error}"
                        )));
                    }
                }
            }
            return child.wait().map_err(|error| {
                console_cancel::mark_termination_failed();
                CommandError::new(format!("cannot wait for canceled command entry: {error}"))
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                return Err(CommandError::new(format!(
                    "cannot wait for command entry: {error}"
                )));
            }
        }
    }
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
    let mut frame_decoder = CommandEventFrameDecoder::default();
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
            if let Some(text) = decoder.decode(&[], true) {
                decode_events(
                    &mut console,
                    &journal,
                    phase,
                    stream,
                    text,
                    &mut frame_decoder,
                    &mut journal_available,
                    &mut first_error,
                );
            }
            for event in frame_decoder.finish() {
                render_event(
                    &mut console,
                    &journal,
                    phase,
                    stream,
                    event,
                    &mut journal_available,
                    &mut first_error,
                );
            }
            break;
        }
        if let Some(text) = decoder.decode(&buffer[..count], false) {
            decode_events(
                &mut console,
                &journal,
                phase,
                stream,
                text,
                &mut frame_decoder,
                &mut journal_available,
                &mut first_error,
            );
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn decode_events(
    console: &mut ConsoleWriter,
    journal: &RunJournal,
    phase: RunJournalPhase,
    stream: RunJournalStream,
    text: String,
    decoder: &mut CommandEventFrameDecoder,
    journal_available: &mut bool,
    first_error: &mut Option<io::Error>,
) {
    for event in decoder.push(&text) {
        render_event(
            console,
            journal,
            phase,
            stream,
            event,
            journal_available,
            first_error,
        );
    }
}

fn render_event(
    console: &mut ConsoleWriter,
    journal: &RunJournal,
    phase: RunJournalPhase,
    stream: RunJournalStream,
    captured: CapturedCommandEvent,
    journal_available: &mut bool,
    first_error: &mut Option<io::Error>,
) {
    match captured {
        CapturedCommandEvent::Output(text) => {
            if *journal_available {
                match journal.output(phase, stream, text.clone()) {
                    Ok(Some(event)) => {
                        render_console(console, &event, first_error);
                        return;
                    }
                    Ok(None) => return,
                    Err(error) => {
                        *journal_available = false;
                        remember_first(first_error, error);
                    }
                }
            }
            if let Err(error) = console.write_text(&text) {
                remember_first(first_error, error);
            }
        }
        CapturedCommandEvent::Progress(progress) => {
            if *journal_available {
                match journal.progress(phase, progress.clone()) {
                    Ok(event) => {
                        render_console(console, &event, first_error);
                        return;
                    }
                    Err(error) => {
                        *journal_available = false;
                        remember_first(first_error, error);
                    }
                }
            }
            if let Err(error) = console.write_progress(&progress) {
                remember_first(first_error, error);
            }
        }
    }
}

fn render_console(
    console: &mut ConsoleWriter,
    event: &RunJournalEvent,
    first_error: &mut Option<io::Error>,
) {
    if let Err(error) = console.render(event) {
        remember_first(first_error, error);
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

    fn render(&mut self, event: &RunJournalEvent) -> io::Result<()> {
        match &event.data {
            RunJournalEventData::Output { text, .. } => self.write_text(text),
            RunJournalEventData::Progress { progress } => self.write_progress(progress),
        }
    }

    fn write_progress(&mut self, progress: &CommandProgress) -> io::Result<()> {
        let status = match progress.state {
            CommandProgressState::Running => "PROGRESS",
            CommandProgressState::Completed => "OK",
            CommandProgressState::Failed => "ERROR",
        };
        let unit = match progress.unit {
            CommandProgressUnit::Bytes => "bytes",
            CommandProgressUnit::Items => "items",
            CommandProgressUnit::Percent => "%",
        };
        let amount = match (progress.current, progress.total) {
            (Some(current), Some(total)) => format!(" ({current}/{total} {unit})"),
            (Some(current), None) => format!(" ({current} {unit})"),
            _ => String::new(),
        };
        self.write_text(&format!("[{status}] {}{amount}\n", progress.message))
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        let bytes = text.as_bytes();
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
