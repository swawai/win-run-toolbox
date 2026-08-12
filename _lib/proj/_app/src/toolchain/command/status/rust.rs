use super::CommandContext;
use swawkit_proj::development::rust::{RustDefinition, RustStore};

pub(super) enum RustReport {
    Off,
    Rustup {
        toolchain: String,
        versions: Option<(String, String)>,
    },
}

impl RustReport {
    pub(super) fn render(&self) {
        match self {
            Self::Off => println!("[OFF] rust is disabled."),
            Self::Rustup {
                toolchain,
                versions,
            } => {
                let (state, version) = versions.as_ref().map_or_else(
                    || ("MISSING", "not installed".to_owned()),
                    |(rustc, cargo)| ("READY", format!("rustc {rustc}, cargo {cargo}")),
                );
                println!("[{state}] rust {toolchain}  rust-static-sha256  {version}");
            }
        }
    }
}

pub(super) fn inspect(context: &CommandContext) -> Result<RustReport, String> {
    let mode = context
        .environment("SWAWKIT_PROJ_RUST_MODE")
        .to_ascii_lowercase();
    if mode.is_empty() || mode == "disabled" {
        return Ok(RustReport::Off);
    }
    if mode != "rustup" {
        return Err(format!(
            "Unsupported SWAWKIT_PROJ_RUST_MODE value '{mode}'. Expected 'rustup' or 'disabled'."
        ));
    }
    let definition = RustDefinition::new(
        &context.environment("SWAWKIT_PROJ_RUST_TOOLCHAIN"),
        &context.environment("SWAWKIT_PROJ_RUST_PROFILE"),
        &context.environment("SWAWKIT_PROJ_RUST_HOST"),
    )
    .map_err(|error| error.to_string())?;
    let versions = RustStore::new(&context.data_root, &definition)
        .read_installation()
        .ok()
        .map(|installation| {
            (
                installation.rustc_version().to_owned(),
                installation.cargo_version().to_owned(),
            )
        });
    Ok(RustReport::Rustup {
        toolchain: definition.toolchain().to_owned(),
        versions,
    })
}
