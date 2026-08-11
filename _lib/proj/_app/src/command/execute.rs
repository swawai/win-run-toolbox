use std::ffi::OsString;
use std::path::Path;

use crate::catalog::{CatalogSnapshot, CommandAdapter, CommandSource};
use crate::run_journal::{RunJournal, RunJournalPhase, RunJournalSource, StartRunJournal};

use super::{
    CommandError, CommandExecutionContext, CommandResult, ExecutionPhase, GuardPlan, Invocation,
    ProcessEnvironment, ResolvedCommand, command_data_root,
    process::{AdapterLaunch, run_process, run_process_journaled, validate_adapter},
    resolve_entry_bun,
};

pub struct CommandExecutor<'a> {
    context: &'a CommandExecutionContext,
    catalog: &'a CatalogSnapshot,
}

impl<'a> CommandExecutor<'a> {
    pub fn new(context: &'a CommandExecutionContext, catalog: &'a CatalogSnapshot) -> Self {
        Self { context, catalog }
    }

    pub fn preflight(
        kernel_root: &Path,
        catalog: &CatalogSnapshot,
        argv: &[OsString],
    ) -> CommandResult<()> {
        let invocation = Invocation::resolve(catalog, argv)?;
        validate_command_adapter(&invocation.command)?;
        GuardPlan::discover(kernel_root, &invocation.command)?;
        Ok(())
    }

    pub fn execute(&self, argv: &[OsString]) -> CommandResult<i32> {
        let invocation = Invocation::resolve(self.catalog, argv)?;
        self.execute_invocation(&invocation, None)
    }

    pub fn execute_journaled(&self, argv: &[OsString]) -> CommandResult<i32> {
        let invocation = Invocation::resolve(self.catalog, argv)?;
        let journal = RunJournal::start(StartRunJournal {
            module_data_root: command_data_root(self.context, &invocation.command)?,
            address: invocation.command.address.clone(),
            source: RunJournalSource::Cli,
            argument_count: invocation.arguments.len(),
            profile_revision: self.context.profile_revision.clone(),
        })
        .map_err(|error| CommandError::new(format!("cannot start command journal: {error}")))?;
        match self.execute_invocation(&invocation, Some(&journal)) {
            Ok(exit_code) => journal
                .finish_exited(exit_code)
                .map(|()| exit_code)
                .map_err(|error| {
                    CommandError::new(format!("cannot complete command journal: {error}"))
                }),
            Err(error) => {
                let journal_result = journal.finish_failed(error.to_string());
                match journal_result {
                    Ok(()) => Err(error),
                    Err(journal_error) => Err(CommandError::new(format!(
                        "{error}; additionally, command journal completion failed: {journal_error}"
                    ))),
                }
            }
        }
    }

    fn execute_invocation(
        &self,
        invocation: &Invocation,
        journal: Option<&RunJournal>,
    ) -> CommandResult<i32> {
        validate_command_adapter(&invocation.command)?;
        let adapter_launch = match invocation.command.adapter {
            CommandAdapter::Bun => AdapterLaunch::Bun(resolve_entry_bun(self.context)?),
            CommandAdapter::Toolchain => AdapterLaunch::Toolchain {
                executable: self.context.toolchain_executable.clone(),
                handler: invocation.command.handler.clone().ok_or_else(|| {
                    CommandError::new("Catalog invariant failed: Toolchain command has no handler")
                })?,
            },
            _ => AdapterLaunch::Direct,
        };
        let guard_plan = GuardPlan::discover(&self.context.kernel_root, &invocation.command)?;

        for guard in guard_plan.guards {
            let environment = ProcessEnvironment::for_command(
                self.context,
                &invocation.command,
                ExecutionPhase::Guard(guard.scope),
            )?;
            let phase = match guard.scope {
                super::GuardScope::Global => RunJournalPhase::GuardGlobal,
                super::GuardScope::Command => RunJournalPhase::GuardCommand,
            };
            let exit_code = run(
                guard.adapter,
                &guard.entry_path,
                &[],
                &self.context.target_project_root,
                &AdapterLaunch::Direct,
                &environment,
                self.context.process_mode,
                journal,
                phase,
            )?;
            if exit_code != 0 {
                return Ok(exit_code);
            }
        }

        let mut environment = ProcessEnvironment::for_command(
            self.context,
            &invocation.command,
            ExecutionPhase::Run,
        )?;
        if let AdapterLaunch::Bun(executable) = &adapter_launch {
            let directory = executable.parent().ok_or_else(|| {
                CommandError::new("the resolved Entry Bun path has no parent directory")
            })?;
            environment.prepend_path(directory)?;
        }
        run(
            invocation.command.adapter,
            &invocation.command.entry_path,
            &invocation.arguments,
            &self.context.target_project_root,
            &adapter_launch,
            &environment,
            self.context.process_mode,
            journal,
            RunJournalPhase::Run,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    adapter: crate::catalog::CommandAdapter,
    entry_path: &Path,
    arguments: &[OsString],
    working_directory: &Path,
    adapter_launch: &AdapterLaunch,
    environment: &ProcessEnvironment,
    process_mode: super::CommandProcessMode,
    journal: Option<&RunJournal>,
    phase: RunJournalPhase,
) -> CommandResult<i32> {
    match journal {
        Some(journal) => run_process_journaled(
            adapter,
            entry_path,
            arguments,
            working_directory,
            adapter_launch,
            environment,
            process_mode,
            journal,
            phase,
        ),
        None => run_process(
            adapter,
            entry_path,
            arguments,
            working_directory,
            adapter_launch,
            environment,
            process_mode,
        ),
    }
}

fn validate_command_adapter(command: &ResolvedCommand) -> CommandResult<()> {
    validate_adapter(command.adapter)?;
    if command.adapter == CommandAdapter::Bun && command.source != CommandSource::Action {
        return Err(CommandError::new(format!(
            "the run.ts adapter is only supported for Action commands; '{}' is product-owned \
             and must use a Rust-native entry",
            command.address
        )));
    }
    if command.adapter == CommandAdapter::Toolchain && command.source != CommandSource::Kernel {
        return Err(CommandError::new(format!(
            "the run.toolchain.json adapter is only supported for Kernel commands; '{}' has an invalid owner",
            command.address
        )));
    }
    Ok(())
}
