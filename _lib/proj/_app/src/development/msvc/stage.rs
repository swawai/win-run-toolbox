use std::path::Path;

use crate::development::archive_tool::install::extract_vsix_contents;

use super::{MsvcError, VerifiedMsvcPayload};

pub struct MsvcStager;

impl MsvcStager {
    pub fn expand_vsix(payload: &VerifiedMsvcPayload, destination: &Path) -> Result<(), MsvcError> {
        let file = payload.try_clone()?;
        extract_vsix_contents(&file, destination).map_err(MsvcError::from)
    }
}
