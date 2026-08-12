use std::ffi::OsStr;
use std::path::Path;

use swawkit_proj::development::archive_tool::install::transfer_archive;

use super::{event, path};

pub(crate) fn run(
    controlled_root: &Path,
    source: &OsStr,
    destination: &Path,
    progress_id: &str,
) -> Result<(), String> {
    let destination =
        path::controlled_destination(controlled_root, destination, "download destination")?;
    if destination.exists() {
        return Err(format!(
            "download destination already exists: {}",
            destination.display()
        ));
    }
    let file_name = destination
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("artifact");
    let message = format!("Downloading {file_name}");
    event::progress(progress_id, "running", Some(0), None, &message);

    let result = transfer_archive(source, &destination, &mut |bytes, total| {
        event::progress(progress_id, "running", Some(bytes), total, &message);
    });
    match result {
        Ok(bytes) => {
            event::progress(
                progress_id,
                "completed",
                Some(bytes),
                Some(bytes),
                &format!("Downloaded {file_name}"),
            );
            Ok(())
        }
        Err(error) => {
            event::progress(
                progress_id,
                "failed",
                None,
                None,
                &format!("Download failed: {file_name}"),
            );
            Err(error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs};

    #[test]
    fn local_download_is_published_complete() {
        let root =
            env::temp_dir().join(format!("swawkit-toolchain-download-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("out")).unwrap();
        let source = root.join("source.bin");
        let destination = root.join("out/artifact.bin");
        fs::write(&source, b"fixture").unwrap();

        run(&root, source.as_os_str(), &destination, "download:test").unwrap();

        assert_eq!(fs::read(destination).unwrap(), b"fixture");
        fs::remove_dir_all(root).unwrap();
    }
}
