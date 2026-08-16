use serde::Deserialize;

use crate::{
    facet::{FacetKind, FacetRenderer},
    subject_kind::SubjectKindRef,
};

use super::{ModuleProvision, ModuleRequirement};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModuleFacet {
    pub id: String,
    pub kind: FacetKind,
    pub renderer: FacetRenderer,
    pub icon: String,
    pub label: String,
    pub summary: String,
    pub subject_kind: Option<SubjectKindRef>,
    pub resolver: Option<ModuleFacetResolver>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModuleSubjectKind {
    pub kind: String,
    pub facets: Vec<ModuleFacet>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModuleFacetResolver {
    Command {
        address: String,
        arguments: Vec<ModuleFacetArgument>,
        accepts_tail: bool,
        confirmation: Option<String>,
        returns: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub(crate) enum ModuleFacetArgument {
    Literal(String),
    Binding(ModuleFacetArgumentBinding),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModuleFacetArgumentBinding {
    pub bind: ModuleFacetBinding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ModuleFacetBinding {
    CommandAddress,
    #[serde(rename = "subject.id")]
    SubjectId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ModuleManifest {
    pub(super) schema: String,
    #[serde(default)]
    pub(super) requires: Vec<ModuleRequirement>,
    #[serde(default)]
    pub(super) provides: Vec<ModuleProvision>,
    #[serde(default)]
    pub(super) facets: Vec<ModuleFacetManifest>,
    #[serde(default)]
    pub(super) subject_kinds: Vec<ModuleSubjectKindManifest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ModuleFacetManifest {
    pub(super) id: String,
    pub(super) kind: FacetKind,
    pub(super) renderer: FacetRenderer,
    pub(super) icon: String,
    pub(super) label: LocalizedText,
    pub(super) summary: LocalizedText,
    #[serde(default)]
    pub(super) subject_kind: Option<SubjectKindRef>,
    #[serde(default)]
    pub(super) resolver: Option<ModuleFacetResolverManifest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ModuleSubjectKindManifest {
    pub(super) kind: String,
    pub(super) facets: Vec<ModuleFacetManifest>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub(super) enum ModuleFacetResolverManifest {
    Command {
        address: String,
        #[serde(default)]
        arguments: Vec<ModuleFacetArgument>,
        #[serde(rename = "acceptsTail", default)]
        accepts_tail: bool,
        #[serde(default)]
        confirmation: Option<String>,
        #[serde(default)]
        returns: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LocalizedText {
    #[serde(rename = "zh-CN")]
    pub(super) zh_cn: String,
    pub(super) en: String,
}
