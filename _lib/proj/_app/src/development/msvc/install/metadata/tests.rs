use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::development::msvc::{MsvcPayload, MsvcRecipe, MsvcStore};

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn writer_and_formal_reader_share_one_metadata_contract() {
    let root = std::env::temp_dir().join(format!(
        "swawkit-msvc-metadata-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let data_root = root.join("data");
    let install = data_root.join("modules/kernel/.dev/setup/export/msvc/installs/17");
    fs::create_dir_all(&install).unwrap();
    let definition = MsvcDefinition::new("17").unwrap();
    let versions = AssemblyVersions {
        tool: "14.44.35228".to_owned(),
        sdk: "10.0.26100.0".to_owned(),
    };
    for (index, relative) in required_paths(&versions.tool, &versions.sdk)
        .into_iter()
        .enumerate()
    {
        let path = install.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("fixture-{index}")).unwrap();
    }
    let recipe = MsvcRecipe::fixture(
        vec![MsvcPayload::fixture("tool.vsix", b"tool")],
        Vec::new(),
        Vec::new(),
    );

    write(&definition, &recipe, &install, &versions).unwrap();
    let installation = MsvcStore::new(&data_root, &definition)
        .read_installation()
        .unwrap();

    assert_eq!(installation.tool_version(), versions.tool);
    assert_eq!(installation.sdk_version(), versions.sdk);
    let _ = fs::remove_dir_all(root);
}
