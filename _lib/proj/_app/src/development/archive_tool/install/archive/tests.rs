use super::*;
use std::env;
use zip::write::SimpleFileOptions;

#[test]
fn safe_zip_is_validated_and_extracted() {
    let root = env::temp_dir().join(format!("swawkit-archive-zip-{}", std::process::id()));
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
    extract(&archive_path, &root.join("out")).unwrap();

    assert_eq!(
        fs::read(root.join("out/nested/fixture.txt")).unwrap(),
        b"fixture"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn vsix_contents_merge_into_an_existing_staging_directory() {
    let root = env::temp_dir().join(format!("swawkit-vsix-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("out/existing")).unwrap();
    fs::write(root.join("out/existing/first.txt"), b"first").unwrap();
    let archive_path = root.join("fixture.vsix");
    let file = File::create(&archive_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file("extension.vsixmanifest", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"ignored").unwrap();
    writer
        .start_file("Contents/VC/bin/tool.exe", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"tool").unwrap();
    writer.finish().unwrap();
    let file = File::open(&archive_path).unwrap();

    extract_contents_file(&file, &root.join("out")).unwrap();

    assert_eq!(fs::read(root.join("out/VC/bin/tool.exe")).unwrap(), b"tool");
    assert_eq!(
        fs::read(root.join("out/existing/first.txt")).unwrap(),
        b"first"
    );
    assert!(!root.join("out/extension.vsixmanifest").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn vsix_contents_are_strictly_uri_decoded_before_windows_path_validation() {
    let root = env::temp_dir().join(format!("swawkit-vsix-percent-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("out")).unwrap();
    let archive_path = root.join("fixture.vsix");
    let file = File::create(&archive_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file(
            "Contents/DIA%20SDK/bin/tool.dll",
            SimpleFileOptions::default(),
        )
        .unwrap();
    writer.write_all(b"dia").unwrap();
    writer.finish().unwrap();

    extract_contents_file(&File::open(&archive_path).unwrap(), &root.join("out")).unwrap();

    assert_eq!(
        fs::read(root.join(r"out\DIA SDK\bin\tool.dll")).unwrap(),
        b"dia"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_vsix_percent_encoding_is_rejected() {
    let root = env::temp_dir().join(format!("swawkit-vsix-bad-percent-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("out")).unwrap();
    let archive_path = root.join("fixture.vsix");
    let file = File::create(&archive_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file("Contents/bad%2/path.txt", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"bad").unwrap();
    writer.finish().unwrap();

    let error =
        extract_contents_file(&File::open(&archive_path).unwrap(), &root.join("out")).unwrap_err();

    assert!(error.to_string().contains("percent encoding"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn vsix_without_contents_is_rejected() {
    let root = env::temp_dir().join(format!("swawkit-vsix-empty-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("out")).unwrap();
    let archive_path = root.join("fixture.vsix");
    write_single_entry_zip(&archive_path, "extension.vsixmanifest");
    let file = File::open(&archive_path).unwrap();

    let cause = extract_contents_file(&file, &root.join("out")).unwrap_err();

    assert_eq!(cause.kind(), ArchiveToolErrorKind::ArchiveInvalid);
    assert!(cause.to_string().contains("no Contents"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn duplicate_windows_paths_are_rejected() {
    let root = env::temp_dir().join(format!("swawkit-archive-duplicate-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let archive_path = root.join("fixture.zip");
    let file = File::create(&archive_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file("Tool.exe", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"first").unwrap();
    writer
        .start_file("tool.exe", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"second").unwrap();
    writer.finish().unwrap();

    let cause = test(&archive_path).unwrap_err();

    assert_eq!(cause.kind(), ArchiveToolErrorKind::ArchiveInvalid);
    assert!(cause.to_string().contains("duplicate Windows path"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn symbolic_links_are_rejected() {
    let root = env::temp_dir().join(format!("swawkit-archive-symlink-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let archive_path = root.join("fixture.zip");
    let file = File::create(&archive_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .add_symlink("link", "target", SimpleFileOptions::default())
        .unwrap();
    writer.finish().unwrap();

    let cause = test(&archive_path).unwrap_err();

    assert_eq!(cause.kind(), ArchiveToolErrorKind::ArchiveInvalid);
    assert!(cause.to_string().contains("symbolic link"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn windows_ads_reserved_names_and_trailing_dots_are_rejected() {
    let root = env::temp_dir().join(format!(
        "swawkit-archive-windows-name-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    for (sequence, name) in ["pwsh.exe:payload", "CON.txt", "COM1 .dll", "tool."]
        .iter()
        .enumerate()
    {
        let archive_path = root.join(format!("fixture-{sequence}.zip"));
        write_single_entry_zip(&archive_path, name);

        let cause = test(&archive_path).unwrap_err();

        assert_eq!(cause.kind(), ArchiveToolErrorKind::ArchiveInvalid);
        assert!(cause.to_string().contains("unsafe Windows path"));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn superscript_dos_device_names_are_rejected() {
    for name in ["COM\u{00b9}.txt", "LPT\u{00b2}"] {
        let error = windows_entry_path(name).unwrap_err();
        assert!(error.to_string().contains("device name"));
    }
}

#[test]
fn original_noncanonical_zip_paths_are_rejected_instead_of_rewritten() {
    let root = env::temp_dir().join(format!(
        "swawkit-archive-original-path-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    for (sequence, name) in ["a/../b.txt", "/absolute.txt", "a//b.txt", r"a\b.txt"]
        .iter()
        .enumerate()
    {
        let archive_path = root.join(format!("fixture-{sequence}.zip"));
        write_single_entry_zip(&archive_path, name);

        let cause = test(&archive_path).unwrap_err();

        assert_eq!(cause.kind(), ArchiveToolErrorKind::ArchiveInvalid);
        assert!(cause.to_string().contains("unsafe"));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bounded_copy_never_writes_bytes_beyond_the_declared_entry_size() {
    let mut input = std::io::Cursor::new(vec![b'x'; 9]);
    let mut output = Vec::new();
    let mut total = 0;

    let cause = copy_entry_bounded(
        &mut input,
        &mut output,
        8,
        &mut total,
        Path::new("fixture.bin"),
    )
    .unwrap_err();

    assert_eq!(cause.kind(), ArchiveToolErrorKind::ArchiveInvalid);
    assert!(cause.to_string().contains("declared size"));
    assert!(output.len() <= 8);
    assert!(total <= 8);
}

#[test]
fn bounded_copy_enforces_the_runtime_archive_total_before_writing() {
    let mut input = std::io::Cursor::new([b'x']);
    let mut output = Vec::new();
    let mut total = MAX_TOTAL_BYTES;

    let cause = copy_entry_bounded(
        &mut input,
        &mut output,
        1,
        &mut total,
        Path::new("fixture.bin"),
    )
    .unwrap_err();

    assert_eq!(cause.kind(), ArchiveToolErrorKind::ArchiveInvalid);
    assert!(cause.to_string().contains("12 GB"));
    assert!(output.is_empty());
    assert_eq!(total, MAX_TOTAL_BYTES);
}

fn write_single_entry_zip(path: &Path, name: &str) {
    let file = File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    writer
        .start_file(name, SimpleFileOptions::default())
        .unwrap();
    writer.write_all(b"fixture").unwrap();
    writer.finish().unwrap();
}
