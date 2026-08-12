use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use crate::development::archive_tool::install::remove_controlled_data;
use crate::development::msvc::{MsvcError, MsvcErrorKind, error, is_numeric_dotted};

pub(super) struct AssemblyVersions {
    pub(super) tool: String,
    pub(super) sdk: String,
}

pub(super) fn complete(root: &Path) -> Result<AssemblyVersions, MsvcError> {
    let versions = AssemblyVersions {
        tool: one_version(&root.join(r"VC\Tools\MSVC"), "MSVC tool")?,
        sdk: one_version(&root.join(r"Windows Kits\10\bin"), "Windows SDK")?,
    };
    let dia = checked_child(root, r"DIA SDK\bin\amd64\msdia140.dll", false)?;
    require_file(&dia, "x64 DIA runtime")?;
    let tool_bin = ensure_child_directory(
        root,
        &format!(r"VC\Tools\MSVC\{}\bin\Hostx64\x64", versions.tool),
    )?;
    copy_replace(&dia, &tool_bin.join("msdia140.dll"))?;

    let mut optional = vec![
        format!(r"VC\Tools\MSVC\{}\bin\Hostx64\x64\vctip.exe", versions.tool),
        "Common7".to_owned(),
        "Catalogs".to_owned(),
        "DesignTime".to_owned(),
        r"Windows Kits\10\Catalogs".to_owned(),
        r"Windows Kits\10\DesignTime".to_owned(),
        format!(r"VC\Tools\MSVC\{}\bin\Hostx86", versions.tool),
        format!(r"VC\Tools\MSVC\{}\bin\Hostarm", versions.tool),
        format!(r"VC\Tools\MSVC\{}\bin\Hostarm64", versions.tool),
    ];
    for architecture in ["x86", "arm", "arm64"] {
        optional.extend([
            format!(r"Windows Kits\10\bin\{}\{architecture}", versions.sdk),
            format!(r"Windows Kits\10\Lib\{}\ucrt\{architecture}", versions.sdk),
            format!(r"Windows Kits\10\Lib\{}\um\{architecture}", versions.sdk),
        ]);
    }
    for relative in optional {
        let path = checked_child(root, &relative, true)?;
        remove_controlled_data(&path)?;
    }

    let build = ensure_child_directory(root, r"VC\Auxiliary\Build")?;
    write_new(
        &build.join("vcvarsall.bat"),
        b"@echo off\r\nrem Compatibility marker for tools such as nvcc.\r\n",
    )?;
    write_new(
        &build.join("vcvars64.bat"),
        b"@echo off\r\ncall \"%~dp0..\\..\\..\\setup_x64.bat\"\r\n",
    )?;
    write_new(
        &root.join("setup_x64.bat"),
        setup_script(&versions).as_bytes(),
    )?;
    Ok(versions)
}

fn setup_script(versions: &AssemblyVersions) -> String {
    let tool = &versions.tool;
    let sdk = &versions.sdk;
    [
        "@echo off".to_owned(),
        "set \"VSCMD_ARG_HOST_ARCH=x64\"".to_owned(),
        "set \"VSCMD_ARG_TGT_ARCH=x64\"".to_owned(),
        format!("set \"VCToolsVersion={tool}\""),
        format!("set \"WindowsSDKVersion={sdk}\\\""),
        format!("set \"VCToolsInstallDir=%~dp0VC\\Tools\\MSVC\\{tool}\\\""),
        "set \"VCINSTALLDIR=%~dp0VC\\\"".to_owned(),
        "set \"WindowsSdkDir=%~dp0Windows Kits\\10\\\"".to_owned(),
        "set \"WindowsSdkBinPath=%~dp0Windows Kits\\10\\bin\\\"".to_owned(),
        format!("set \"WindowsSdkVerBinPath=%~dp0Windows Kits\\10\\bin\\{sdk}\\x64\\\""),
        "set \"UniversalCRTSdkDir=%~dp0Windows Kits\\10\\\"".to_owned(),
        format!("set \"UCRTVersion={sdk}\""),
        format!("set \"PATH=%~dp0VC\\Tools\\MSVC\\{tool}\\bin\\Hostx64\\x64;%~dp0Windows Kits\\10\\bin\\{sdk}\\x64;%~dp0Windows Kits\\10\\bin\\{sdk}\\x64\\ucrt;%PATH%\""),
        format!("set \"INCLUDE=%~dp0VC\\Tools\\MSVC\\{tool}\\include;%~dp0Windows Kits\\10\\Include\\{sdk}\\ucrt;%~dp0Windows Kits\\10\\Include\\{sdk}\\shared;%~dp0Windows Kits\\10\\Include\\{sdk}\\um;%~dp0Windows Kits\\10\\Include\\{sdk}\\winrt;%~dp0Windows Kits\\10\\Include\\{sdk}\\cppwinrt\""),
        format!("set \"LIB=%~dp0VC\\Tools\\MSVC\\{tool}\\lib\\x64;%~dp0Windows Kits\\10\\Lib\\{sdk}\\ucrt\\x64;%~dp0Windows Kits\\10\\Lib\\{sdk}\\um\\x64\""),
    ]
    .join("\r\n")
        + "\r\n"
}

fn one_version(parent: &Path, subject: &str) -> Result<String, MsvcError> {
    require_directory(parent, subject)?;
    let mut versions = Vec::new();
    for entry in fs::read_dir(parent).map_err(|cause| storage("scan", parent, cause))? {
        let entry = entry.map_err(|cause| storage("read", parent, cause))?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|cause| storage("inspect", &entry.path(), cause))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if metadata.is_dir() && !is_reparse(&metadata) && is_numeric_dotted(&name) {
            versions.push(name);
        }
    }
    if versions.len() != 1 {
        return Err(error(
            MsvcErrorKind::InstallationFailed,
            format!(
                "expected one extracted {subject} version; found {}",
                versions.len()
            ),
        ));
    }
    Ok(versions.remove(0))
}

fn checked_child(root: &Path, relative: &str, allow_missing: bool) -> Result<PathBuf, MsvcError> {
    require_directory(root, "staged MSVC installation")?;
    let components = Path::new(relative).components().collect::<Vec<_>>();
    if components.is_empty()
        || components
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(error(
            MsvcErrorKind::UnsafeStorage,
            format!("unsafe MSVC path '{relative}'"),
        ));
    }
    let destination = root.join(relative);
    let mut path = root.to_path_buf();
    let mut missing = false;
    for component in components {
        path.push(component.as_os_str());
        if missing {
            continue;
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata) if !is_reparse(&metadata) => {}
            Ok(_) => {
                return Err(error(
                    MsvcErrorKind::UnsafeStorage,
                    format!("MSVC path cannot be a reparse point: {}", path.display()),
                ));
            }
            Err(cause) if allow_missing && cause.kind() == io::ErrorKind::NotFound => {
                missing = true;
            }
            Err(cause) => return Err(storage("inspect", &path, cause)),
        }
    }
    Ok(destination)
}

fn ensure_child_directory(root: &Path, relative: &str) -> Result<PathBuf, MsvcError> {
    require_directory(root, "staged MSVC installation")?;
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(name) = component else {
            return Err(error(
                MsvcErrorKind::UnsafeStorage,
                format!("unsafe MSVC directory '{relative}'"),
            ));
        };
        path.push(name);
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(cause) if cause.kind() == io::ErrorKind::AlreadyExists => {}
            Err(cause) => return Err(storage("create", &path, cause)),
        }
        require_directory(&path, "MSVC directory")?;
    }
    Ok(path)
}

fn copy_replace(source: &Path, destination: &Path) -> Result<(), MsvcError> {
    remove_controlled_data(destination)?;
    let mut input = fs::File::open(source).map_err(|cause| storage("open", source, cause))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|cause| storage("create", destination, cause))?;
    io::copy(&mut input, &mut output).map_err(|cause| storage("copy to", destination, cause))?;
    output
        .sync_all()
        .map_err(|cause| storage("flush", destination, cause))
}

fn write_new(path: &Path, content: &[u8]) -> Result<(), MsvcError> {
    remove_controlled_data(path)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|cause| storage("create", path, cause))?;
    file.write_all(content)
        .map_err(|cause| storage("write", path, cause))?;
    file.sync_all()
        .map_err(|cause| storage("flush", path, cause))
}

fn require_file(path: &Path, subject: &str) -> Result<(), MsvcError> {
    let metadata = fs::symlink_metadata(path).map_err(|cause| storage("inspect", path, cause))?;
    if metadata.is_file() && !is_reparse(&metadata) {
        Ok(())
    } else {
        Err(error(
            MsvcErrorKind::InstallationFailed,
            format!(
                "the extracted MSVC payload is missing the {subject}: {}",
                path.display()
            ),
        ))
    }
}

fn require_directory(path: &Path, subject: &str) -> Result<(), MsvcError> {
    let metadata = fs::symlink_metadata(path).map_err(|cause| storage("inspect", path, cause))?;
    if metadata.is_dir() && !is_reparse(&metadata) {
        Ok(())
    } else {
        Err(error(
            MsvcErrorKind::UnsafeStorage,
            format!("{subject} must be a regular directory: {}", path.display()),
        ))
    }
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn storage(action: &str, path: &Path, cause: io::Error) -> MsvcError {
    error(
        MsvcErrorKind::InstallationFailed,
        format!("cannot {action} MSVC path '{}': {cause}", path.display()),
    )
}

#[cfg(test)]
mod tests;
