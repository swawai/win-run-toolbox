use swawkit_proj::development::msvc::{MsvcDefinition, MsvcStore};

use super::CommandContext;

pub(super) enum MsvcReport {
    Off,
    Managed {
        channel: String,
        versions: Option<(String, String)>,
    },
}

impl MsvcReport {
    pub(super) fn render(&self) {
        match self {
            Self::Off => println!("[OFF] msvc is disabled."),
            Self::Managed { channel, versions } => {
                let (state, version) = match versions {
                    Some((tool, sdk)) => ("READY", format!("tool {tool}, SDK {sdk}")),
                    None => ("MISSING", "not installed".to_owned()),
                };
                println!("[{state}] msvc channel {channel}  microsoft-manifest  {version}");
            }
        }
    }
}

pub(super) fn inspect(context: &CommandContext) -> Result<MsvcReport, String> {
    let mode = context
        .environment("SWAWKIT_PROJ_MSVC_MODE")
        .to_ascii_lowercase();
    if mode.is_empty() || mode == "disabled" {
        return Ok(MsvcReport::Off);
    }
    if mode != "managed" {
        return Err(format!(
            "Unsupported SWAWKIT_PROJ_MSVC_MODE value '{mode}'. Expected 'managed' or 'disabled'."
        ));
    }
    let definition = MsvcDefinition::new(&context.environment("SWAWKIT_PROJ_MSVC_CHANNEL"))
        .map_err(|error| error.to_string())?;
    let store = MsvcStore::new(&context.data_root, &definition);
    let versions = store.read_installation().ok().map(|installation| {
        (
            installation.tool_version().to_owned(),
            installation.sdk_version().to_owned(),
        )
    });
    Ok(MsvcReport::Managed {
        channel: definition.channel().to_owned(),
        versions,
    })
}
