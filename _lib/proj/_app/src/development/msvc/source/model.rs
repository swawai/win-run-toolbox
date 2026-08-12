#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use sha2::{Digest, Sha256};

#[cfg(test)]
use super::MsvcDefinition;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MsvcPayload {
    file_name: String,
    leaf_name: String,
    sha256: String,
    size: Option<u64>,
    url: String,
}

impl MsvcPayload {
    pub(super) fn new(
        file_name: String,
        leaf_name: String,
        sha256: String,
        size: Option<u64>,
        url: String,
    ) -> Self {
        Self {
            file_name,
            leaf_name,
            sha256,
            size,
            url,
        }
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub fn leaf_name(&self) -> &str {
        &self.leaf_name
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn size(&self) -> Option<u64> {
        self.size
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    #[cfg(test)]
    pub(crate) fn fixture(file_name: &str, content: &[u8]) -> Self {
        Self {
            file_name: file_name.to_owned(),
            leaf_name: Path::new(file_name)
                .file_name()
                .expect("fixture payload has a file name")
                .to_string_lossy()
                .into_owned(),
            sha256: format!("{:x}", Sha256::digest(content)),
            size: Some(content.len() as u64),
            url: format!("https://download.visualstudio.microsoft.com/fixture/{file_name}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MsvcRecipe {
    pub(super) channel: String,
    pub(super) definition_signature: String,
    pub(super) manifest: MsvcPayload,
    pub(super) manifest_sha256: String,
    pub(super) tool_package_version: String,
    pub(super) tool_payloads: Vec<MsvcPayload>,
    pub(super) sdk_package: String,
    pub(super) sdk_payloads: Vec<MsvcPayload>,
    pub(super) msi_payloads: Vec<MsvcPayload>,
}

impl MsvcRecipe {
    pub fn channel(&self) -> &str {
        &self.channel
    }

    pub fn definition_signature(&self) -> &str {
        &self.definition_signature
    }

    pub fn manifest(&self) -> &MsvcPayload {
        &self.manifest
    }

    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    pub fn tool_package_version(&self) -> &str {
        &self.tool_package_version
    }

    pub fn tool_payloads(&self) -> &[MsvcPayload] {
        &self.tool_payloads
    }

    pub fn sdk_package(&self) -> &str {
        &self.sdk_package
    }

    pub fn sdk_payloads(&self) -> &[MsvcPayload] {
        &self.sdk_payloads
    }

    pub fn msi_payloads(&self) -> &[MsvcPayload] {
        &self.msi_payloads
    }

    #[cfg(test)]
    pub(crate) fn fixture(
        tool_payloads: Vec<MsvcPayload>,
        sdk_payloads: Vec<MsvcPayload>,
        msi_payloads: Vec<MsvcPayload>,
    ) -> Self {
        let manifest = MsvcPayload::fixture("VisualStudio.vsman", b"fixture manifest");
        Self {
            channel: "17".to_owned(),
            definition_signature: MsvcDefinition::new("17").unwrap().definition_signature(),
            manifest,
            manifest_sha256: format!("{:x}", Sha256::digest(b"fixture manifest")),
            tool_package_version: "14.44.17.14".to_owned(),
            tool_payloads,
            sdk_package: "Win11SDK_fixture".to_owned(),
            sdk_payloads,
            msi_payloads,
        }
    }
}
