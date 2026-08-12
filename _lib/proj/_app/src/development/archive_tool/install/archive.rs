use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};
use zip::ZipArchive;

use super::super::{ArchiveToolError, ArchiveToolErrorKind};

const MAX_ENTRIES: usize = 200_000;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 12 * 1024 * 1024 * 1024;

pub(super) fn test(path: &Path) -> Result<(), ArchiveToolError> {
    let file = open_regular(path, "ZIP archive")?;
    test_file(&file)
}

pub(super) fn test_file(file: &File) -> Result<(), ArchiveToolError> {
    let file = clone_from_start(file, "ZIP archive")?;
    let mut archive = ZipArchive::new(file)
        .map_err(|cause| archive_error(format!("invalid ZIP archive: {cause}")))?;
    inspect(&mut archive).map(|_| ())
}

pub(super) fn extract(path: &Path, destination: &Path) -> Result<(), ArchiveToolError> {
    let file = open_regular(path, "ZIP archive")?;
    extract_file(&file, destination)
}

pub(super) fn extract_file(file: &File, destination: &Path) -> Result<(), ArchiveToolError> {
    extract_selected(file, destination, true, |name| Ok(Some(name.to_owned()))).map(|_| ())
}

pub(super) fn extract_contents_file(
    file: &File,
    destination: &Path,
) -> Result<(), ArchiveToolError> {
    let extracted = extract_selected(file, destination, false, |name| {
        if !name
            .get(.."Contents/".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Contents/"))
        {
            return Ok(None);
        }
        decode_vsix_path(&name["Contents/".len()..]).map(Some)
    })?;
    if extracted == 0 {
        return Err(archive_error("VSIX has no Contents payload"));
    }
    Ok(())
}

fn extract_selected(
    file: &File,
    destination: &Path,
    require_empty: bool,
    mut select: impl FnMut(&str) -> Result<Option<String>, ArchiveToolError>,
) -> Result<usize, ArchiveToolError> {
    validate_directory(destination, "ZIP destination", require_empty)?;
    let file = clone_from_start(file, "ZIP archive")?;
    let mut archive = ZipArchive::new(file)
        .map_err(|cause| archive_error(format!("invalid ZIP archive: {cause}")))?;
    if archive.len() > MAX_ENTRIES {
        return Err(archive_error(format!(
            "ZIP archive contains more than {MAX_ENTRIES} entries"
        )));
    }
    let mut names = HashSet::new();
    let mut entries = Vec::new();
    let mut declared_total = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|cause| archive_error(format!("cannot inspect ZIP entry {index}: {cause}")))?;
        let Some(selected) = select(entry.name())? else {
            continue;
        };
        if selected.is_empty() {
            continue;
        }
        let (relative, key) = windows_entry_path(&selected)?;
        inspect_entry(&entry, &relative, &key, &mut names, &mut declared_total)?;
        entries.push((index, relative));
    }
    let mut extracted_total = 0u64;
    for (index, relative) in &entries {
        let mut entry = archive
            .by_index(*index)
            .map_err(|cause| archive_error(format!("cannot read ZIP entry {index}: {cause}")))?;
        let target = destination.join(relative);
        if entry.is_dir() {
            create_directory_chain(destination, &target)?;
            continue;
        }
        let parent = target
            .parent()
            .ok_or_else(|| archive_error("ZIP entry has no parent"))?;
        create_directory_chain(destination, parent)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|cause| {
                storage_error(format!(
                    "cannot create ZIP file '{}': {cause}",
                    target.display()
                ))
            })?;
        let expected = entry.size();
        copy_entry_bounded(
            &mut entry,
            &mut output,
            expected,
            &mut extracted_total,
            relative,
        )?;
        output
            .flush()
            .map_err(|cause| storage_error(format!("cannot flush ZIP file: {cause}")))?;
    }
    Ok(entries.len())
}

fn decode_vsix_path(value: &str) -> Result<String, ArchiveToolError> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return Err(archive_error(format!(
                    "VSIX entry has invalid percent encoding: {value}"
                )));
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    percent_encoding::percent_decode_str(value)
        .decode_utf8()
        .map(|decoded| decoded.into_owned())
        .map_err(|_| archive_error(format!("VSIX entry path is not valid UTF-8: {value}")))
}

fn inspect<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<Vec<PathBuf>, ArchiveToolError> {
    if archive.len() > MAX_ENTRIES {
        return Err(archive_error(format!(
            "ZIP archive contains more than {MAX_ENTRIES} entries"
        )));
    }
    let mut total = 0u64;
    let mut names = HashSet::new();
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|cause| archive_error(format!("cannot inspect ZIP entry {index}: {cause}")))?;
        let (relative, key) = windows_entry_path(entry.name())?;
        inspect_entry(&entry, &relative, &key, &mut names, &mut total)?;
        entries.push(relative);
    }
    Ok(entries)
}

fn inspect_entry<R: Read>(
    entry: &zip::read::ZipFile<'_, R>,
    relative: &Path,
    key: &str,
    names: &mut HashSet<String>,
    total: &mut u64,
) -> Result<(), ArchiveToolError> {
    if entry
        .unix_mode()
        .is_some_and(|mode| mode & 0o170000 == 0o120000)
    {
        return Err(archive_error(format!(
            "ZIP entry cannot be a symbolic link: {}",
            entry.name()
        )));
    }
    if entry.size() > MAX_FILE_BYTES {
        return Err(archive_error(format!(
            "ZIP entry exceeds the 4 GB safety limit: {}",
            entry.name()
        )));
    }
    *total = total
        .checked_add(entry.size())
        .filter(|value| *value <= MAX_TOTAL_BYTES)
        .ok_or_else(|| archive_error("ZIP archive exceeds the 12 GB safety limit"))?;
    if !names.insert(key.to_owned()) {
        return Err(archive_error(format!(
            "ZIP archive contains a duplicate Windows path: {}",
            relative.display()
        )));
    }
    Ok(())
}

fn copy_entry_bounded(
    reader: &mut impl Read,
    writer: &mut impl Write,
    expected: u64,
    extracted_total: &mut u64,
    relative: &Path,
) -> Result<(), ArchiveToolError> {
    let limit = expected
        .checked_add(1)
        .ok_or_else(|| archive_error("ZIP entry declares an unsupported size"))?;
    let mut reader = reader.take(limit);
    let mut copied = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer).map_err(|cause| {
            archive_error(format!(
                "cannot read ZIP entry '{}': {cause}",
                relative.display()
            ))
        })?;
        if count == 0 {
            break;
        }
        copied = copied
            .checked_add(count as u64)
            .filter(|value| *value <= expected)
            .ok_or_else(|| {
                archive_error(format!(
                    "ZIP entry expands beyond its declared size: {}",
                    relative.display()
                ))
            })?;
        let next_total = extracted_total
            .checked_add(count as u64)
            .filter(|value| *value <= MAX_TOTAL_BYTES)
            .ok_or_else(|| archive_error("ZIP extraction exceeds the 12 GB safety limit"))?;
        writer.write_all(&buffer[..count]).map_err(|cause| {
            storage_error(format!(
                "cannot extract ZIP file '{}': {cause}",
                relative.display()
            ))
        })?;
        *extracted_total = next_total;
    }
    if copied != expected {
        return Err(archive_error(format!(
            "ZIP entry extracted incompletely: {}",
            relative.display()
        )));
    }
    Ok(())
}

fn windows_entry_path(name: &str) -> Result<(PathBuf, String), ArchiveToolError> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.chars().any(|character| character == '\0')
    {
        return Err(unsafe_entry_name(name));
    }
    let body = name.strip_suffix('/').unwrap_or(name);
    if body.is_empty() || body.ends_with('/') {
        return Err(unsafe_entry_name(name));
    }

    let mut path = PathBuf::new();
    let mut normalized = Vec::new();
    for component in body.split('/') {
        validate_windows_component(component).map_err(|reason| {
            archive_error(format!(
                "ZIP entry has an unsafe Windows path '{name}': {reason}"
            ))
        })?;
        path.push(component);
        normalized.push(component.to_lowercase());
    }
    Ok((path, normalized.join("\\")))
}

fn validate_windows_component(component: &str) -> Result<(), &'static str> {
    if component.is_empty() {
        return Err("empty path component");
    }
    if matches!(component, "." | "..") {
        return Err("relative path component");
    }
    if component.ends_with('.') || component.ends_with(' ') {
        return Err("path component ends with a dot or space");
    }
    if component.chars().any(|character| {
        character <= '\u{1f}' || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
    }) {
        return Err("path component contains a Windows-invalid character");
    }
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches(|character| matches!(character, ' ' | '.'));
    if is_windows_device_name(stem) {
        return Err("reserved DOS device name");
    }
    Ok(())
}

fn is_windows_device_name(stem: &str) -> bool {
    let stem = stem.to_uppercase();
    matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2"
                    | "3"
                    | "4"
                    | "5"
                    | "6"
                    | "7"
                    | "8"
                    | "9"
                    | "\u{00b9}"
                    | "\u{00b2}"
                    | "\u{00b3}"
            )
        })
}

fn unsafe_entry_name(name: &str) -> ArchiveToolError {
    archive_error(format!("ZIP entry has an unsafe path: {name}"))
}

fn open_regular(path: &Path, subject: &str) -> Result<File, ArchiveToolError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|cause| {
            storage_error(format!(
                "cannot open {subject} '{}': {cause}",
                path.display()
            ))
        })?;
    let metadata = file.metadata().map_err(|cause| {
        storage_error(format!(
            "cannot inspect {subject} '{}': {cause}",
            path.display()
        ))
    })?;
    if !metadata.is_file() || is_reparse(&metadata) {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!("{subject} is not a regular file: {}", path.display()),
        ));
    }
    Ok(file)
}

fn clone_from_start(file: &File, subject: &str) -> Result<File, ArchiveToolError> {
    let mut clone = file
        .try_clone()
        .map_err(|cause| storage_error(format!("cannot clone the opened {subject}: {cause}")))?;
    clone
        .seek(SeekFrom::Start(0))
        .map_err(|cause| storage_error(format!("cannot seek the opened {subject}: {cause}")))?;
    Ok(clone)
}

fn validate_directory(
    path: &Path,
    subject: &str,
    require_empty: bool,
) -> Result<(), ArchiveToolError> {
    let metadata = fs::symlink_metadata(path).map_err(|cause| {
        storage_error(format!(
            "cannot inspect {subject} '{}': {cause}",
            path.display()
        ))
    })?;
    if !metadata.is_dir() || is_reparse(&metadata) {
        return Err(ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            format!("{subject} is not a regular directory: {}", path.display()),
        ));
    }
    if require_empty
        && fs::read_dir(path)
            .map_err(|cause| storage_error(format!("cannot inspect ZIP destination: {cause}")))?
            .next()
            .is_some()
    {
        return Err(storage_error(format!(
            "ZIP destination must be empty: {}",
            path.display()
        )));
    }
    Ok(())
}

fn create_directory_chain(root: &Path, target: &Path) -> Result<(), ArchiveToolError> {
    let relative = target.strip_prefix(root).map_err(|_| {
        ArchiveToolError::new(
            ArchiveToolErrorKind::UnsafeStorage,
            "ZIP entry escapes its destination",
        )
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::create_dir(&current) {
            Ok(()) => {}
            Err(cause) if cause.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&current).map_err(|inspect_cause| {
                    storage_error(format!(
                        "cannot inspect ZIP directory '{}': {inspect_cause}",
                        current.display()
                    ))
                })?;
                if !metadata.is_dir() || is_reparse(&metadata) {
                    return Err(ArchiveToolError::new(
                        ArchiveToolErrorKind::UnsafeStorage,
                        format!(
                            "ZIP directory is not a regular directory: {}",
                            current.display()
                        ),
                    ));
                }
            }
            Err(cause) => {
                return Err(storage_error(format!(
                    "cannot create ZIP directory '{}': {cause}",
                    current.display()
                )));
            }
        }
    }
    Ok(())
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

fn storage_error(message: impl Into<String>) -> ArchiveToolError {
    ArchiveToolError::new(ArchiveToolErrorKind::Storage, message)
}

fn archive_error(message: impl Into<String>) -> ArchiveToolError {
    ArchiveToolError::new(ArchiveToolErrorKind::ArchiveInvalid, message)
}

#[cfg(test)]
mod tests;
