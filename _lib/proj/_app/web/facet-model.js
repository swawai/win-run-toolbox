const FACET_ID = /^[a-z][a-z0-9-]{0,31}$/;
const SUBJECT_COLLECTION_PROTOCOL = "swawkit.subject-collection/v2";
const KINDS = new Set(["collection", "operation", "projection"]);
const RENDERERS = new Set(["collection", "edit", "help", "overview", "run"]);

function requireObject(value, field, invalid) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw invalid(`${field} must be an object.`);
  }
  return value;
}

function requireString(value, field, invalid) {
  if (typeof value !== "string" || value.trim().length === 0) {
    throw invalid(`${field} must be a non-empty string.`);
  }
  return value;
}

function optionalString(value, field, invalid, maximum) {
  if (value === undefined || value === null) {
    return null;
  }
  const result = requireString(value, field, invalid);
  if (result.trim() !== result || [...result].length > maximum) {
    throw invalid(`${field} is invalid.`);
  }
  return result;
}

function normalizeCommandResolver(value, field, invalid) {
  const address = requireString(value.address, `${field}.address`, invalid);
  if (
    !Array.isArray(value.arguments)
    || value.arguments.length > 32
    || value.arguments.some((argument) => (
      typeof argument !== "string" || argument.length > 4096
    ))
  ) {
    throw invalid(`${field}.arguments must contain at most 32 strings.`);
  }
  const acceptsTail = value.acceptsTail ?? false;
  if (typeof acceptsTail !== "boolean") {
    throw invalid(`${field}.acceptsTail must be a boolean.`);
  }
  const confirmation = optionalString(value.confirmation, `${field}.confirmation`, invalid, 500);
  const returns = optionalString(value.returns, `${field}.returns`, invalid, 128);
  if (acceptsTail && confirmation !== null) {
    throw invalid(`${field} cannot combine acceptsTail with confirmation.`);
  }
  return {
    acceptsTail,
    address,
    arguments: [...value.arguments],
    confirmation,
    returns,
    type: "command",
  };
}

function normalizeResolver(value, field, invalid) {
  if (value === undefined || value === null) {
    return null;
  }
  const resolver = requireObject(value, field, invalid);
  const type = requireString(resolver.type, `${field}.type`, invalid);
  if (type === "catalog") {
    if (resolver.relation !== "children") {
      throw invalid(`${field}.relation must be children.`);
    }
    return { relation: "children", type: "catalog" };
  }
  if (type === "command") {
    return normalizeCommandResolver(resolver, field, invalid);
  }
  throw invalid(`${field}.type is not supported.`);
}

function normalizeSubjectKindRef(value, field, invalid) {
  const reference = requireObject(value, field, invalid);
  const kind = requireString(reference.kind, `${field}.kind`, invalid);
  if (!FACET_ID.test(kind)) {
    throw invalid(`${field}.kind must match [a-z][a-z0-9-]{0,31}.`);
  }
  const provider = requireObject(reference.provider, `${field}.provider`, invalid);
  if (
    provider.type !== "command"
    || !new Set(["control", "kernel", "action"]).has(provider.source)
    || typeof provider.address !== "string"
    || (provider.address.length === 0 && provider.source !== "kernel")
  ) {
    throw invalid(`${field}.provider must identify a command Subject.`);
  }
  return {
    kind,
    provider: {
      address: provider.address,
      source: provider.source,
      type: "command",
    },
  };
}

export function normalizeFacets(value, field, invalid) {
  if (!Array.isArray(value)) {
    throw invalid(`${field} must be an array.`);
  }
  const identifiers = new Set();
  return value.map((rawFacet, facetIndex) => {
    const facetField = `${field}[${facetIndex}]`;
    const facet = requireObject(rawFacet, facetField, invalid);
    const id = requireString(facet.id, `${facetField}.id`, invalid);
    if (!FACET_ID.test(id) || identifiers.has(id)) {
      throw invalid(`${facetField}.id must be unique and match [a-z][a-z0-9-]{0,31}.`);
    }
    identifiers.add(id);

    const kind = requireString(facet.kind, `${facetField}.kind`, invalid);
    const renderer = requireString(facet.renderer, `${facetField}.renderer`, invalid);
    if (!KINDS.has(kind)) {
      throw invalid(`${facetField}.kind is not supported.`);
    }
    if (!RENDERERS.has(renderer)) {
      throw invalid(`${facetField}.renderer is not supported.`);
    }

    const icon = requireString(facet.icon, `${facetField}.icon`, invalid);
    const label = requireString(facet.label, `${facetField}.label`, invalid);
    const summary = requireString(facet.summary, `${facetField}.summary`, invalid);
    if (
      icon.trim() !== icon
      || [...icon].length > 8
      || label.trim() !== label
      || [...label].length > 64
      || summary.trim() !== summary
      || [...summary].length > 200
    ) {
      throw invalid(`${facetField} contains invalid display text.`);
    }

    const resolver = normalizeResolver(facet.resolver, `${facetField}.resolver`, invalid);
    const subjectKind = facet.subjectKind === undefined || facet.subjectKind === null
      ? null
      : normalizeSubjectKindRef(facet.subjectKind, `${facetField}.subjectKind`, invalid);
    if (
      (kind === "collection") !== (renderer === "collection")
      || (kind === "projection") !== (renderer === "overview")
    ) {
      throw invalid(`${facetField} kind and renderer do not match.`);
    }
    if (kind === "collection" && resolver === null) {
      throw invalid(`${facetField} collection must declare a resolver.`);
    }
    if (kind === "projection" && resolver?.type !== "command") {
      throw invalid(`${facetField} projection must declare a command resolver.`);
    }
    if (
      (kind === "collection" && resolver?.type === "command")
        !== (subjectKind !== null)
    ) {
      throw invalid(`${facetField} command collection must declare one subjectKind.`);
    }
    if (kind === "operation" && resolver?.type !== "command") {
      throw invalid(`${facetField} operation must declare a command resolver.`);
    }
    if (resolver?.type === "catalog" && kind !== "collection") {
      throw invalid(`${facetField} catalog resolver is only valid for collections.`);
    }
    if (
      resolver?.type === "command"
      && (kind === "collection" || kind === "projection")
      && (!resolver.returns || resolver.acceptsTail || resolver.confirmation)
    ) {
      throw invalid(`${facetField} document resolver must declare returns without interactive input.`);
    }
    if (
      kind === "collection"
      && resolver?.type === "command"
      && resolver.returns !== SUBJECT_COLLECTION_PROTOCOL
    ) {
      throw invalid(`${facetField} collection resolver must return ${SUBJECT_COLLECTION_PROTOCOL}.`);
    }
    if (
      kind === "projection"
      && resolver?.type === "command"
      && resolver.returns === SUBJECT_COLLECTION_PROTOCOL
    ) {
      throw invalid(`${facetField} projection resolver cannot return a Subject collection.`);
    }
    if (kind === "operation" && resolver?.returns !== null) {
      throw invalid(`${facetField} operation resolver cannot return a document.`);
    }
    const coreRendererTarget = renderer === "help" ? ".help" : null;
    if (coreRendererTarget && resolver?.address !== coreRendererTarget) {
      throw invalid(`${facetField} ${renderer} renderer requires ${coreRendererTarget}.`);
    }

    return { icon, id, kind, label, renderer, resolver, subjectKind, summary };
  });
}
