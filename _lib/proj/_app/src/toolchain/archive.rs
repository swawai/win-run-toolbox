use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use super::path;

const MAX_ENTRIES: usize = 200_000;
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 12 * 1024 * 1024 * 1024;

pub(crate) fn test(archive: &Path) -> Result<(), String> {
    let archive = path::regular_file(archive, "ZIP archive")?;
    let file = File::open(&archive)
        .map_err(|error| format!("cannot open ZIP archive '{}': {error}", archive.display()))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("invalid ZIP archive: {error}"))?;
    inspect(&mut archive).map(|_| ())
}

pub(crate) fn extract(
    controlled_root: &Path,
    archive: &Path,
    destination: &Path,
) -> Result<(), String> {
    let archive_path = path::regular_file(archive, "ZIP archive")?;
    let destination =
        path::controlled_destination(controlled_root, destination, "ZIP destination")?;
    path::validate_directory(&destination, "ZIP destination")?;
    if fs::read_dir(&destination)
        .map_err(|error| format!("cannot inspect ZIP destination: {error}"))?
        .next()
        .is_some()
    {
        return Err(format!(
            "ZIP destination must be empty: {}",
            destination.display()
        ));
    }
    let file = File::open(&archive_path).map_err(|error| {
        format!(
            "cannot open ZIP archive '{}': {error}",
            archive_path.display()
        )
    })?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("invalid ZIP archive: {error}"))?;
    let entries = inspect(&mut archive)?;
    for index in 0..entries.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("cannot read ZIP entry {index}: {error}"))?;
        let relative = &entries[index];
        let target = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|error| {
                format!(
                    "cannot create ZIP directory '{}': {error}",
                    target.display()
                )
            })?;
            continue;
        }
        let parent = target
            .parent()
            .ok_or_else(|| "ZIP entry has no parent".to_owned())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create ZIP parent '{}': {error}", parent.display()))?;
        let mut output = File::create(&target)
            .map_err(|error| format!("cannot create ZIP file '{}': {error}", target.display()))?;
        let copied = std::io::copy(&mut entry, &mut output)
            .map_err(|error| format!("cannot extract ZIP file '{}': {error}", target.display()))?;
        if copied != entry.size() {
            return Err(format!(
                "ZIP entry extracted incompletely: {}",
                relative.display()
            ));
        }
        output
            .flush()
            .map_err(|error| format!("cannot flush ZIP file: {error}"))?;
    }
    Ok(())
}

fn inspect<R: Read + std::io::Seek>(archive: &mut ZipArchive<R>) -> Result<Vec<PathBuf>, String> {
    if archive.len() > MAX_ENTRIES {
        return Err(format!(
            "ZIP archive contains more than {MAX_ENTRIES} entries"
        ));
    }
    let mut total = 0u64;
    let mut names = HashSet::new();
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("cannot inspect ZIP entry {index}: {error}"))?;
        let relative = entry
            .enclosed_name()
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| format!("ZIP entry has an unsafe path: {}", entry.name()))?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(format!(
                "ZIP entry cannot be a symbolic link: {}",
                entry.name()
            ));
        }
        if entry.size() > MAX_FILE_BYTES {
            return Err(format!(
                "ZIP entry exceeds the 4 GB safety limit: {}",
                entry.name()
            ));
        }
        total = total
            .checked_add(entry.size())
            .filter(|value| *value <= MAX_TOTAL_BYTES)
            .ok_or_else(|| "ZIP archive exceeds the 12 GB safety limit".to_owned())?;
        let key = relative.to_string_lossy().to_lowercase();
        if !names.insert(key) {
            return Err(format!(
                "ZIP archive contains a duplicate Windows path: {}",
                relative.display()
            ));
        }
        entries.push(relative);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use zip::write::SimpleFileOptions;

    #[test]
    fn safe_zip_is_validated_and_extracted() {
        let root = env::temp_dir().join(format!("swawkit-toolchain-zip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("out")).unwrap();
        let archive_path = root.join("fixture.zip");
        let file = File::create(&archive_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("nested/fixture.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"fixture").unwrap();
        writer.finish().unwrap();

        test(&archive_path).unwrap();
        extract(&root, &archive_path, &root.join("out")).unwrap();

        assert_eq!(
            fs::read(root.join("out/nested/fixture.txt")).unwrap(),
            b"fixture"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
