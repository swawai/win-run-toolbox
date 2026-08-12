use std::fmt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

mod store;

#[path = "rust/environment.rs"]
mod environment;

pub use store::RustInstallation;

use super::archive_tool::{ArchiveToolError, ArchiveToolErrorKind};

pub const HOST: &str = "x86_64-pc-windows-msvc";
pub const PROFILE: &str = "minimal";
pub const RUSTUP_URL: &str =
    "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe";
pub const RUSTUP_CHECKSUM_URL: &str =
    "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe.sha256";
const RECIPE_VERSION: &str = "2";
const REQUIRED_COMPONENTS: [&str; 1] = ["rustfmt"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustDefinition {
    toolchain: String,
    toolchain_name: String,
}

impl RustDefinition {
    pub fn new(toolchain: &str, profile: &str, host: &str) -> Result<Self, RustError> {
        let toolchain = toolchain.trim().to_ascii_lowercase();
        if !valid_toolchain(&toolchain) {
            return Err(error(
                RustErrorKind::InvalidDefinition,
                "SWAWKIT_PROJ_RUST_TOOLCHAIN must be stable, beta, nightly, a Rust version, or a dated channel.",
            ));
        }
        let profile = profile.trim().to_ascii_lowercase();
        if profile != PROFILE {
            return Err(error(
                RustErrorKind::InvalidDefinition,
                format!("Unsupported Rust profile '{profile}'. Expected one of: {PROFILE}"),
            ));
        }
        let host = host.trim().to_ascii_lowercase();
        if host != HOST {
            return Err(error(
                RustErrorKind::InvalidDefinition,
                format!("Rust V0 supports host '{HOST}' only; received '{host}'."),
            ));
        }
        Ok(Self {
            toolchain_name: format!("{toolchain}-{HOST}"),
            toolchain,
        })
    }

    pub fn toolchain(&self) -> &str {
        &self.toolchain
    }

    pub fn toolchain_name(&self) -> &str {
        &self.toolchain_name
    }

    pub fn profile(&self) -> &'static str {
        PROFILE
    }

    pub fn host(&self) -> &'static str {
        HOST
    }

    pub fn required_components(&self) -> &'static [&'static str] {
        &REQUIRED_COMPONENTS
    }

    pub fn definition_signature(&self) -> String {
        let identity = [
            "swawkit.proj-dev.rust-definition.v0",
            "rustup",
            self.toolchain(),
            PROFILE,
            HOST,
            "rustfmt",
            RECIPE_VERSION,
            RUSTUP_URL,
            RUSTUP_CHECKSUM_URL,
        ]
        .join("\n");
        format!("{:x}", Sha256::digest(identity.as_bytes()))
    }

    pub(crate) fn recipe_version(&self) -> &'static str {
        RECIPE_VERSION
    }

    pub(crate) fn required_paths(&self) -> Vec<String> {
        let root = format!("rustup\\toolchains\\{}", self.toolchain_name);
        [
            "cargo\\bin\\rustup.exe".to_owned(),
            "cargo\\bin\\rustc.exe".to_owned(),
            "cargo\\bin\\cargo.exe".to_owned(),
            "cargo\\bin\\rustfmt.exe".to_owned(),
            "cargo\\bin\\cargo-fmt.exe".to_owned(),
            "rustup\\settings.toml".to_owned(),
            format!("{root}\\bin\\rustc.exe"),
            format!("{root}\\bin\\cargo.exe"),
            format!("{root}\\bin\\rustdoc.exe"),
            format!("{root}\\lib\\rustlib\\manifest-rust-std-{HOST}"),
            format!("{root}\\bin\\rustfmt.exe"),
            format!("{root}\\bin\\cargo-fmt.exe"),
        ]
        .into_iter()
        .collect()
    }
}

pub struct RustStore<'a> {
    data_root: &'a Path,
    definition: &'a RustDefinition,
}

impl<'a> RustStore<'a> {
    pub fn new(data_root: &'a Path, definition: &'a RustDefinition) -> Self {
        Self {
            data_root,
            definition,
        }
    }

    pub fn read_installation(&self) -> Result<RustInstallation, RustError> {
        let root = self.install_root()?;
        store::read_installation_at(self.definition, &root)
    }

    pub fn install_root(&self) -> Result<PathBuf, RustError> {
        Ok(super::archive_tool::filesystem::directory_chain(
            self.data_root,
            &[
                "modules",
                "kernel",
                ".dev",
                "setup",
                "export",
                "rust",
                "installs",
                self.definition.toolchain(),
            ],
            "Rust installation",
        )?)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RustErrorKind {
    InvalidDefinition,
    MetadataUnreadable,
    MetadataStale,
    InvalidInventory,
    FileMismatch,
    MissingStorage,
    UnsafeStorage,
    Storage,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustError {
    kind: RustErrorKind,
    message: String,
}

impl RustError {
    #[cfg(test)]
    pub(crate) fn kind(&self) -> RustErrorKind {
        self.kind
    }
}

impl fmt::Display for RustError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RustError {}

impl From<ArchiveToolError> for RustError {
    fn from(source: ArchiveToolError) -> Self {
        let kind = match source.kind() {
            ArchiveToolErrorKind::InvalidDocument => RustErrorKind::MetadataUnreadable,
            ArchiveToolErrorKind::FileMismatch => RustErrorKind::FileMismatch,
            ArchiveToolErrorKind::MissingStorage => RustErrorKind::MissingStorage,
            ArchiveToolErrorKind::UnsafeStorage => RustErrorKind::UnsafeStorage,
            _ => RustErrorKind::Storage,
        };
        error(kind, source.to_string())
    }
}

pub(crate) fn error(kind: RustErrorKind, message: impl Into<String>) -> RustError {
    RustError {
        kind,
        message: message.into(),
    }
}

fn valid_toolchain(value: &str) -> bool {
    if matches!(value, "stable" | "beta" | "nightly") {
        return true;
    }
    for channel in ["stable-", "beta-", "nightly-"] {
        if let Some(date) = value.strip_prefix(channel) {
            return valid_date(date);
        }
    }
    let base = value.split_once("-beta").map_or(value, |(base, _)| base);
    let fields = base.split('.').collect::<Vec<_>>();
    (fields.len() == 2 || fields.len() == 3)
        && fields
            .iter()
            .all(|field| !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()))
        && value.strip_prefix(base).is_none_or(|suffix| {
            suffix.is_empty()
                || suffix == "-beta"
                || suffix.strip_prefix("-beta.").is_some_and(|number| {
                    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                })
        })
}

fn valid_date(value: &str) -> bool {
    let fields = value.split('-').collect::<Vec<_>>();
    fields.len() == 3
        && [4, 2, 2].into_iter().zip(fields).all(|(length, field)| {
            field.len() == length && field.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests;
