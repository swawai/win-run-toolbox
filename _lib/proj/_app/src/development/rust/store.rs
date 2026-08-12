use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{RUSTUP_URL, RustDefinition, RustError, RustErrorKind, error};
use crate::development::archive_tool::filesystem::{
    MAX_METADATA_BYTES, child_file, is_lower_hex, is_reparse, read_json, regular_directory,
    verify_regular_file,
};

const PROXY_LINKS: [&str; 4] = [
    "cargo\\bin\\rustc.exe",
    "cargo\\bin\\cargo.exe",
    "cargo\\bin\\rustfmt.exe",
    "cargo\\bin\\cargo-fmt.exe",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustInstallation {
    pub(super) root: PathBuf,
    metadata: RustMetadata,
}

impl RustInstallation {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn rustc_version(&self) -> &str {
        &self.metadata.rustc_version
    }

    pub fn cargo_version(&self) -> &str {
        &self.metadata.cargo_version
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RustMetadata {
    schema: String,
    name: String,
    inventory: String,
    declared_toolchain: String,
    toolchain_name: String,
    profile: String,
    host: String,
    components: Vec<String>,
    recipe_version: String,
    definition_signature: String,
    rustup_init_url: String,
    rustup_init_sha256: String,
    rustup_version: String,
    rustc_version: String,
    rustc_commit: String,
    cargo_version: String,
    rustfmt_version: String,
    source_verification: String,
    files: Vec<RustFile>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct RustFile {
    path: String,
    kind: String,
    target: String,
    length: u64,
    sha256: String,
}

pub(super) fn read_installation_at(
    definition: &RustDefinition,
    root: &Path,
) -> Result<RustInstallation, RustError> {
    regular_directory(root, "Rust installation")?;
    let metadata: RustMetadata = read_json(
        &root.join(".swawkit-dev-rust.json"),
        "Rust installation metadata",
        MAX_METADATA_BYTES,
    )?;
    validate_metadata(definition, &metadata)?;
    validate_inventory(definition, root, &metadata)?;
    verify_hashes(root, &metadata)?;
    Ok(RustInstallation {
        root: root.to_path_buf(),
        metadata,
    })
}

fn validate_metadata(
    definition: &RustDefinition,
    metadata: &RustMetadata,
) -> Result<(), RustError> {
    let valid = metadata.schema == "swawkit.proj-dev.rust-install.v0"
        && metadata.name == "rust"
        && metadata.inventory == "toolchain-files-v0"
        && metadata.declared_toolchain == definition.toolchain()
        && metadata.toolchain_name == definition.toolchain_name()
        && metadata.profile == definition.profile()
        && metadata.host == definition.host()
        && metadata.components == definition.required_components()
        && metadata.recipe_version == definition.recipe_version()
        && metadata.definition_signature == definition.definition_signature()
        && metadata.rustup_init_url == RUSTUP_URL
        && is_lower_hex(&metadata.rustup_init_sha256, 64)
        && version_prefix(&metadata.rustup_version)
        && version_prefix(&metadata.rustc_version)
        && is_lower_hex(&metadata.rustc_commit, 40)
        && version_prefix(&metadata.cargo_version)
        && version_prefix(&metadata.rustfmt_version)
        && metadata.source_verification == "rust-static-sha256";
    if valid {
        Ok(())
    } else {
        Err(error(
            RustErrorKind::MetadataStale,
            "Rust installation metadata is stale",
        ))
    }
}

fn validate_inventory(
    definition: &RustDefinition,
    root: &Path,
    metadata: &RustMetadata,
) -> Result<(), RustError> {
    let required = definition.required_paths();
    if metadata.files.len() <= required.len() {
        return Err(invalid_inventory(
            "Rust installation inventory is incomplete",
        ));
    }
    let records = metadata
        .files
        .iter()
        .map(|record| (record.path.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    if records.len() != metadata.files.len() {
        return Err(invalid_inventory(
            "Rust installation inventory has duplicate paths",
        ));
    }
    for record in &metadata.files {
        validate_shape(root, record)?;
    }
    if !required
        .iter()
        .all(|path| records.contains_key(path.as_str()))
    {
        return Err(invalid_inventory("Rust required file record is missing"));
    }
    let rustup = records
        .get("cargo\\bin\\rustup.exe")
        .ok_or_else(|| invalid_inventory("Rust rustup record is missing"))?;
    if rustup.sha256 != metadata.rustup_init_sha256 {
        return Err(invalid_inventory(
            "Rust rustup digest does not match its source",
        ));
    }
    if inventory_paths(root, definition, &required)?
        != records.keys().map(|path| (*path).to_owned()).collect()
    {
        return Err(invalid_inventory("Rust installation inventory changed"));
    }
    Ok(())
}

fn validate_shape(root: &Path, record: &RustFile) -> Result<(), RustError> {
    if record.path.is_empty() || !is_lower_hex(&record.sha256, 64) {
        return Err(invalid_inventory("Rust installed file record is invalid"));
    }
    let path = child_file(root, &record.path, "Rust installed file")?;
    let metadata = fs::symlink_metadata(&path).map_err(|cause| {
        error(
            if cause.kind() == std::io::ErrorKind::NotFound {
                RustErrorKind::MissingStorage
            } else {
                RustErrorKind::Storage
            },
            format!(
                "cannot inspect Rust installed file '{}': {cause}",
                path.display()
            ),
        )
    })?;
    match record.kind.as_str() {
        "symlink" => {
            let target = fs::read_link(&path).map_err(|cause| {
                error(
                    RustErrorKind::Storage,
                    format!("cannot read Rust proxy link '{}': {cause}", path.display()),
                )
            })?;
            if !is_reparse(&metadata)
                || !PROXY_LINKS.contains(&record.path.as_str())
                || target != Path::new("rustup.exe")
                || record.target != "rustup.exe"
                || record.length != 0
            {
                return Err(invalid_inventory(
                    "Rust installed link is not an owned rustup proxy",
                ));
            }
        }
        "file"
            if metadata.is_file()
                && !is_reparse(&metadata)
                && metadata.len() != 0
                && metadata.len() == record.length
                && record.target.is_empty() => {}
        "file" => return Err(invalid_inventory("Rust installed file shape changed")),
        _ => return Err(invalid_inventory("Rust installed file record is invalid")),
    }
    Ok(())
}

fn verify_hashes(root: &Path, metadata: &RustMetadata) -> Result<(), RustError> {
    for record in &metadata.files {
        let path = child_file(root, &record.path, "Rust installed file")?;
        if record.kind == "symlink" {
            verify_followed(&path, record)?;
        } else {
            verify_regular_file(&path, "Rust installed file", record.length, &record.sha256)?;
        }
    }
    Ok(())
}

fn verify_followed(path: &Path, record: &RustFile) -> Result<(), RustError> {
    let file = fs::File::open(path).map_err(|cause| {
        error(
            RustErrorKind::Storage,
            format!(
                "cannot open Rust proxy target '{}': {cause}",
                path.display()
            ),
        )
    })?;
    let metadata = file.metadata().map_err(|cause| {
        error(
            RustErrorKind::Storage,
            format!(
                "cannot inspect Rust proxy target '{}': {cause}",
                path.display()
            ),
        )
    })?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(error(
            RustErrorKind::FileMismatch,
            format!(
                "Rust proxy target is not a non-empty file: {}",
                path.display()
            ),
        ));
    }
    let mut digest = Sha256::new();
    std::io::copy(&mut BufReader::new(file), &mut digest).map_err(|cause| {
        error(
            RustErrorKind::Storage,
            format!(
                "cannot hash Rust proxy target '{}': {cause}",
                path.display()
            ),
        )
    })?;
    if format!("{:x}", digest.finalize()) != record.sha256 {
        return Err(error(
            RustErrorKind::FileMismatch,
            format!(
                "Rust installed file SHA-256 does not match its published metadata: {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn inventory_paths(
    root: &Path,
    definition: &RustDefinition,
    required: &[String],
) -> Result<BTreeSet<String>, RustError> {
    let relative_root = format!("rustup\\toolchains\\{}", definition.toolchain_name());
    let toolchain_root = root
        .join("rustup")
        .join("toolchains")
        .join(definition.toolchain_name());
    regular_directory(&toolchain_root, "Rust toolchain")?;
    let mut paths = required.iter().cloned().collect::<BTreeSet<_>>();
    for relative in collect_files(&toolchain_root)? {
        paths.insert(format!("{relative_root}\\{relative}"));
    }
    Ok(paths)
}

fn collect_files(root: &Path) -> Result<BTreeSet<String>, RustError> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|cause| {
            error(
                RustErrorKind::Storage,
                format!(
                    "cannot enumerate Rust toolchain '{}': {cause}",
                    directory.display()
                ),
            )
        })? {
            let entry = entry.map_err(|cause| error(RustErrorKind::Storage, cause.to_string()))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|cause| {
                error(
                    RustErrorKind::Storage,
                    format!(
                        "cannot inspect Rust toolchain '{}': {cause}",
                        path.display()
                    ),
                )
            })?;
            if metadata.is_dir() && !is_reparse(&metadata) {
                pending.push(path);
            } else if metadata.is_file() || is_reparse(&metadata) {
                let relative = path.strip_prefix(root).map_err(|_| {
                    error(
                        RustErrorKind::UnsafeStorage,
                        format!("Rust toolchain escaped its root: {}", path.display()),
                    )
                })?;
                files.insert(relative.to_string_lossy().into_owned());
            } else {
                return Err(error(
                    RustErrorKind::UnsafeStorage,
                    format!("unsupported Rust toolchain entry: {}", path.display()),
                ));
            }
        }
    }
    Ok(files)
}

fn invalid_inventory(message: impl Into<String>) -> RustError {
    error(RustErrorKind::InvalidInventory, message)
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
