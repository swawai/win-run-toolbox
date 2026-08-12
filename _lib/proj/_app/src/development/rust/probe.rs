use std::path::Path;
use std::process::Command;

use super::{RustDefinition, RustError, RustErrorKind, error};
use crate::development::archive_tool::install::run_bounded_process;

pub(super) struct RustProbe {
    pub rustup_version: String,
    pub rustc_version: String,
    pub rustc_commit: String,
    pub cargo_version: String,
    pub rustfmt_version: String,
}

pub(super) fn inspect(definition: &RustDefinition, root: &Path) -> Result<RustProbe, RustError> {
    let rustup = root.join("cargo/bin/rustup.exe");
    for relative in ["rustup.exe", "rustc.exe", "cargo.exe"] {
        if !root.join("cargo/bin").join(relative).is_file() {
            return Err(probe_error(format!(
                "Rust installation is missing a proxy: {relative}"
            )));
        }
    }
    let rustup_output = run(&rustup, root, root, &["--version"])?;
    let rustc_output = run(
        &rustup,
        root,
        root,
        &["run", definition.toolchain_name(), "rustc", "-Vv"],
    )?;
    let cargo_output = run(
        &rustup,
        root,
        root,
        &["run", definition.toolchain_name(), "cargo", "--version"],
    )?;
    let rustfmt_output = run(
        &rustup,
        root,
        root,
        &["run", definition.toolchain_name(), "rustfmt", "--version"],
    )?;
    let host = line_value(&rustc_output, "host: ").unwrap_or_default();
    if host != definition.host() {
        return Err(probe_error(
            "The installed Rust toolchain reported invalid identity data.",
        ));
    }
    Ok(RustProbe {
        rustup_version: command_version(&rustup_output, "rustup ")?,
        rustc_version: line_value(&rustc_output, "release: ").ok_or_else(|| {
            probe_error("The installed Rust toolchain did not report its release.")
        })?,
        rustc_commit: line_value(&rustc_output, "commit-hash: ")
            .filter(|value| {
                value.len() == 40
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            .ok_or_else(|| {
                probe_error("The installed Rust toolchain did not report its commit.")
            })?,
        cargo_version: command_version(&cargo_output, "cargo ")?,
        rustfmt_version: command_version(&rustfmt_output, "rustfmt ")?,
    })
}

fn run(
    executable: &Path,
    working_directory: &Path,
    install_root: &Path,
    arguments: &[&str],
) -> Result<String, RustError> {
    let mut command = Command::new(executable);
    command.args(arguments).current_dir(working_directory);
    command.env("CARGO_HOME", install_root.join("cargo"));
    command.env("RUSTUP_HOME", install_root.join("rustup"));
    for name in [
        "RUSTUP_TOOLCHAIN",
        "RUSTUP_TOOLCHAIN_SOURCE",
        "RUSTUP_DIST_SERVER",
        "RUSTUP_DIST_ROOT",
        "RUSTUP_UPDATE_ROOT",
        "RUSTUP_VERSION",
    ] {
        command.env_remove(name);
    }
    let output = run_bounded_process(command)?;
    if output.exit_code != 0 {
        return Err(probe_error(format!(
            "Rust installation probe failed: {}",
            output.stderr
        )));
    }
    Ok(output.stdout)
}

fn command_version(output: &str, prefix: &str) -> Result<String, RustError> {
    let value = output
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .and_then(|value| value.split_whitespace().next())
        .filter(|value| version_prefix(value))
        .ok_or_else(|| {
            probe_error("The installed Rust toolchain reported invalid version data.")
        })?;
    Ok(value.to_owned())
}

fn line_value(output: &str, prefix: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn version_prefix(value: &str) -> bool {
    let prefix = value
        .split(|character: char| !character.is_ascii_digit() && character != '.')
        .next()
        .unwrap_or("");
    let fields = prefix.trim_end_matches('.').split('.').collect::<Vec<_>>();
    fields.len() >= 3
        && fields
            .iter()
            .all(|field| !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()))
}

fn probe_error(message: impl Into<String>) -> RustError {
    error(RustErrorKind::InstallationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_parsers_accept_the_published_shapes() {
        assert_eq!(
            command_version("rustup 1.29.0 (fixture)", "rustup ").unwrap(),
            "1.29.0"
        );
        assert_eq!(
            line_value("release: 1.97.1\ncommit-hash: abc", "release: ").unwrap(),
            "1.97.1"
        );
        assert!(!version_prefix("1.2"));
    }
}
