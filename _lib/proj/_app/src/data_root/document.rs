use serde::Serialize;

use super::DataRootClaim;

pub const DATA_ROOT_CLAIM_PROTOCOL: &str = "swawkit.data-root-claim/v2";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRootClaimDocument {
    protocol: &'static str,
    status: &'static str,
    claim: Option<DataRootClaimView>,
}

impl DataRootClaimDocument {
    pub fn inspect(claim: Option<&DataRootClaim>) -> Self {
        Self {
            protocol: DATA_ROOT_CLAIM_PROTOCOL,
            status: if claim.is_some() {
                "claimRequired"
            } else {
                "notRequired"
            },
            claim: claim.map(DataRootClaimView::from),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DataRootClaimView {
    kind: &'static str,
    entry_name: String,
    entry_file: String,
    volume_id: String,
    file_id: String,
    data_root: String,
    source_data_root: Option<String>,
    reason: String,
}

impl From<&DataRootClaim> for DataRootClaimView {
    fn from(claim: &DataRootClaim) -> Self {
        Self {
            kind: claim.kind.as_str(),
            entry_name: claim.entry_name.clone(),
            entry_file: claim.entry_file.to_string_lossy().into_owned(),
            volume_id: claim.volume_id.clone(),
            file_id: claim.file_id.clone(),
            data_root: claim.data_root.to_string_lossy().into_owned(),
            source_data_root: claim
                .source_data_root
                .as_deref()
                .map(|path| path.to_string_lossy().into_owned()),
            reason: claim.reason.clone(),
        }
    }
}
