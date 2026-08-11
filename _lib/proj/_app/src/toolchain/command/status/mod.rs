use std::ffi::OsString;

use super::context::CommandContext;

mod archive_tool;
mod filesystem;
mod msvc;
mod provider;
mod rust;

pub(super) fn run(context: &CommandContext, arguments: &[OsString]) -> Result<(), String> {
    if !arguments.is_empty() {
        return Err(".dev.status does not accept dynamic arguments".to_owned());
    }

    match provider::publication_token(context) {
        Ok(token) => println!("[READY] .dev.setup publication {}", &token[..8]),
        Err(error) => println!("[OUTDATED] {error}"),
    }
    let bun = archive_tool::inspect(context, &swawkit_proj::development::BUN)?;
    bun.render(&swawkit_proj::development::BUN);
    let pwsh = archive_tool::inspect(context, &swawkit_proj::development::PWSH)?;
    pwsh.render(&swawkit_proj::development::PWSH);
    msvc::inspect(context)?.render();
    rust::inspect(context)?.render();
    Ok(())
}
