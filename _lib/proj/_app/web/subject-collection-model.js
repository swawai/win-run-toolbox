import { instantiateSubjectFacets } from "./subject-kind-model.js";

const COLLECTION_PROTOCOL = "swawkit.subject-collection/v2";
const TOKEN = /^[a-z][a-z0-9-]{0,31}$/;
const INSTANCE_ID = /^[a-z0-9][a-z0-9-]{0,127}$/;

function invalid(message) {
  return new Error(`Invalid Subject collection protocol: ${message}`);
}

function object(value, field) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw invalid(`${field} must be an object.`);
  }
  return value;
}

function text(value, field, maximum) {
  if (
    typeof value !== "string"
    || value.trim().length === 0
    || value.trim() !== value
    || [...value].length > maximum
  ) {
    throw invalid(`${field} must be a trimmed string of at most ${maximum} characters.`);
  }
  return value;
}

function normalizeSubjectRef(value, field) {
  const reference = object(value, field);
  if (reference.type === "command") {
    if (
      !new Set(["control", "kernel", "action"]).has(reference.source)
      || typeof reference.address !== "string"
      || (reference.address.length === 0 && reference.source !== "kernel")
    ) {
      throw invalid(`${field} must identify a command Subject.`);
    }
    return {
      address: reference.address,
      source: reference.source,
      type: "command",
    };
  }
  if (reference.type === "instance") {
    if (
      !TOKEN.test(reference.kind)
      || typeof reference.id !== "string"
      || !INSTANCE_ID.test(reference.id)
    ) {
      throw invalid(`${field} must identify a typed instance Subject.`);
    }
    return { id: reference.id, kind: reference.kind, type: "instance" };
  }
  throw invalid(`${field}.type is not supported.`);
}

function subjectRefKey(reference) {
  return reference.type === "command"
    ? `${reference.source}:${reference.address}`
    : `::${reference.kind}/${reference.id}`;
}

export function createSubjectCollection(document, catalog, expectedSubject, collectionFacet) {
  const payload = object(document, "Subject collection");
  if (payload.protocol !== COLLECTION_PROTOCOL) {
    throw invalid(`protocol must be ${COLLECTION_PROTOCOL}.`);
  }
  if (
    !expectedSubject
    || !collectionFacet
    || collectionFacet.kind !== "collection"
    || collectionFacet.resolver?.type !== "command"
    || collectionFacet.resolver.returns !== COLLECTION_PROTOCOL
    || !TOKEN.test(collectionFacet.subjectKind?.kind)
  ) {
    throw invalid("the selected Facet does not resolve a Subject collection.");
  }
  const expectedRef = normalizeSubjectRef(
    expectedSubject.ref ?? expectedSubject,
    "expected Subject",
  );
  const owner = normalizeSubjectRef(payload.owner, "owner");
  if (owner.type !== "command") {
    throw invalid("v2 collection owner must identify a command Subject.");
  }
  if (subjectRefKey(owner) !== subjectRefKey(expectedRef)) {
    throw invalid("owner does not match the requested Subject.");
  }
  if (payload.facet !== collectionFacet.id) {
    throw invalid("owner or facet does not match the request.");
  }
  const provider = catalog.subjectKindByKind.get(collectionFacet.subjectKind.kind);
  if (
    !provider
    || provider.command.address !== collectionFacet.subjectKind.provider.address
    || provider.command.source !== collectionFacet.subjectKind.provider.source
  ) {
    throw invalid("collection Subject kind provider is unavailable.");
  }
  if (!Array.isArray(payload.subjects)) {
    throw invalid("subjects must be an array.");
  }

  const references = new Set();
  const ownerLocator = owner.type === "command" ? owner.address : subjectRefKey(owner);
  const subjects = payload.subjects.map((raw, index) => {
    const field = `subjects[${index}]`;
    const subject = object(raw, field);
    const reference = normalizeSubjectRef(subject.ref, `${field}.ref`);
    if (reference.type !== "instance") {
      throw invalid(`${field}.ref must identify an instance Subject.`);
    }
    const canonicalRef = `::${reference.kind}/${reference.id}`;
    if (references.has(canonicalRef)) {
      throw invalid(`${field}.ref is duplicated.`);
    }
    references.add(canonicalRef);
    if (reference.kind !== collectionFacet.subjectKind.kind) {
      throw invalid(`${field}.ref kind does not match the collection Subject kind.`);
    }
    if (subject.facets !== undefined) {
      throw invalid(`${field}.facets is not part of SubjectCollection v2.`);
    }
    if (
      !Array.isArray(subject.facetIds)
      || subject.facetIds.length === 0
      || subject.facetIds.length > 32
      || subject.facetIds.some((facetId) => !TOKEN.test(facetId))
      || new Set(subject.facetIds).size !== subject.facetIds.length
    ) {
      throw invalid(`${field}.facetIds must contain 1 to 32 unique Facet ids.`);
    }
    const facets = instantiateSubjectFacets(
      provider.subjectKind,
      subject.facetIds,
      reference.id,
      invalid,
    );
    return {
      canonicalRef,
      collectionFacet: payload.facet,
      facets,
      id: reference.id,
      label: text(subject.label, `${field}.label`, 128),
      owner: ownerLocator,
      ref: reference,
      summary: text(subject.summary, `${field}.summary`, 500),
      via: owner.type === "command"
        ? { facet: payload.facet, subject: owner }
        : null,
    };
  });
  return {
    facet: payload.facet,
    label: collectionFacet.label,
    owner: ownerLocator,
    ownerRef: owner,
    protocol: COLLECTION_PROTOCOL,
    subjectByRef: new Map(subjects.map((subject) => [subject.canonicalRef, subject])),
    subjects,
  };
}
