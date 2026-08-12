use swawkit_proj::development::setup::provider::read_ready;

use super::CommandContext;
use super::filesystem::{directory_chain, regular_file_length};

pub(super) fn publication_token(context: &CommandContext) -> Result<String, String> {
    let state = read_ready(&context.data_root, &context.environment_input_revision)
        .map_err(|_| unavailable(context))?;

    let export_root = directory_chain(
        &context.data_root,
        &["modules", "kernel", ".dev", "setup", "export"],
        "development environment export",
    )
    .map_err(|_| unavailable(context))?;
    if export_root != context.export_root {
        return Err("development environment export path invariant failed".to_owned());
    }
    for name in ["env.cmd", "env.ps1"] {
        regular_file_length(&export_root.join(name), "development environment export")
            .map_err(|_| incomplete(context))?;
    }
    Ok(state.token().to_owned())
}

fn unavailable(context: &CommandContext) -> String {
    format!(
        "Required export from '.dev.setup' is unavailable or outdated. Run '{}'.",
        context.repair_invocation()
    )
}

fn incomplete(context: &CommandContext) -> String {
    format!(
        "The development environment export is incomplete. Run '{}'.",
        context.repair_invocation()
    )
}
