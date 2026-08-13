use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::development::is_semantic_version;
use crate::development::process_probe;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemPwsh {
    executable: PathBuf,
    version: String,
}

impl SystemPwsh {
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn version(&self) -> &str {
        &self.version
    }
}

pub fn resolve_system() -> Result<SystemPwsh, String> {
    let mut found_path = false;
    for (_, path) in env::vars_os().filter(|(name, _)| {
        name.to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case("PATH"))
    }) {
        found_path = true;
        for directory in env::split_paths(&path).filter(|path| path.is_absolute()) {
            let candidate = directory.join("pwsh.exe");
            match inspect_candidate(&candidate) {
                Ok(Some(pwsh)) => return Ok(pwsh),
                Ok(None) => {}
                Err(error) => return Err(error),
            }
        }
    }
    if !found_path {
        return Err("system PowerShell 7 is unavailable because PATH is not defined".to_owned());
    }
    Err(format!(
        "system PowerShell 7 (pwsh.exe) was not found on PATH. Install PowerShell 7, restart the Entry Host, and run .dev.setup again"
    ))
}

pub fn inspect(executable: &Path) -> Result<SystemPwsh, String> {
    inspect_candidate(executable)?.ok_or_else(|| {
        format!(
            "system PowerShell 7 executable does not exist: {}",
            executable.display()
        )
    })
}

fn inspect_candidate(candidate: &Path) -> Result<Option<SystemPwsh>, String> {
    let metadata = match fs::metadata(candidate) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot inspect system PowerShell candidate '{}': {error}",
                candidate.display()
            ));
        }
    };
    if !metadata.is_file() {
        return Err(format!(
            "system PowerShell candidate is not a file: {}",
            candidate.display()
        ));
    }
    let executable = fs::canonicalize(candidate).map_err(|error| {
        format!(
            "cannot resolve system PowerShell candidate '{}': {error}",
            candidate.display()
        )
    })?;
    let mut command = Command::new(&executable);
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "[Console]::Out.Write($PSVersionTable.PSEdition.ToString() + '|' + $PSVersionTable.PSVersion.ToString())",
    ]);
    let output = process_probe::run(command, "system PowerShell version probe")?;
    if output.exit_code != 0 {
        return Err(format!(
            "system PowerShell version probe failed with exit code {}: {}",
            output.exit_code, output.stderr
        ));
    }
    let (edition, version) = output.stdout.split_once('|').ok_or_else(|| {
        format!(
            "system PowerShell returned an invalid version identity: {}",
            output.stdout
        )
    })?;
    validate_identity(&executable, edition, version)?;
    Ok(Some(SystemPwsh {
        executable,
        version: version.to_owned(),
    }))
}

fn validate_identity(executable: &Path, edition: &str, version: &str) -> Result<(), String> {
    let major = version
        .split('.')
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    if edition != "Core" || major < 7 || !is_semantic_version(version) {
        return Err(format!(
            "system PowerShell must be PowerShell 7 or newer; '{}' reported {edition} {version}",
            executable.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_powershell_core_seven_or_newer() {
        let path = Path::new(r"C:\Tools\pwsh.exe");
        validate_identity(path, "Core", "7.6.4").unwrap();

        assert!(validate_identity(path, "Desktop", "7.6.4").is_err());
        assert!(validate_identity(path, "Core", "6.2.7").is_err());
        assert!(validate_identity(path, "Core", "latest").is_err());
    }

    #[test]
    fn rejects_windows_powershell_five() {
        let system_root = env::var_os("SystemRoot").expect("SystemRoot");
        let executable =
            PathBuf::from(system_root).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let error = inspect(&executable).unwrap_err();

        assert!(error.contains("PowerShell 7 or newer"), "{error}");
    }
}
