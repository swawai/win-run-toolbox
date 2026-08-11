use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::CommandContext;
use super::filesystem::{
    MAX_METADATA_BYTES, child_file, collect_regular_files, directory_chain, is_lower_hex,
    read_json, sha256_follow, sha256_regular, sha256_text,
};

const HOST: &str = "x86_64-pc-windows-msvc";
const RUSTUP_URL: &str =
    "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe";
const RUSTUP_CHECKSUM_URL: &str =
    "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe.sha256";
const PROXY_LINKS: [&str; 4] = [
    "cargo\\bin\\rustc.exe",
    "cargo\\bin\\cargo.exe",
    "cargo\\bin\\rustfmt.exe",
    "cargo\\bin\\cargo-fmt.exe",
];

struct RustDefinition {
    toolchain: String,
    toolchain_name: String,
}

pub(super) enum RustReport {
    Off,
    Rustup {
        toolchain: String,
        metadata: Option<RustMetadata>,
        ready: bool,
    },
}

impl RustReport {
    pub(super) fn render(&self) {
        match self {
            Self::Off => println!("[OFF] rust is disabled."),
            Self::Rustup {
                toolchain,
                metadata,
                ready,
            } => {
                let state = if *ready { "READY" } else { "MISSING" };
                let version = if *ready {
                    let metadata = metadata.as_ref().expect("ready Rust metadata");
                    format!(
                        "rustc {}, cargo {}",
                        metadata.rustc_version, metadata.cargo_version
                    )
                } else {
                    "not installed".to_owned()
                };
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
    let definition = definition(context)?;
    let metadata = validate_metadata(context, &definition).ok();
    let ready = metadata
        .as_ref()
        .is_some_and(|metadata| hashes_match(context, &definition, metadata));
    Ok(RustReport::Rustup {
        toolchain: definition.toolchain,
        metadata,
        ready,
    })
}

fn definition(context: &CommandContext) -> Result<RustDefinition, String> {
    let toolchain = context
        .environment("SWAWKIT_PROJ_RUST_TOOLCHAIN")
        .to_ascii_lowercase();
    if !valid_toolchain(&toolchain) {
        return Err(
            "SWAWKIT_PROJ_RUST_TOOLCHAIN must be stable, beta, nightly, a Rust version, or a dated channel."
                .to_owned(),
        );
    }
    let profile = context
        .environment("SWAWKIT_PROJ_RUST_PROFILE")
        .to_ascii_lowercase();
    if profile != "minimal" {
        return Err(format!(
            "Unsupported Rust profile '{profile}'. Expected one of: minimal"
        ));
    }
    let host = context
        .environment("SWAWKIT_PROJ_RUST_HOST")
        .to_ascii_lowercase();
    if host != HOST {
        return Err(format!(
            "Rust V0 supports host '{HOST}' only; received '{host}'."
        ));
    }
    Ok(RustDefinition {
        toolchain_name: format!("{toolchain}-{HOST}"),
        toolchain,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RustMetadata {
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
    pub(super) rustc_version: String,
    rustc_commit: String,
    pub(super) cargo_version: String,
    rustfmt_version: String,
    source_verification: String,
    files: Vec<RustFile>,
}

#[derive(Deserialize)]
struct RustFile {
    path: String,
    kind: String,
    target: String,
    length: u64,
    sha256: String,
}

fn validate_metadata(
    context: &CommandContext,
    definition: &RustDefinition,
) -> Result<RustMetadata, String> {
    let root = install_root(context, definition)?;
    let metadata: RustMetadata = read_json(
        &root.join(".swawkit-dev-rust.json"),
        "Rust installation metadata",
        MAX_METADATA_BYTES,
    )?;
    if metadata.schema != "swawkit.proj-dev.rust-install.v0"
        || metadata.name != "rust"
        || metadata.inventory != "toolchain-files-v0"
        || metadata.declared_toolchain != definition.toolchain
        || metadata.toolchain_name != definition.toolchain_name
        || metadata.profile != "minimal"
        || metadata.host != HOST
        || metadata.components != ["rustfmt"]
        || metadata.recipe_version != "2"
        || metadata.definition_signature != definition_signature(definition)
        || metadata.rustup_init_url != RUSTUP_URL
        || !is_lower_hex(&metadata.rustup_init_sha256, 64)
        || !version_prefix(&metadata.rustup_version)
        || !version_prefix(&metadata.rustc_version)
        || !is_lower_hex(&metadata.rustc_commit, 40)
        || !version_prefix(&metadata.cargo_version)
        || !version_prefix(&metadata.rustfmt_version)
        || metadata.source_verification != "rust-static-sha256"
    {
        return Err("Rust installation metadata is stale".to_owned());
    }

    let required = required_paths(definition);
    if metadata.files.len() <= required.len() {
        return Err("Rust installation inventory is incomplete".to_owned());
    }
    let records: BTreeMap<_, _> = metadata
        .files
        .iter()
        .map(|record| (record.path.as_str(), record))
        .collect();
    if records.len() != metadata.files.len() {
        return Err("Rust installation inventory has duplicate paths".to_owned());
    }
    for record in &metadata.files {
        validate_shape(&root, record)?;
    }
    if !required
        .iter()
        .all(|path| records.contains_key(path.as_str()))
    {
        return Err("Rust required file record is missing".to_owned());
    }
    let rustup = records
        .get("cargo\\bin\\rustup.exe")
        .ok_or("Rust rustup record is missing")?;
    if rustup.sha256 != metadata.rustup_init_sha256 {
        return Err("Rust rustup digest does not match its source".to_owned());
    }
    if inventory_paths(&root, definition, &required)?
        != records.keys().map(|path| (*path).to_owned()).collect()
    {
        return Err("Rust installation inventory changed".to_owned());
    }
    Ok(metadata)
}

fn validate_shape(root: &Path, record: &RustFile) -> Result<(), String> {
    if record.path.is_empty()
        || !matches!(record.kind.as_str(), "file" | "symlink")
        || !is_lower_hex(&record.sha256, 64)
    {
        return Err("Rust installed file record is invalid".to_owned());
    }
    let path = child_file(root, &record.path, "Rust installed file")?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        format!(
            "cannot inspect Rust installed file '{}': {error}",
            path.display()
        )
    })?;
    let reparse = metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    if record.kind == "symlink" {
        let target = fs::read_link(&path).map_err(|error| {
            format!("cannot read Rust proxy link '{}': {error}", path.display())
        })?;
        if !reparse
            || !PROXY_LINKS.contains(&record.path.as_str())
            || target != Path::new("rustup.exe")
            || record.target != "rustup.exe"
            || record.length != 0
        {
            return Err(format!(
                "Rust installed link is not an owned rustup proxy: {}",
                record.path
            ));
        }
    } else if !metadata.is_file()
        || reparse
        || metadata.len() == 0
        || metadata.len() != record.length
        || !record.target.is_empty()
    {
        return Err(format!(
            "Rust installed file shape changed: {}",
            record.path
        ));
    }
    Ok(())
}

fn hashes_match(
    context: &CommandContext,
    definition: &RustDefinition,
    metadata: &RustMetadata,
) -> bool {
    let Ok(root) = install_root(context, definition) else {
        return false;
    };
    metadata.files.iter().all(|record| {
        let digest = child_file(&root, &record.path, "Rust installed file").and_then(|path| {
            if record.kind == "symlink" {
                sha256_follow(&path, "Rust installed file")
            } else {
                sha256_regular(&path, "Rust installed file")
            }
        });
        digest.is_ok_and(|actual| actual == record.sha256)
    })
}

fn inventory_paths(
    root: &Path,
    definition: &RustDefinition,
    required: &[String],
) -> Result<BTreeSet<String>, String> {
    let relative_root = format!("rustup\\toolchains\\{}", definition.toolchain_name);
    let toolchain_root = directory_chain(
        root,
        &["rustup", "toolchains", &definition.toolchain_name],
        "Rust toolchain",
    )?;
    let mut paths: BTreeSet<String> = required.iter().cloned().collect();
    for relative in collect_regular_files(&toolchain_root, "Rust toolchain")? {
        paths.insert(format!("{relative_root}\\{relative}"));
    }
    Ok(paths)
}

fn install_root(context: &CommandContext, definition: &RustDefinition) -> Result<PathBuf, String> {
    directory_chain(
        &context.data_root,
        &[
            "modules",
            "kernel",
            ".dev",
            "setup",
            "export",
            "rust",
            "installs",
            &definition.toolchain,
        ],
        "Rust installation",
    )
}

fn required_paths(definition: &RustDefinition) -> Vec<String> {
    let root = format!("rustup\\toolchains\\{}", definition.toolchain_name);
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

fn definition_signature(definition: &RustDefinition) -> String {
    sha256_text(
        &[
            "swawkit.proj-dev.rust-definition.v0",
            "rustup",
            &definition.toolchain,
            "minimal",
            HOST,
            "rustfmt",
            "2",
            RUSTUP_URL,
            RUSTUP_CHECKSUM_URL,
        ]
        .join("\n"),
    )
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
    let fields: Vec<_> = base.split('.').collect();
    (fields.len() == 2 || fields.len() == 3)
        && fields
            .iter()
            .all(|field| !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()))
        && value.strip_prefix(base).is_none_or(|suffix| {
            suffix.is_empty()
                || suffix == "-beta"
                || suffix.strip_prefix("-beta.").is_some_and(|number| {
                    !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
                })
        })
}

fn valid_date(value: &str) -> bool {
    let fields: Vec<_> = value.split('-').collect();
    fields.len() == 3
        && [4, 2, 2].into_iter().zip(fields).all(|(length, field)| {
            field.len() == length && field.bytes().all(|b| b.is_ascii_digit())
        })
}

fn version_prefix(value: &str) -> bool {
    let prefix = value
        .split(|character: char| !character.is_ascii_digit() && character != '.')
        .next()
        .unwrap_or("");
    let fields: Vec<_> = prefix.trim_end_matches('.').split('.').collect();
    fields.len() >= 3
        && fields
            .iter()
            .all(|field| !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()))
}
