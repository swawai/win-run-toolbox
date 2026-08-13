use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::windows::fs::MetadataExt;

use swawkit_proj::development::archive_tool::install::InstallOutcome;
use swawkit_proj::development::msvc::MsvcInstallOutcome;
use swawkit_proj::development::rust::RustInstallOutcome;
use swawkit_proj::development::setup::declaration::snapshot_from_environment;
use swawkit_proj::development::setup::native::{NativeSetupContext, NativeSetupResult, run_native};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::{CommandContext, context::SetupCommandContext};
use crate::event;

pub(super) fn run(context: &CommandContext, arguments: &[OsString]) -> Result<(), String> {
    if !arguments.is_empty() {
        return Err(".dev.setup does not accept dynamic arguments".to_owned());
    }
    let setup_context = SetupCommandContext::from_environment()?;
    let setup = NativeSetupContext::new(
        &context.data_root,
        &setup_context.cache_data_root,
        &setup_context.profile_revision,
        &context.environment_input_revision,
    )?;
    let declarations = snapshot_from_environment();
    let mut progress = Progress::default();
    let result = run_native(&setup, &declarations, &mut |tool, current, total| {
        progress.update(tool, current, total);
    });
    let result = match result {
        Ok(result) => {
            progress.complete();
            result
        }
        Err(error) => {
            progress.fail();
            return Err(error);
        }
    };
    render(&result, context);
    remove_legacy_state(context);
    Ok(())
}

#[derive(Default)]
struct Progress {
    latest: BTreeMap<String, (String, u64, Option<u64>)>,
}

impl Progress {
    fn update(&mut self, tool: &str, current: u64, total: Option<u64>) {
        let id = progress_id(tool);
        self.latest
            .insert(id.clone(), (tool.to_owned(), current, total));
        event::progress(
            &id,
            "running",
            Some(current),
            total,
            &format!("Downloading {tool}"),
        );
    }

    fn complete(&self) {
        for (id, (tool, current, total)) in &self.latest {
            event::progress(
                id,
                "completed",
                Some(*current),
                total.or(Some(*current)),
                &format!("Downloaded {tool}"),
            );
        }
    }

    fn fail(&self) {
        for (id, (tool, _, _)) in &self.latest {
            event::progress(
                id,
                "failed",
                None,
                None,
                &format!("Download failed: {tool}"),
            );
        }
    }
}

fn progress_id(tool: &str) -> String {
    use sha2::{Digest, Sha256};

    let id = format!("setup:{tool}");
    if id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    {
        id
    } else {
        format!("setup:artifact:{:x}", Sha256::digest(tool.as_bytes()))
    }
}

fn render(result: &NativeSetupResult, context: &CommandContext) {
    for tool in result.archive_tools() {
        let version = if tool.requested() == "latest" {
            format!("latest -> {}", tool.version())
        } else {
            tool.version().to_owned()
        };
        let state = if tool.outcome() == InstallOutcome::Installed {
            "installed and configured"
        } else {
            "is ready"
        };
        println!("[OK] {} {version} {state}.", display_name(tool.name()));
        render_warnings(tool.warnings());
        if let Some(warning) = tool.trust().warning() {
            eprintln!("WARNING: {warning}");
        }
    }
    if let Some(pwsh) = result.system_pwsh() {
        println!(
            "[OK] PowerShell {} system runtime is ready: {}",
            pwsh.version(),
            pwsh.executable().display()
        );
    }
    if let Some(msvc) = result.msvc() {
        let state = if msvc.outcome() == MsvcInstallOutcome::Installed {
            "installed and configured"
        } else {
            "is ready"
        };
        println!("[OK] MSVC channel {} {state}.", msvc.channel());
        render_warnings(msvc.warnings());
    }
    if let Some(rust) = result.rust() {
        let state = if rust.outcome() == RustInstallOutcome::Installed {
            "installed and configured"
        } else {
            "is ready"
        };
        println!("[OK] Rust {} {state}.", rust.toolchain());
        render_warnings(rust.warnings());
    }
    if result.archive_tools().is_empty()
        && result.system_pwsh().is_none()
        && result.msvc().is_none()
        && result.rust().is_none()
    {
        println!("[OK] The base development environment is ready.");
    }
    if result.environment_changed() {
        println!("[ENV] {}", context.export_root.join("env.cmd").display());
        println!("[ENV] {}", context.export_root.join("env.ps1").display());
    }
}

fn display_name(name: &str) -> &str {
    match name {
        "bun" => "Bun",
        "pwsh" => "PowerShell",
        _ => name,
    }
}

fn render_warnings(warnings: &[String]) {
    for warning in warnings {
        eprintln!("WARNING: {warning}");
    }
}

fn remove_legacy_state(context: &CommandContext) {
    let path = context.export_root.join("_state.json");
    if let Err(error) = validate_export_root(context) {
        eprintln!("WARNING: The ignored legacy export state could not be removed: {error}");
        return;
    }
    let result = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => Err(format!("cannot inspect '{}': {error}", path.display())),
        Ok(metadata)
            if metadata.is_file()
                && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 =>
        {
            fs::remove_file(&path).map_err(|error| {
                format!(
                    "cannot remove legacy setup state '{}': {error}",
                    path.display()
                )
            })
        }
        Ok(_) => Err(format!(
            "legacy setup state is not a regular file: {}",
            path.display()
        )),
    };
    if let Err(error) = result {
        eprintln!("WARNING: The ignored legacy export state could not be removed: {error}");
    }
}

fn validate_export_root(context: &CommandContext) -> Result<(), String> {
    let mut path = context.data_root.clone();
    for segment in ["modules", "kernel", ".dev", "setup", "export"] {
        path.push(segment);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect '{}': {error}", path.display()))?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(format!(
                "development export must be a regular directory: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::progress_id;

    #[test]
    fn progress_ids_are_protocol_safe_for_external_artifact_names() {
        assert_eq!(progress_id("bun"), "setup:bun");
        let id = progress_id(&format!("msvc:{}", "unsafe payload ".repeat(20)));
        assert!(id.starts_with("setup:artifact:"));
        assert!(id.len() <= 128);
        assert!(
            id.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
        );
    }
}
