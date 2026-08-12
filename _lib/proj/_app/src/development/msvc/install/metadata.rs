use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use crate::development::archive_tool::filesystem::regular_file_digest;

use super::assembly::AssemblyVersions;
use crate::development::msvc::{
    INSTALL_SCHEMA, InstalledFile, MsvcDefinition, MsvcError, MsvcErrorKind, MsvcMetadata,
    MsvcRecipe, RECIPE_VERSION, error, required_paths,
};

pub(super) fn write(
    definition: &MsvcDefinition,
    recipe: &MsvcRecipe,
    root: &Path,
    versions: &AssemblyVersions,
) -> Result<(), MsvcError> {
    let mut files = Vec::new();
    for relative in required_paths(&versions.tool, &versions.sdk) {
        let path = root.join(&relative);
        let (length, sha256) = regular_file_digest(
            &path,
            "MSVC required installed file",
            4 * 1024 * 1024 * 1024,
        )?;
        files.push(InstalledFile {
            path: relative,
            length,
            sha256,
        });
    }
    let metadata = MsvcMetadata {
        schema: INSTALL_SCHEMA.to_owned(),
        name: "msvc".to_owned(),
        channel: definition.channel().to_owned(),
        channel_url: definition.channel_url(),
        recipe_version: RECIPE_VERSION.to_owned(),
        definition_signature: definition.definition_signature(),
        manifest_url: recipe.manifest().url().to_owned(),
        manifest_sha256: recipe.manifest_sha256().to_owned(),
        tool_package_version: recipe.tool_package_version().to_owned(),
        tool_version: versions.tool.clone(),
        sdk_package: recipe.sdk_package().to_owned(),
        sdk_version: versions.sdk.clone(),
        source_verification: "microsoft-manifest".to_owned(),
        files,
    };
    let content = serde_json::to_string_pretty(&metadata)
        .map_err(|cause| {
            error(
                MsvcErrorKind::InstallationFailed,
                format!("cannot serialize MSVC installation metadata: {cause}"),
            )
        })?
        .replace('\n', "\r\n")
        + "\r\n";
    let path = root.join(".swawkit-dev-msvc.json");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|cause| install_io("create", &path, cause))?;
    file.write_all(content.as_bytes())
        .map_err(|cause| install_io("write", &path, cause))?;
    file.sync_all()
        .map_err(|cause| install_io("flush", &path, cause))
}

fn install_io(action: &str, path: &Path, cause: std::io::Error) -> MsvcError {
    error(
        MsvcErrorKind::InstallationFailed,
        format!(
            "cannot {action} MSVC installation metadata '{}': {cause}",
            path.display()
        ),
    )
}

#[cfg(test)]
mod tests;
