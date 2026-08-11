mod context;
mod status;

use std::ffi::OsString;

use context::CommandContext;

pub(crate) fn run(handler: &str, arguments: &[OsString]) -> Result<(), String> {
    let context = CommandContext::from_environment(handler)?;
    match handler {
        "dev.status" => status::run(&context, arguments),
        _ => Err(format!("unsupported Toolchain command handler '{handler}'")),
    }
}
