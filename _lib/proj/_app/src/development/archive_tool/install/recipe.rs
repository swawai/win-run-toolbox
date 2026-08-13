use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::development::process_probe;
use crate::development::{ArchiveToolContract, BUN, PWSH};

use super::super::{ArchiveToolError, ArchiveToolErrorKind, ResolvedDefinition};

const BUNX_CONTENT: &[u8] = b"@echo off\r\n\"%~dp0bun.exe\" x %*\r\n";

pub(super) trait Recipe {
    fn prepare(
        &self,
        tool: &ArchiveToolContract,
        staged_root: &Path,
    ) -> Result<(), ArchiveToolError>;

    fn validate(
        &self,
        tool: &ArchiveToolContract,
        resolved: &ResolvedDefinition,
        staged_root: &Path,
    ) -> Result<(), ArchiveToolError>;
}

pub(super) struct NativeRecipe;

impl Recipe for NativeRecipe {
    fn prepare(
        &self,
        tool: &ArchiveToolContract,
        staged_root: &Path,
    ) -> Result<(), ArchiveToolError> {
        match tool.name {
            name if name == BUN.name => write_bunx(staged_root),
            name if name == PWSH.name => Ok(()),
            name => Err(unsupported(name)),
        }
    }

    fn validate(
        &self,
        tool: &ArchiveToolContract,
        resolved: &ResolvedDefinition,
        staged_root: &Path,
    ) -> Result<(), ArchiveToolError> {
        let executable = staged_root.join(tool.executable);
        let arguments: &[&str] = match tool.name {
            name if name == BUN.name => &["--version"],
            name if name == PWSH.name => &["-Version"],
            name => return Err(unsupported(name)),
        };
        let mut command = Command::new(&executable);
        command.args(arguments).current_dir(staged_root);
        let output = run_bounded_process(command)?;
        if output.exit_code != 0 {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::ProbeFailed,
                format!(
                    "staged {} version probe failed with exit code {}: {}",
                    tool.display_name, output.exit_code, output.stderr
                ),
            ));
        }
        let actual = if tool.name == BUN.name {
            output.stdout.as_str()
        } else {
            output.stdout.strip_prefix("PowerShell ").unwrap_or("")
        };
        if actual != resolved.version() {
            return Err(ArchiveToolError::new(
                ArchiveToolErrorKind::ProbeFailed,
                format!(
                    "staged {} reports '{}', expected '{}'.",
                    tool.display_name,
                    output.stdout,
                    if tool.name == PWSH.name {
                        format!("PowerShell {}", resolved.version())
                    } else {
                        resolved.version().to_owned()
                    }
                ),
            ));
        }
        Ok(())
    }
}

fn write_bunx(staged_root: &Path) -> Result<(), ArchiveToolError> {
    let path = staged_root.join("bunx.cmd");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|error| install_error("write the Bun shim", &path, error))?;
    file.write_all(BUNX_CONTENT)
        .map_err(|error| install_error("write the Bun shim", &path, error))?;
    file.sync_all()
        .map_err(|error| install_error("flush the Bun shim", &path, error))
}

pub(crate) fn run_bounded_process(
    command: Command,
) -> Result<process_probe::CapturedProcess, ArchiveToolError> {
    process_probe::run(command, "the staged executable probe")
        .map_err(|error| ArchiveToolError::new(ArchiveToolErrorKind::ProbeFailed, error))
}

fn unsupported(name: &str) -> ArchiveToolError {
    ArchiveToolError::new(
        ArchiveToolErrorKind::InvalidInstallRequest,
        format!("unsupported archive tool recipe '{name}'"),
    )
}

fn install_error(action: &str, path: &Path, error: std::io::Error) -> ArchiveToolError {
    ArchiveToolError::new(
        ArchiveToolErrorKind::InstallationFailed,
        format!("cannot {action} '{}': {error}", path.display()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bun_shim_is_byte_compatible_with_the_published_recipe() {
        let root =
            std::env::temp_dir().join(format!("swawkit-archive-recipe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();

        NativeRecipe.prepare(&BUN, &root).unwrap();

        assert_eq!(std::fs::read(root.join("bunx.cmd")).unwrap(), BUNX_CONTENT);
        std::fs::remove_dir_all(root).unwrap();
    }
}
