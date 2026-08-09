use std::ffi::OsString;

use crate::catalog::CatalogSnapshot;

use super::{CommandError, CommandResult, ResolvedCommand};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invocation {
    pub command: ResolvedCommand,
    pub arguments: Vec<OsString>,
}

impl Invocation {
    pub(crate) fn resolve(snapshot: &CatalogSnapshot, argv: &[OsString]) -> CommandResult<Self> {
        let address = argv
            .first()
            .map(|value| {
                value
                    .to_str()
                    .ok_or_else(|| CommandError::new("command address is not valid Unicode"))
            })
            .transpose()?
            .unwrap_or("");
        let raw_arguments = argv.get(1..).unwrap_or_default();

        Ok(Self {
            command: ResolvedCommand::from_catalog(snapshot, address)?,
            arguments: raw_arguments.to_vec(),
        })
    }
}
