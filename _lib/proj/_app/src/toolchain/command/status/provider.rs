use serde::Deserialize;

use super::CommandContext;
use super::filesystem::{
    MAX_STATE_BYTES, directory_chain, is_lower_hex, read_json, regular_file_length,
};

const STATE_SCHEMA: &str = "swawkit.command-provider-state/v1";
const PRODUCER_CONTRACT: &str = "swawkit.proj.dev-setup/v2";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderState {
    schema: String,
    status: String,
    input_revision: String,
    token: String,
    producer_contract: Option<String>,
}

pub(super) fn publication_token(context: &CommandContext) -> Result<String, String> {
    let setup_root = directory_chain(
        &context.data_root,
        &["modules", "kernel", ".dev", "setup"],
        "development environment provider",
    )
    .map_err(|_| unavailable(context))?;
    if setup_root != context.setup_root {
        return Err("development environment provider path invariant failed".to_owned());
    }
    let state: ProviderState = read_json(
        &setup_root.join("_state.json"),
        "development environment provider state",
        MAX_STATE_BYTES,
    )
    .map_err(|_| unavailable(context))?;
    if state.schema != STATE_SCHEMA
        || state.status != "ready"
        || state.input_revision != context.environment_input_revision
        || !is_lower_hex(&state.token, 32)
        || state.producer_contract.as_deref() != Some(PRODUCER_CONTRACT)
    {
        return Err(unavailable(context));
    }

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
    Ok(state.token)
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
