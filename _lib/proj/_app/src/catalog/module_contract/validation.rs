use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use crate::{
    facet::{FacetKind, FacetRenderer},
    subject::SUBJECT_COLLECTION_PROTOCOL,
};

use super::{
    MODULE_CONTRACT_PROTOCOL,
    declaration::{
        ModuleFacetArgument, ModuleFacetBinding, ModuleFacetManifest, ModuleFacetResolverManifest,
        ModuleManifest,
    },
};
use crate::catalog::invalid_data;

mod identity;

use identity::{
    valid_provider_address, valid_token, validate_contract, validate_localized_text, validate_text,
};

const MAX_FACETS: usize = 16;
const MAX_SUBJECT_KINDS: usize = 8;
const MAX_ARGUMENTS: usize = 32;
const MAX_ARGUMENT_LENGTH: usize = 4096;

pub(super) fn validate_manifest(manifest: &ModuleManifest, path: &Path) -> io::Result<()> {
    if manifest.schema != MODULE_CONTRACT_PROTOCOL {
        return invalid_data(format!(
            "unsupported module contract schema '{}' in '{}'",
            manifest.schema,
            path.display()
        ));
    }
    if manifest.requires.is_empty()
        && manifest.provides.is_empty()
        && manifest.facets.is_empty()
        && manifest.subject_kinds.is_empty()
    {
        return invalid_data(format!(
            "module contract manifest must declare requires, provides, facets, or subjectKinds: {}",
            path.display()
        ));
    }
    validate_requirements(manifest, path)?;
    validate_provisions(manifest, path)?;
    validate_subject_kinds(manifest, path)?;
    validate_facets(&manifest.facets, FacetScope::Command, path)?;

    Ok(())
}

fn validate_requirements(manifest: &ModuleManifest, path: &Path) -> io::Result<()> {
    let mut seen = BTreeSet::new();
    for requirement in &manifest.requires {
        if !valid_provider_address(&requirement.provider) {
            return invalid_data(format!(
                "invalid module provider address '{}' in '{}'",
                requirement.provider,
                path.display()
            ));
        }
        validate_contract(&requirement.contract, path)?;
        if !seen.insert((&requirement.provider, &requirement.contract)) {
            return invalid_data(format!(
                "duplicate module requirement '{} -> {}' in '{}'",
                requirement.provider,
                requirement.contract,
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_provisions(manifest: &ModuleManifest, path: &Path) -> io::Result<()> {
    let mut seen = BTreeSet::new();
    for provision in &manifest.provides {
        validate_contract(&provision.contract, path)?;
        if !seen.insert(&provision.contract) {
            return invalid_data(format!(
                "duplicate module provision '{}' in '{}'",
                provision.contract,
                path.display()
            ));
        }
    }
    Ok(())
}

fn validate_subject_kinds(manifest: &ModuleManifest, path: &Path) -> io::Result<()> {
    if manifest.subject_kinds.len() > MAX_SUBJECT_KINDS {
        return invalid_data(format!(
            "module subjectKinds in '{}' cannot contain more than {MAX_SUBJECT_KINDS} items",
            path.display()
        ));
    }
    let mut seen = BTreeSet::new();
    for subject_kind in &manifest.subject_kinds {
        if !valid_token(&subject_kind.kind) || !seen.insert(subject_kind.kind.as_str()) {
            return invalid_data(format!(
                "module subject kind '{}' in '{}' must be unique and match [a-z][a-z0-9-]{{0,31}}",
                subject_kind.kind,
                path.display()
            ));
        }
        if subject_kind.facets.is_empty() {
            return invalid_data(format!(
                "module subject kind '{}' in '{}' must declare at least one facet",
                subject_kind.kind,
                path.display()
            ));
        }
        validate_facets(&subject_kind.facets, FacetScope::Subject, path)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum FacetScope {
    Command,
    Subject,
}

fn validate_facets(
    facets: &[ModuleFacetManifest],
    scope: FacetScope,
    path: &Path,
) -> io::Result<()> {
    if facets.len() > MAX_FACETS {
        return invalid_data(format!(
            "module facets in '{}' cannot contain more than {MAX_FACETS} items",
            path.display()
        ));
    }
    let mut identifiers = BTreeSet::new();
    for facet in facets {
        if !valid_token(&facet.id) || !identifiers.insert(facet.id.as_str()) {
            return invalid_data(format!(
                "module facet id '{}' in '{}' must be unique and match [a-z][a-z0-9-]{{0,31}}",
                facet.id,
                path.display()
            ));
        }
        validate_text(&facet.icon, 8, "facet icon", path)?;
        validate_localized_text(&facet.label, 64, "facet label", path)?;
        validate_localized_text(&facet.summary, 200, "facet summary", path)?;
        validate_facet_shape(facet, scope, path)?;
    }
    Ok(())
}

fn validate_facet_shape(
    facet: &ModuleFacetManifest,
    scope: FacetScope,
    path: &Path,
) -> io::Result<()> {
    let renderer_matches = matches!(
        (facet.kind, facet.renderer),
        (FacetKind::Collection, FacetRenderer::Collection)
            | (FacetKind::Operation, FacetRenderer::Run)
            | (FacetKind::Projection, FacetRenderer::Overview)
    );
    if !renderer_matches {
        return invalid_data(format!(
            "module facet '{}' in '{}' has an incompatible kind and renderer",
            facet.id,
            path.display()
        ));
    }
    match scope {
        FacetScope::Command if facet.kind == FacetKind::Collection => {
            if !facet
                .subject_kind
                .as_ref()
                .is_some_and(|subject_kind| subject_kind.validate().is_ok())
            {
                return invalid_data(format!(
                    "collection facet '{}' in '{}' must declare one valid subjectKind reference",
                    facet.id,
                    path.display()
                ));
            }
        }
        FacetScope::Command if facet.subject_kind.is_some() => {
            return invalid_data(format!(
                "only a collection facet may declare subjectKind in '{}'",
                path.display()
            ));
        }
        FacetScope::Subject if facet.subject_kind.is_some() => {
            return invalid_data(format!(
                "subject facet '{}' in '{}' cannot declare subjectKind",
                facet.id,
                path.display()
            ));
        }
        FacetScope::Subject if facet.kind == FacetKind::Collection => {
            return invalid_data(format!(
                "subject facet '{}' in '{}' cannot expose a nested collection",
                facet.id,
                path.display()
            ));
        }
        _ => {}
    }

    let Some(ModuleFacetResolverManifest::Command {
        address,
        arguments,
        accepts_tail,
        confirmation,
        returns,
    }) = &facet.resolver
    else {
        return invalid_data(format!(
            "module facet '{}' in '{}' must declare a command resolver",
            facet.id,
            path.display()
        ));
    };
    validate_resolver(
        facet,
        scope,
        address,
        arguments,
        *accepts_tail,
        confirmation.as_deref(),
        returns.as_deref(),
        path,
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_resolver(
    facet: &ModuleFacetManifest,
    scope: FacetScope,
    address: &str,
    arguments: &[ModuleFacetArgument],
    accepts_tail: bool,
    confirmation: Option<&str>,
    returns: Option<&str>,
    path: &Path,
) -> io::Result<()> {
    if !valid_provider_address(address) {
        return invalid_data(format!(
            "invalid module facet command address '{}' in '{}'",
            address,
            path.display()
        ));
    }
    if arguments.len() > MAX_ARGUMENTS
        || arguments.iter().any(|argument| {
            matches!(argument, ModuleFacetArgument::Literal(value) if value.len() > MAX_ARGUMENT_LENGTH || value.contains('\0'))
        })
    {
        return invalid_data(format!(
            "module facet '{}' in '{}' exceeds the argument limits",
            facet.id,
            path.display()
        ));
    }
    let valid_binding = |binding: ModuleFacetBinding| match scope {
        FacetScope::Command => binding == ModuleFacetBinding::CommandAddress,
        FacetScope::Subject => binding == ModuleFacetBinding::SubjectId,
    };
    if arguments.iter().any(|argument| {
        matches!(argument, ModuleFacetArgument::Binding(binding) if !valid_binding(binding.bind))
    }) {
        return invalid_data(format!(
            "module facet '{}' in '{}' uses a binding outside its Subject scope",
            facet.id,
            path.display()
        ));
    }
    if confirmation.is_some_and(|value| {
        value.trim() != value || value.is_empty() || value.chars().count() > 500
    }) {
        return invalid_data(format!(
            "module facet '{}' confirmation in '{}' must contain 1 to 500 trimmed characters",
            facet.id,
            path.display()
        ));
    }
    if accepts_tail && confirmation.is_some() {
        return invalid_data(format!(
            "module facet '{}' in '{}' cannot combine tail arguments with confirmation",
            facet.id,
            path.display()
        ));
    }
    match facet.kind {
        FacetKind::Collection if returns != Some(SUBJECT_COLLECTION_PROTOCOL) => {
            return invalid_data(format!(
                "collection facet '{}' in '{}' must return {SUBJECT_COLLECTION_PROTOCOL}",
                facet.id,
                path.display()
            ));
        }
        FacetKind::Projection
            if returns.is_none() || returns == Some(SUBJECT_COLLECTION_PROTOCOL) =>
        {
            return invalid_data(format!(
                "projection facet '{}' in '{}' must declare a non-collection returned protocol",
                facet.id,
                path.display()
            ));
        }
        FacetKind::Operation if returns.is_some() => {
            return invalid_data(format!(
                "operation facet '{}' in '{}' cannot declare a returned protocol",
                facet.id,
                path.display()
            ));
        }
        FacetKind::Collection | FacetKind::Projection if accepts_tail || confirmation.is_some() => {
            return invalid_data(format!(
                "document facet '{}' in '{}' must use exact arguments without confirmation",
                facet.id,
                path.display()
            ));
        }
        _ => {}
    }
    if let Some(protocol) = returns {
        validate_text(protocol, 128, "facet returned protocol", path)?;
    }
    Ok(())
}
