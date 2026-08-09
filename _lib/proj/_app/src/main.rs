#[cfg(not(target_os = "windows"))]
compile_error!("The Swaw Kit Proj application V0 supports Windows only.");

mod cli;
mod host_instance;
mod tray;

use std::error::Error;
use swawkit_proj::{
    command::CommandProcessMode,
    context::EntryContext,
    data_root::{DataRootSession, ResolveDataRootRequest},
    launch::{LaunchMode, LaunchRequest, clear_inherited_swawkit_environment},
};

use crate::host_instance::{HostInstance, HostInstanceAcquisition};

fn main() {
    match run() {
        Ok(0) => {}
        Ok(exit_code) => std::process::exit(exit_code),
        Err(error) => {
            eprintln!("[ERROR] {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<i32, Box<dyn Error>> {
    let request = LaunchRequest::from_process()?;
    // SAFETY: `run` is the process composition root and no application thread
    // has been spawned yet. All launch facts needed below are owned by request.
    unsafe { clear_inherited_swawkit_environment() };
    let context = EntryContext::from_launch(&request)?;

    match request.mode {
        LaunchMode::Cli => cli::run(&context, &request.argv, CommandProcessMode::InheritConsole)
            .map_err(Into::into),
        LaunchMode::Worker => {
            cli::run(&context, &request.argv, CommandProcessMode::NoWindow).map_err(Into::into)
        }
        LaunchMode::InternalHost => {
            let data_root = DataRootSession::new(ResolveDataRootRequest {
                swawkit_home: &context.swawkit_home,
                entry_file: &context.entry_file,
            })?;
            let instance = match HostInstance::acquire(data_root.entry_identity())? {
                HostInstanceAcquisition::Primary(instance) => instance,
                HostInstanceAcquisition::ActivatedExisting => return Ok(0),
            };
            tray::run(context, data_root, instance)?;
            Ok(0)
        }
    }
}
