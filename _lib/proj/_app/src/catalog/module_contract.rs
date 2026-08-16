use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::profile::EntryLanguage;

use super::{filesystem::directory_files, invalid_data};

mod declaration;
mod validation;

use declaration::{
    LocalizedText, ModuleFacetManifest, ModuleFacetResolverManifest, ModuleManifest,
};
pub(crate) use declaration::{
    ModuleFacet, ModuleFacetArgument, ModuleFacetBinding, ModuleFacetResolver, ModuleSubjectKind,
};
use validation::validate_manifest;

pub const MODULE_CONTRACT_PROTOCOL: &str = "swawkit.command-module/v4";
const MODULE_CONTRACT_FILE: &str = "_module.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandModuleContract {
    pub schema: String,
    pub requires: Vec<ModuleRequirement>,
    pub provides: Vec<ModuleProvision>,
    #[serde(skip)]
    pub(crate) facets: Vec<ModuleFacet>,
    #[serde(skip)]
    pub(crate) subject_kinds: Vec<ModuleSubjectKind>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleRequirement {
    pub provider: String,
    pub contract: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleProvision {
    pub contract: String,
}

pub(super) fn read_local_module_contract(
    command_directory: &Path,
    language: EntryLanguage,
) -> io::Result<Option<CommandModuleContract>> {
    let files = directory_files(command_directory)?;
    let matches = files
        .iter()
        .filter(|file| file.name.eq_ignore_ascii_case(MODULE_CONTRACT_FILE))
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return invalid_data(format!(
            "module contract file name collision below '{}': {}",
            command_directory.display(),
            matches
                .iter()
                .map(|file| file.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let Some(file) = matches.first() else {
        return Ok(None);
    };
    if file.name != MODULE_CONTRACT_FILE {
        return invalid_data(format!(
            "non-canonical module contract file '{}'; expected '{MODULE_CONTRACT_FILE}'",
            file.name
        ));
    }
    if file.reparse_point {
        return invalid_data(format!(
            "module contract file cannot be a reparse point: {}",
            file.path.display()
        ));
    }

    let content = fs::read_to_string(&file.path)?;
    let manifest: ModuleManifest = serde_json::from_str(&content).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "invalid module contract manifest '{}': {error}",
                file.path.display()
            ),
        )
    })?;
    validate_manifest(&manifest, &file.path)?;

    let facets = manifest
        .facets
        .into_iter()
        .map(|facet| localize_facet(facet, language))
        .collect();
    let subject_kinds = manifest
        .subject_kinds
        .into_iter()
        .map(|subject_kind| ModuleSubjectKind {
            kind: subject_kind.kind,
            facets: subject_kind
                .facets
                .into_iter()
                .map(|facet| localize_facet(facet, language))
                .collect(),
        })
        .collect();
    Ok(Some(CommandModuleContract {
        schema: manifest.schema,
        requires: manifest.requires,
        provides: manifest.provides,
        facets,
        subject_kinds,
    }))
}

fn localize_facet(facet: ModuleFacetManifest, language: EntryLanguage) -> ModuleFacet {
    ModuleFacet {
        id: facet.id,
        kind: facet.kind,
        renderer: facet.renderer,
        icon: facet.icon,
        label: localized(facet.label, language),
        summary: localized(facet.summary, language),
        subject_kind: facet.subject_kind,
        resolver: facet.resolver.map(|resolver| match resolver {
            ModuleFacetResolverManifest::Command {
                address,
                arguments,
                accepts_tail,
                confirmation,
                returns,
            } => ModuleFacetResolver::Command {
                address,
                arguments,
                accepts_tail,
                confirmation,
                returns,
            },
        }),
    }
}

fn localized(value: LocalizedText, language: EntryLanguage) -> String {
    match language {
        EntryLanguage::ZhCn => value.zh_cn,
        EntryLanguage::En => value.en,
    }
}
