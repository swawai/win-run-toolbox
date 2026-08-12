use serde_json::{Value, json};

use super::*;

fn payload(file_name: &str, seed: char) -> Value {
    json!({
        "fileName": file_name,
        "sha256": seed.to_string().repeat(64),
        "size": 10,
        "url": format!(
            "https://download.visualstudio.microsoft.com/fixture/{}",
            file_name.replace(' ', "%20").replace('\\', "/")
        ),
    })
}

pub(super) fn channel() -> Value {
    json!({
        "channelItems": [{
            "id": MANIFEST_ID,
            "payloads": [payload("VisualStudio.vsman", 'a')],
        }],
    })
}

pub(super) fn manifest() -> Value {
    let mut packages = Vec::new();
    for (index, template) in TOOL_PACKAGE_TEMPLATES.iter().enumerate() {
        let id = template.replace("{tool}", "14.44.17.14");
        if id.ends_with(".res.base") {
            packages.push(json!({
                "id": id,
                "language": "de-DE",
                "payloads": [payload("tool-de-DE.vsix", 'b')],
            }));
            packages.push(json!({
                "id": id,
                "language": "en-US",
                "payloads": [payload("tool-en-US.vsix", 'c')],
            }));
        } else {
            packages.push(json!({
                "id": id,
                "payloads": [payload(&format!("tool-{index}.vsix"), 'd')],
            }));
        }
    }
    packages.push(json!({
        "id": "microsoft.vc.14.43.1.1.tools.hostx64.targetx64.base",
        "payloads": [payload("old.vsix", 'e')],
    }));
    packages.push(json!({
        "id": "Microsoft.VisualStudio.Component.Windows10SDK.22621",
        "dependencies": {"Win10SDK_10": "[10,11)"},
    }));
    packages.push(json!({
        "id": "Microsoft.VisualStudio.Component.Windows11SDK.26100",
        "dependencies": {"Win11SDK_10": "[10,11)"},
    }));
    let sdk_payloads = SDK_MSI_NAMES
        .iter()
        .enumerate()
        .map(|(index, name)| {
            payload(
                &format!("Installers\\{name}"),
                char::from(b'a' + (index % 6) as u8),
            )
        })
        .collect::<Vec<_>>();
    packages.push(json!({
        "id": "Win11SDK_10",
        "payloads": sdk_payloads,
    }));
    json!({"packages": packages})
}

fn resolve(channel: &Value, manifest: &Value) -> Result<MsvcRecipe, MsvcError> {
    resolve_recipe(
        &MsvcDefinition::new("17").unwrap(),
        &serde_json::to_vec(channel).unwrap(),
        &serde_json::to_vec(manifest).unwrap(),
    )
}

#[test]
fn exact_packages_language_and_newest_versions_form_one_recipe() {
    let recipe = resolve(&channel(), &manifest()).unwrap();

    assert_eq!(recipe.channel(), "17");
    assert_eq!(
        recipe.definition_signature(),
        "4597e8b291e1b50b7bec65f02b99df7dac9ef2e495d1931c6e0ea9f34b31d5a8"
    );
    assert_eq!(recipe.tool_package_version(), "14.44.17.14");
    assert_eq!(recipe.sdk_package(), "Win11SDK_10");
    assert_eq!(recipe.manifest_sha256().len(), 64);
    assert_eq!(recipe.tool_payloads().len(), 7);
    assert_eq!(recipe.msi_payloads().len(), 8);
    assert!(
        recipe
            .tool_payloads()
            .iter()
            .any(|payload| payload.leaf_name() == "tool-en-US.vsix")
    );
    assert!(
        recipe
            .tool_payloads()
            .iter()
            .all(|payload| payload.leaf_name() != "tool-de-DE.vsix")
    );
}

#[test]
fn channel_requires_one_exact_manifest_payload() {
    let mut missing = channel();
    missing["channelItems"][0]["id"] = json!("wrong");
    assert_eq!(
        resolve(&missing, &manifest()).unwrap_err().kind(),
        MsvcErrorKind::InvalidSource
    );

    let mut duplicate = channel();
    let duplicate_item = duplicate["channelItems"][0].clone();
    duplicate["channelItems"]
        .as_array_mut()
        .unwrap()
        .push(duplicate_item);
    assert!(resolve(&duplicate, &manifest()).is_err());
}

#[test]
fn payloads_require_hash_size_safe_leaf_and_the_microsoft_host() {
    let cases = [
        json!({"fileName":"a.vsix","sha256":"","size":10,"url":"https://download.visualstudio.microsoft.com/a"}),
        json!({"fileName":"a.vsix","sha256":"a".repeat(64),"size":0,"url":"https://download.visualstudio.microsoft.com/a"}),
        json!({"fileName":"..","sha256":"a".repeat(64),"size":10,"url":"https://download.visualstudio.microsoft.com/a"}),
        json!({"fileName":"NUL.txt","sha256":"a".repeat(64),"size":10,"url":"https://download.visualstudio.microsoft.com/a"}),
        json!({"fileName":" a.vsix","sha256":"a".repeat(64),"size":10,"url":"https://download.visualstudio.microsoft.com/a"}),
        json!({"fileName":"a.vsix","sha256":"a".repeat(64),"size":10,"url":" https://download.visualstudio.microsoft.com/a"}),
        json!({"fileName":"a.vsix","sha256":"a".repeat(64),"size":10,"url":"https://example.test/a"}),
        json!({"fileName":"a.vsix","sha256":"a".repeat(64),"size":10,"url":"http://download.visualstudio.microsoft.com/a"}),
    ];
    for raw in cases {
        let mut invalid_channel = channel();
        invalid_channel["channelItems"][0]["payloads"][0] = raw;
        assert!(resolve(&invalid_channel, &manifest()).is_err());
    }
}

#[test]
fn ambiguous_packages_sdk_dependencies_and_msi_payloads_are_rejected() {
    let mut duplicate_tool = manifest();
    let first = duplicate_tool["packages"][0].clone();
    duplicate_tool["packages"]
        .as_array_mut()
        .unwrap()
        .push(first);
    assert!(resolve(&channel(), &duplicate_tool).is_err());

    let mut dependencies = manifest();
    let component = dependencies["packages"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|package| package["id"] == "Microsoft.VisualStudio.Component.Windows11SDK.26100")
        .unwrap();
    component["dependencies"]["Win10SDK_extra"] = json!("[10,11)");
    assert!(resolve(&channel(), &dependencies).is_err());

    let mut missing_msi = manifest();
    let sdk = missing_msi["packages"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|package| package["id"] == "Win11SDK_10")
        .unwrap();
    sdk["payloads"].as_array_mut().unwrap().pop();
    assert!(resolve(&channel(), &missing_msi).is_err());
}
