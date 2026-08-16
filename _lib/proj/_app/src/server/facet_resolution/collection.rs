use axum::{Json, http::StatusCode};

use crate::{
    catalog::CatalogSnapshot,
    facet::Facet,
    subject::{SubjectCollection, SubjectRef},
    subject_kind::SubjectKind,
};

use super::super::{ApiError, api_error};

type ApiResult<T> = Result<T, (StatusCode, Json<ApiError>)>;

pub(super) fn validate_collection_contract(
    catalog: &CatalogSnapshot,
    collection: &SubjectCollection,
    collection_facet: &Facet,
) -> ApiResult<()> {
    let SubjectRef::Command { .. } = &collection.owner else {
        unreachable!("SubjectCollection v2 validation requires a command owner");
    };
    let subject_kind = collection_subject_kind(catalog, collection_facet)?;
    for subject in &collection.subjects {
        let SubjectRef::Instance { kind, .. } = &subject.reference else {
            unreachable!("SubjectCollection v2 validation requires instance items");
        };
        if kind != &subject_kind.kind
            || subject
                .facet_ids
                .iter()
                .any(|id| !subject_kind.facets.iter().any(|facet| &facet.id == id))
        {
            return Err(api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "facet resolver command returned an invalid Subject collection",
            ));
        }
    }
    Ok(())
}

pub(super) fn collection_subject_kind<'a>(
    catalog: &'a CatalogSnapshot,
    collection_facet: &Facet,
) -> ApiResult<&'a SubjectKind> {
    let reference = collection_facet.subject_kind.as_ref().ok_or_else(|| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "collection facet has no Subject kind",
        )
    })?;
    let SubjectRef::Command { source, address } = &reference.provider else {
        unreachable!("Facet validation requires a command Subject kind provider");
    };
    let command = catalog
        .commands
        .iter()
        .find(|command| {
            command.source == *source && command.address == *address && command.alias_of.is_none()
        })
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "Subject not found"))?;
    command
        .subject_kinds
        .iter()
        .find(|subject_kind| subject_kind.kind == reference.kind)
        .ok_or_else(|| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "collection Subject kind is unavailable",
            )
        })
}
