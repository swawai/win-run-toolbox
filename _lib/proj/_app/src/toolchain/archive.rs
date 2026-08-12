use std::fs;
use std::path::Path;

use swawkit_proj::development::archive_tool::install::{extract_archive, test_archive};

use super::path;

pub(crate) fn test(archive: &Path) -> Result<(), String> {
    let archive = path::regular_file(archive, "ZIP archive")?;
    test_archive(&archive).map_err(|error| error.to_string())
}

pub(crate) fn extract(
    controlled_root: &Path,
    archive: &Path,
    destination: &Path,
) -> Result<(), String> {
    let archive = path::regular_file(archive, "ZIP archive")?;
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
    extract_archive(&archive, &destination).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs::File;
    use std::io::Write;
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
