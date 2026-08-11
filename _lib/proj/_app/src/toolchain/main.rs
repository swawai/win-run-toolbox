#![cfg_attr(not(test), windows_subsystem = "console")]

#[cfg(not(windows))]
compile_error!("The Swaw Kit Proj Toolchain V1 supports Windows only.");

mod archive;
mod args;
mod command;
mod download;
mod event;
mod path;

use std::process::ExitCode;

use args::{Operation, parse};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("swawkit-proj-toolchain: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    match parse(std::env::args_os().skip(1))? {
        Operation::Command { handler, arguments } => command::run(&handler, &arguments),
        Operation::Download {
            controlled_root,
            source,
            destination,
            progress_id,
        } => download::run(&controlled_root, &source, &destination, &progress_id),
        Operation::ZipTest { archive } => archive::test(&archive),
        Operation::ZipExtract {
            controlled_root,
            archive,
            destination,
        } => archive::extract(&controlled_root, &archive, &destination),
    }
}
