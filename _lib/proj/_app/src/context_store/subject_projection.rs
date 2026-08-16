use crate::{
    catalog::{CatalogSnapshot, CommandSource},
    profile::EntryLanguage,
    subject::{SUBJECT_COLLECTION_PROTOCOL, SubjectCollection, SubjectRef, SubjectSummary},
};

use super::{ContextRecord, ContextResult, ContextStore, ContextStoreError};

pub const CONTEXT_OWNER_ADDRESS: &str = ".context";
pub const CONTEXT_COLLECTION_FACET: &str = "contexts";
pub const CONTEXT_KIND: &str = "context";

pub fn project_context_collection(
    catalog: &CatalogSnapshot,
    language: EntryLanguage,
    store: &ContextStore,
) -> ContextResult<SubjectCollection> {
    let facet_ids = context_facet_ids(catalog)?;
    let subjects = store
        .list()?
        .into_iter()
        .map(|record| SubjectSummary {
            reference: SubjectRef::Instance {
                kind: CONTEXT_KIND.to_owned(),
                id: record.id.clone(),
            },
            label: format!("::{}/{}", CONTEXT_KIND, record.id),
            summary: context_summary(language, &record),
            facet_ids: facet_ids.clone(),
        })
        .collect();
    Ok(SubjectCollection {
        protocol: SUBJECT_COLLECTION_PROTOCOL.to_owned(),
        owner: SubjectRef::Command {
            source: CommandSource::Kernel,
            address: CONTEXT_OWNER_ADDRESS.to_owned(),
        },
        facet: CONTEXT_COLLECTION_FACET.to_owned(),
        subjects,
    })
}

fn context_facet_ids(catalog: &CatalogSnapshot) -> ContextResult<Vec<String>> {
    let owner = catalog
        .commands
        .iter()
        .find(|command| {
            command.source == CommandSource::Kernel
                && command.address == CONTEXT_OWNER_ADDRESS
                && command.alias_of.is_none()
        })
        .ok_or_else(|| ContextStoreError::new("Context owner command is unavailable"))?;
    let subject_kind = owner
        .subject_kinds
        .iter()
        .find(|subject_kind| subject_kind.kind == CONTEXT_KIND)
        .ok_or_else(|| ContextStoreError::new("Context Subject kind is unavailable"))?;
    Ok(subject_kind
        .facets
        .iter()
        .map(|facet| facet.id.clone())
        .collect())
}

fn context_summary(language: EntryLanguage, record: &ContextRecord) -> String {
    match language {
        EntryLanguage::ZhCn => format!(
            "{} 个命令 · {} 条说明",
            record.commands.len(),
            record.notes.len()
        ),
        EntryLanguage::En => format!(
            "{} commands · {} notes",
            record.commands.len(),
            record.notes.len()
        ),
    }
}
