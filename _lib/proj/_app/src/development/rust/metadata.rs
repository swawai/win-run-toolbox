use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::BufReader;
use std::os::windows::fs::MetadataExt;
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::probe::RustProbe;
use super::{RUSTUP_URL, RustDefinition, RustError, RustErrorKind, error};
use crate::atomic_file;
use crate::development::archive_tool::filesystem::{child_file, regular_directory};

const PROXY_LINKS: [&str; 4] = [
    "cargo\\bin\\rustc.exe",
    "cargo\\bin\\cargo.exe",
    "cargo\\bin\\rustfmt.exe",
    "cargo\\bin\\cargo-fmt.exe",
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Metadata<'a> {
    schema: &'static str,
    name: &'static str,
    inventory: &'static str,
    declared_toolchain: &'a str,
    toolchain_name: &'a str,
    profile: &'a str,
    host: &'a str,
    components: &'a [&'a str],
    recipe_version: &'a str,
    definition_signature: String,
    rustup_init_url: &'static str,
    rustup_init_sha256: &'a str,
    rustup_version: &'a str,
    rustc_version: &'a str,
    rustc_commit: &'a str,
    cargo_version: &'a str,
    rustfmt_version: &'a str,
    source_verification: &'static str,
    files: Vec<FileRecord>,
}

#[derive(Serialize)]
struct FileRecord {
    path: String,
    kind: &'static str,
    target: &'static str,
    length: u64,
    sha256: String,
}

pub(super) fn write(
    definition: &RustDefinition,
    probe: &RustProbe,
    root: &Path,
    rustup_sha256: &str,
) -> Result<(), RustError> {
    let inventory = inventory_paths(definition, root)?;
    let files = inventory
        .into_iter()
        .map(|relative| record(root, relative))
        .collect::<Result<Vec<_>, _>>()?;
    let metadata = Metadata {
        schema: "swawkit.proj-dev.rust-install.v0",
        name: "rust",
        inventory: "toolchain-files-v0",
        declared_toolchain: definition.toolchain(),
        toolchain_name: definition.toolchain_name(),
        profile: definition.profile(),
        host: definition.host(),
        components: definition.required_components(),
        recipe_version: definition.recipe_version(),
        definition_signature: definition.definition_signature(),
        rustup_init_url: RUSTUP_URL,
        rustup_init_sha256: rustup_sha256,
        rustup_version: &probe.rustup_version,
        rustc_version: &probe.rustc_version,
        rustc_commit: &probe.rustc_commit,
        cargo_version: &probe.cargo_version,
        rustfmt_version: &probe.rustfmt_version,
        source_verification: "rust-static-sha256",
        files,
    };
    let content = serde_json::to_vec_pretty(&metadata).map_err(|cause| {
        error(
            RustErrorKind::InstallationFailed,
            format!("cannot serialize Rust installation metadata: {cause}"),
        )
    })?;
    atomic_file::publish(&root.join(".swawkit-dev-rust.json"), &content).map_err(|cause| {
        error(
            RustErrorKind::Storage,
            format!("cannot publish Rust installation metadata: {cause}"),
        )
    })
}

fn inventory_paths(
    definition: &RustDefinition,
    root: &Path,
) -> Result<BTreeSet<String>, RustError> {
    let mut paths = definition
        .required_paths()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let toolchain = root
        .join("rustup")
        .join("toolchains")
        .join(definition.toolchain_name());
    regular_directory(&toolchain, "Rust toolchain")?;
    let mut pending = vec![toolchain.clone()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.is_dir() && !is_reparse(&metadata) {
                pending.push(path);
            } else if metadata.is_file() || is_reparse(&metadata) {
                let relative = path.strip_prefix(root).map_err(|_| {
                    error(
                        RustErrorKind::UnsafeStorage,
                        format!("Rust inventory escaped its root: {}", path.display()),
                    )
                })?;
                paths.insert(relative.to_string_lossy().into_owned());
            } else {
                return Err(error(
                    RustErrorKind::UnsafeStorage,
                    format!("unsupported Rust inventory entry: {}", path.display()),
                ));
            }
        }
    }
    Ok(paths)
}

fn record(root: &Path, relative: String) -> Result<FileRecord, RustError> {
    let path = child_file(root, &relative, "Rust installed file")?;
    let metadata = fs::symlink_metadata(&path)?;
    let (kind, target, length) = if is_reparse(&metadata) {
        let target = fs::read_link(&path)?;
        if !PROXY_LINKS.contains(&relative.as_str()) || target != Path::new("rustup.exe") {
            return Err(error(
                RustErrorKind::UnsafeStorage,
                format!("Rust installed link is not an owned rustup proxy: {relative}"),
            ));
        }
        ("symlink", "rustup.exe", 0)
    } else if metadata.is_file() && metadata.len() != 0 {
        ("file", "", metadata.len())
    } else {
        return Err(error(
            RustErrorKind::FileMismatch,
            format!("Rust installed file is empty or unsafe: {relative}"),
        ));
    };
    let sha256 = followed_digest(&path)?;
    Ok(FileRecord {
        path: relative,
        kind,
        target,
        length,
        sha256,
    })
}

fn followed_digest(path: &Path) -> Result<String, RustError> {
    let file = OpenOptions::new().read(true).open(path)?;
    let length = file.metadata()?.len();
    if length == 0 {
        return Err(error(
            RustErrorKind::FileMismatch,
            format!("Rust installed file target is empty: {}", path.display()),
        ));
    }
    let mut digest = Sha256::new();
    let copied = std::io::copy(&mut BufReader::new(file), &mut digest)?;
    if copied != length {
        return Err(error(
            RustErrorKind::FileMismatch,
            format!(
                "Rust installed file changed while hashing: {}",
                path.display()
            ),
        ));
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}
