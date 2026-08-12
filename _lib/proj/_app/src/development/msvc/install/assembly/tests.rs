use super::*;
use std::fs;

#[test]
fn setup_script_matches_the_power_shell_contract() {
    let content = setup_script(&AssemblyVersions {
        tool: "14.44.35228".to_owned(),
        sdk: "10.0.26100.0".to_owned(),
    });
    assert!(content.starts_with("@echo off\r\n"));
    assert!(content.contains("set \"VCToolsVersion=14.44.35228\"\r\n"));
    assert!(content.contains("Windows Kits\\10\\bin\\10.0.26100.0\\x64"));
    assert!(content.ends_with("\r\n"));
}

#[test]
fn a_missing_optional_descendant_never_broadens_to_an_existing_ancestor() {
    let root =
        std::env::temp_dir().join(format!("swawkit-msvc-assembly-path-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(r"VC\Tools")).unwrap();

    let result = checked_child(&root, r"VC\Tools\MSVC\14.44\bin\Hostx86", true).unwrap();

    assert_eq!(result, root.join(r"VC\Tools\MSVC\14.44\bin\Hostx86"));
    assert!(root.join(r"VC\Tools").is_dir());
    let _ = fs::remove_dir_all(root);
}
