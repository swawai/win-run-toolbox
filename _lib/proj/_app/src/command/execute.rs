use std::ffi::OsString;
use std::path::Path;

use crate::catalog::CatalogSnapshot;

use super::{
    CommandExecutionContext, CommandResult, ExecutionPhase, GuardPlan, Invocation,
    ProcessEnvironment,
    process::{run_process, validate_adapter},
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
        validate_adapter(invocation.command.adapter)?;
        GuardPlan::discover(kernel_root, &invocation.command)?;
        Ok(())
    }

    pub fn execute(&self, argv: &[OsString]) -> CommandResult<i32> {
        let invocation = Invocation::resolve(self.catalog, argv)?;
        validate_adapter(invocation.command.adapter)?;
        let guard_plan = GuardPlan::discover(&self.context.kernel_root, &invocation.command)?;

        for guard in guard_plan.guards {
            let environment = ProcessEnvironment::for_command(
                self.context,
                &invocation.command,
                ExecutionPhase::Guard(guard.scope),
            )?;
            let exit_code = run_process(
                guard.adapter,
                &guard.entry_path,
                &[],
                &self.context.target_project_root,
                &environment,
                self.context.process_mode,
            )?;
            if exit_code != 0 {
                return Ok(exit_code);
            }
        }

        let environment = ProcessEnvironment::for_command(
            self.context,
            &invocation.command,
            ExecutionPhase::Run,
        )?;
        run_process(
            invocation.command.adapter,
            &invocation.command.entry_path,
            &invocation.arguments,
            &self.context.target_project_root,
            &environment,
            self.context.process_mode,
        )
    }
}
