use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    facet::{Facet, FacetKind, FacetRenderer, FacetResolver},
    subject::{SubjectRef, valid_instance_id, valid_token, validate_subject_ref},
};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubjectKindRef {
    pub kind: String,
    pub provider: SubjectRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubjectKind {
    pub kind: String,
    pub facets: Vec<SubjectFacetTemplate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubjectFacetTemplate {
    pub id: String,
    pub kind: FacetKind,
    pub renderer: FacetRenderer,
    pub icon: String,
    pub label: String,
    pub summary: String,
    pub resolver: SubjectFacetResolver,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum SubjectFacetResolver {
    Command {
        address: String,
        arguments: Vec<SubjectFacetArgument>,
        #[serde(rename = "acceptsTail", default, skip_serializing_if = "is_false")]
        accepts_tail: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confirmation: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        returns: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum SubjectFacetArgument {
    Literal(String),
    Binding(SubjectFacetArgumentBinding),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubjectFacetArgumentBinding {
    pub bind: SubjectFacetBinding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum SubjectFacetBinding {
    #[serde(rename = "subject.id")]
    SubjectId,
}

impl SubjectKind {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_token(&self.kind) {
            return Err("Subject kind must match [a-z][a-z0-9-]{0,31}".to_owned());
        }
        if self.facets.is_empty() || self.facets.len() > 32 {
            return Err("Subject kind must declare 1 to 32 facet templates".to_owned());
        }
        let mut identifiers = BTreeSet::new();
        for facet in &self.facets {
            if !identifiers.insert(facet.id.as_str()) {
                return Err("Subject kind contains a duplicate facet id".to_owned());
            }
            facet.validate()?;
        }
        Ok(())
    }

    pub fn instantiate(&self, facet_id: &str, subject_id: &str) -> Result<Option<Facet>, String> {
        if !valid_instance_id(subject_id) {
            return Err("Subject instance id is invalid".to_owned());
        }
        self.facets
            .iter()
            .find(|facet| facet.id == facet_id)
            .map(|facet| facet.instantiate(subject_id))
            .transpose()
    }
}

impl SubjectKindRef {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_token(&self.kind) {
            return Err("Subject kind ref must match [a-z][a-z0-9-]{0,31}".to_owned());
        }
        if !matches!(self.provider, SubjectRef::Command { .. }) {
            return Err("Subject kind provider must be a command Subject".to_owned());
        }
        validate_subject_ref(&self.provider)
    }
}

impl SubjectFacetTemplate {
    fn validate(&self) -> Result<(), String> {
        let facet = self.instantiate("subject")?;
        facet.validate()?;
        if facet.kind == FacetKind::Collection {
            return Err("Subject facet templates cannot expose nested collections".to_owned());
        }
        if facet.kind == FacetKind::Operation && facet.renderer != FacetRenderer::Run {
            return Err("Subject operation templates must use the run renderer".to_owned());
        }
        Ok(())
    }

    fn instantiate(&self, subject_id: &str) -> Result<Facet, String> {
        let resolver = match &self.resolver {
            SubjectFacetResolver::Command {
                address,
                arguments,
                accepts_tail,
                confirmation,
                returns,
            } => FacetResolver::Command {
                address: address.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| match argument {
                        SubjectFacetArgument::Literal(value) => value.clone(),
                        SubjectFacetArgument::Binding(binding) => match binding.bind {
                            SubjectFacetBinding::SubjectId => subject_id.to_owned(),
                        },
                    })
                    .collect(),
                accepts_tail: *accepts_tail,
                confirmation: confirmation.clone(),
                returns: returns.clone(),
            },
        };
        Ok(Facet {
            id: self.id.clone(),
            kind: self.kind,
            renderer: self.renderer,
            icon: self.icon.clone(),
            label: self.label.clone(),
            summary: self.summary.clone(),
            subject_kind: None,
            resolver: Some(resolver),
        })
    }
}

fn is_false(value: &bool) -> bool {
    !value
}

#[cfg(test)]
mod tests {
    use crate::{catalog::CommandSource, subject::SubjectRef};

    use super::*;

    #[test]
    fn typed_subject_id_binding_materializes_one_concrete_facet() {
        let kind = SubjectKind {
            kind: "context".to_owned(),
            facets: vec![SubjectFacetTemplate {
                id: "show".to_owned(),
                kind: FacetKind::Projection,
                renderer: FacetRenderer::Overview,
                icon: "i".to_owned(),
                label: "Overview".to_owned(),
                summary: "Inspect this Context".to_owned(),
                resolver: SubjectFacetResolver::Command {
                    address: ".context.show".to_owned(),
                    arguments: vec![SubjectFacetArgument::Binding(SubjectFacetArgumentBinding {
                        bind: SubjectFacetBinding::SubjectId,
                    })],
                    accepts_tail: false,
                    confirmation: None,
                    returns: Some("swawkit.context/v1".to_owned()),
                },
            }],
        };

        kind.validate().expect("valid Subject kind");
        let facet = kind
            .instantiate("show", "release-check")
            .expect("materialize Facet")
            .expect("known Facet");
        let FacetResolver::Command { arguments, .. } = facet.resolver.expect("resolver") else {
            panic!("expected command resolver");
        };
        assert_eq!(arguments, ["release-check"]);
    }

    #[test]
    fn subject_kind_refs_require_one_explicit_command_provider() {
        SubjectKindRef {
            kind: "run".to_owned(),
            provider: SubjectRef::Command {
                source: CommandSource::Kernel,
                address: ".runs".to_owned(),
            },
        }
        .validate()
        .expect("command provider");

        let error = SubjectKindRef {
            kind: "run".to_owned(),
            provider: SubjectRef::Instance {
                kind: "provider".to_owned(),
                id: "provider-id".to_owned(),
            },
        }
        .validate()
        .expect_err("instance providers must fail");
        assert!(error.contains("command Subject"));
    }
}
