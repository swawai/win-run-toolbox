import { describe, expect, test } from "bun:test";

import {
  instantiateSubjectFacets,
  normalizeSubjectKinds,
} from "./subject-kind-model.js";

const invalid = (message) => new Error(message);

function kinds(argumentsTemplate = [{ bind: "subject.id" }]) {
  return [{
    kind: "context",
    facets: [{
      id: "overview",
      kind: "projection",
      renderer: "overview",
      icon: "i",
      label: "Overview",
      summary: "Inspect Context",
      resolver: {
        type: "command",
        address: ".context.show",
        arguments: argumentsTemplate,
        returns: "swawkit.context/v1",
      },
    }],
  }];
}

describe("Subject kind template model", () => {
  test("keeps subject.id typed until one trusted instance is selected", () => {
    const [subjectKind] = normalizeSubjectKinds(kinds(), "subjectKinds", invalid);
    expect(subjectKind.facets[0].resolver.arguments).toEqual([{ bind: "subject.id" }]);
    expect(instantiateSubjectFacets(
      subjectKind,
      ["overview"],
      "release-check",
      invalid,
    )[0].resolver.arguments).toEqual(["release-check"]);
  });

  test("rejects string interpolation and bindings from another Subject scope", () => {
    expect(() => normalizeSubjectKinds(
      kinds([{ bind: "commandAddress" }]),
      "subjectKinds",
      invalid,
    )).toThrow("must bind subject.id");
    expect(() => normalizeSubjectKinds(
      kinds([{ bind: "subject.id", fallback: "unsafe" }]),
      "subjectKinds",
      invalid,
    )).toThrow("must bind subject.id");
  });

  test("rejects an undeclared stateful Facet subset", () => {
    const [subjectKind] = normalizeSubjectKinds(kinds(), "subjectKinds", invalid);
    expect(() => instantiateSubjectFacets(
      subjectKind,
      ["delete"],
      "release-check",
      invalid,
    )).toThrow("unknown Facet");
  });
});
