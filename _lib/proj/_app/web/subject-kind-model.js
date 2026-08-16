import { normalizeFacets } from "./facet-model.js";

const TOKEN = /^[a-z][a-z0-9-]{0,31}$/;
const INSTANCE_ID = /^[a-z0-9][a-z0-9-]{0,127}$/;

function object(value, field, invalid) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw invalid(`${field} must be an object.`);
  }
  return value;
}

function templateArgument(value, field, invalid) {
  if (typeof value === "string") {
    if (value.length > 4096 || value.includes("\0")) {
      throw invalid(`${field} literal is invalid.`);
    }
    return value;
  }
  const binding = object(value, field, invalid);
  if (Object.keys(binding).length !== 1 || binding.bind !== "subject.id") {
    throw invalid(`${field} must bind subject.id.`);
  }
  return { bind: "subject.id" };
}

export function normalizeSubjectKinds(value, field, invalid) {
  if (!Array.isArray(value) || value.length > 8) {
    throw invalid(`${field} must contain at most 8 Subject kind templates.`);
  }
  const kinds = new Set();
  return value.map((rawKind, kindIndex) => {
    const kindField = `${field}[${kindIndex}]`;
    const subjectKind = object(rawKind, kindField, invalid);
    if (!TOKEN.test(subjectKind.kind) || kinds.has(subjectKind.kind)) {
      throw invalid(`${kindField}.kind must be unique and match [a-z][a-z0-9-]{0,31}.`);
    }
    kinds.add(subjectKind.kind);
    if (
      !Array.isArray(subjectKind.facets)
      || subjectKind.facets.length === 0
      || subjectKind.facets.length > 32
    ) {
      throw invalid(`${kindField}.facets must contain 1 to 32 templates.`);
    }
    const materialized = subjectKind.facets.map((rawFacet, facetIndex) => {
      const facetField = `${kindField}.facets[${facetIndex}]`;
      const facet = object(rawFacet, facetField, invalid);
      const resolver = object(facet.resolver, `${facetField}.resolver`, invalid);
      if (resolver.type !== "command" || !Array.isArray(resolver.arguments)) {
        throw invalid(`${facetField} must declare a command resolver.`);
      }
      const argumentsTemplate = resolver.arguments.map((argument, argumentIndex) => (
        templateArgument(argument, `${facetField}.resolver.arguments[${argumentIndex}]`, invalid)
      ));
      const concrete = {
        ...facet,
        resolver: {
          ...resolver,
          arguments: argumentsTemplate.map((argument) => (
            typeof argument === "string" ? argument : "subject"
          )),
        },
      };
      const normalized = normalizeFacets([concrete], facetField, invalid)[0];
      if (
        normalized.kind === "collection"
        || (normalized.kind === "operation" && normalized.renderer !== "run")
        || normalized.resolver?.type !== "command"
      ) {
        throw invalid(`${facetField} is not a supported instance Facet template.`);
      }
      return {
        ...normalized,
        resolver: { ...normalized.resolver, arguments: argumentsTemplate },
      };
    });
    const facetIds = new Set();
    for (const facet of materialized) {
      if (facetIds.has(facet.id)) {
        throw invalid(`${kindField}.facets contains duplicate ids.`);
      }
      facetIds.add(facet.id);
    }
    return { facets: materialized, kind: subjectKind.kind };
  });
}

export function instantiateSubjectFacets(subjectKind, facetIds, subjectId, invalid) {
  if (!INSTANCE_ID.test(subjectId)) {
    throw invalid("Subject id is invalid.");
  }
  const templates = new Map(subjectKind.facets.map((facet) => [facet.id, facet]));
  return facetIds.map((facetId) => {
    const template = templates.get(facetId);
    if (!template) {
      throw invalid(`Subject exposes unknown Facet '${facetId}'.`);
    }
    return {
      ...template,
      resolver: {
        ...template.resolver,
        arguments: template.resolver.arguments.map((argument) => (
          typeof argument === "string" ? argument : subjectId
        )),
      },
    };
  });
}
