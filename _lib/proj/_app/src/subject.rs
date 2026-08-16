use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::catalog::CommandSource;

pub const SUBJECT_COLLECTION_PROTOCOL: &str = "swawkit.subject-collection/v2";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum SubjectRef {
    Command {
        source: CommandSource,
        address: String,
    },
    Instance {
        kind: String,
        id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubjectSummary {
    #[serde(rename = "ref")]
    pub reference: SubjectRef,
    pub label: String,
    pub summary: String,
    pub facet_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubjectCollection {
    pub protocol: String,
    pub owner: SubjectRef,
    pub facet: String,
    pub subjects: Vec<SubjectSummary>,
}

impl SubjectCollection {
    pub fn validate(&self) -> Result<(), String> {
        if self.protocol != SUBJECT_COLLECTION_PROTOCOL {
            return Err(format!("protocol must be {SUBJECT_COLLECTION_PROTOCOL}"));
        }
        if !matches!(self.owner, SubjectRef::Command { .. }) {
            return Err(
                "v2 collection owner must be a command Subject; nested collections are unsupported"
                    .to_owned(),
            );
        }
        validate_subject_ref(&self.owner)?;
        if !valid_token(&self.facet) {
            return Err("collection facet must match [a-z][a-z0-9-]{0,31}".to_owned());
        }

        let mut references = BTreeSet::new();
        for subject in &self.subjects {
            let SubjectRef::Instance { kind, id } = &subject.reference else {
                return Err("v2 collection items must be instance Subjects".to_owned());
            };
            validate_subject_ref(&subject.reference)?;
            if !references.insert((kind.as_str(), id.as_str())) {
                return Err("collection contains a duplicate Subject ref".to_owned());
            }
            validate_text(&subject.label, 128, "Subject label")?;
            validate_text(&subject.summary, 500, "Subject summary")?;
            validate_subject_facet_ids(&subject.facet_ids)?;
        }
        Ok(())
    }
}

pub(crate) fn validate_subject_ref(reference: &SubjectRef) -> Result<(), String> {
    match reference {
        SubjectRef::Command { source, address } => {
            if address.contains('\0')
                || address.len() > 256
                || (address.is_empty() && *source != CommandSource::Kernel)
            {
                return Err("command Subject ref is invalid".to_owned());
            }
        }
        SubjectRef::Instance { kind, id } => {
            if !valid_token(kind) || !valid_instance_id(id) {
                return Err("instance Subject ref kind or id is invalid".to_owned());
            }
        }
    }
    Ok(())
}

fn validate_subject_facet_ids(facet_ids: &[String]) -> Result<(), String> {
    if facet_ids.is_empty() || facet_ids.len() > 32 {
        return Err("Subject must expose 1 to 32 facet ids".to_owned());
    }
    let mut identifiers = BTreeSet::new();
    for facet_id in facet_ids {
        if !valid_token(facet_id) || !identifiers.insert(facet_id.as_str()) {
            return Err("Subject contains a duplicate facet id".to_owned());
        }
    }
    Ok(())
}

pub(crate) fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || (index > 0 && (byte.is_ascii_digit() || byte == b'-'))
        })
}

pub(crate) fn valid_instance_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || (index > 0 && byte == b'-')
        })
}

fn validate_text(value: &str, maximum: usize, field: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.chars().count() > maximum {
        Err(format!(
            "{field} must contain 1 to {maximum} trimmed characters"
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn collection_with(facet_ids: serde_json::Value) -> SubjectCollection {
        serde_json::from_value(json!({
            "protocol": SUBJECT_COLLECTION_PROTOCOL,
            "owner": {"type": "command", "source": "kernel", "address": ".context"},
            "facet": "contexts",
            "subjects": [{
                "ref": {"type": "instance", "kind": "context", "id": "release-check"},
                "label": "::context/release-check",
                "summary": "1 command",
                "facetIds": facet_ids
            }]
        }))
        .expect("Subject collection JSON")
    }

    #[test]
    fn rejects_duplicate_subject_refs() {
        let mut collection = collection_with(json!(["overview"]));
        collection.subjects.push(collection.subjects[0].clone());

        assert!(
            collection
                .validate()
                .expect_err("duplicate ref must fail")
                .contains("duplicate Subject ref")
        );
    }

    #[test]
    fn rejects_duplicate_instance_facet_ids() {
        let collection = collection_with(json!(["overview", "overview"]));

        assert!(
            collection
                .validate()
                .expect_err("duplicate Facet id must fail")
                .contains("duplicate facet id")
        );
    }

    #[test]
    fn accepts_a_valid_one_level_collection() {
        let collection = collection_with(json!(["overview"]));

        collection.validate().expect("valid Subject collection");
    }

    #[test]
    fn accepts_a_numeric_run_id_without_relaxing_the_kind_token() {
        let mut collection = collection_with(json!(["overview"]));
        collection.subjects[0].reference = SubjectRef::Instance {
            kind: "run".to_owned(),
            id: "20260816-01".to_owned(),
        };
        collection.subjects[0].label = "::run/20260816-01".to_owned();

        collection.validate().expect("numeric run ID");
    }
}
