#[cfg(not(target_os = "windows"))]
compile_error!("The Swaw Kit Proj application V0 supports Windows only.");

mod cli;
mod host_instance;
mod tray;

use std::error::Error;
use std::{env, path::PathBuf};

use swawkit_proj::{
    context::EntryContext,
    data_root::{DataRootSession, ResolveDataRootRequest},
    launch::{LaunchMode, LaunchRequest},
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
    let context = EntryContext::from_launch(&request)?;

    match request.mode {
        LaunchMode::Cli => cli::run(&context, &request.argv).map_err(Into::into),
        LaunchMode::InternalHost => {
            let inherited_data_root = env::var_os("SWAWKIT_PROJ_DATA_ROOT")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from);
            let legacy_data_directory = env::var_os("SWAWKIT_PROJ_TARGET_PROJECT_ROOT")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .map(|path| path.join("data"));
            let data_root = DataRootSession::new(ResolveDataRootRequest {
                swawkit_home: &context.swawkit_home,
                entry_file: &context.entry_file,
                inherited_data_root: inherited_data_root.as_deref(),
                legacy_data_directory: legacy_data_directory.as_deref(),
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
