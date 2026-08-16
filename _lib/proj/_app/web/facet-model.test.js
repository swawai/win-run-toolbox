import { describe, expect, test } from "bun:test";

import { normalizeFacets } from "./facet-model.js";

const invalid = (message) => new Error(message);

function facet(overrides = {}) {
  return {
    icon: "i",
    id: "overview",
    kind: "projection",
    label: "Overview",
    renderer: "overview",
    summary: "Inspect document",
    ...overrides,
  };
}

function resolver(overrides = {}) {
  return {
    address: ".fixture",
    arguments: [],
    returns: "fixture.document/v1",
    type: "command",
    ...overrides,
  };
}

function subjectKindRef(kind = "item", address = ".fixture") {
  return {
    kind,
    provider: { type: "command", source: "kernel", address },
  };
}

describe("Facet model", () => {
  test("accepts a non-interactive projection document resolver", () => {
    expect(normalizeFacets([
      facet({ resolver: resolver() }),
    ], "facets", invalid)[0].resolver).toEqual({
      acceptsTail: false,
      address: ".fixture",
      arguments: [],
      confirmation: null,
      returns: "fixture.document/v1",
      type: "command",
    });
  });

  test("rejects the removed resolver-free command overview Facet", () => {
    expect(() => normalizeFacets([
      facet(),
    ], "facets", invalid)).toThrow("projection must declare a command resolver");
  });

  test("requires collection and projection command resolvers to return documents", () => {
    expect(() => normalizeFacets([
      facet({
        id: "items",
        kind: "collection",
        renderer: "collection",
        subjectKind: subjectKindRef(),
        resolver: resolver({ returns: undefined }),
      }),
    ], "facets", invalid)).toThrow("must declare returns");
    expect(() => normalizeFacets([
      facet({ resolver: resolver({ acceptsTail: true }) }),
    ], "facets", invalid)).toThrow("without interactive input");
    expect(() => normalizeFacets([
      facet({
        id: "items",
        kind: "collection",
        renderer: "collection",
        subjectKind: subjectKindRef(),
        resolver: resolver({ returns: "fixture.items/v1" }),
      }),
    ], "facets", invalid)).toThrow("swawkit.subject-collection/v2");
    expect(() => normalizeFacets([
      facet({
        resolver: resolver({ returns: "swawkit.subject-collection/v2" }),
      }),
    ], "facets", invalid)).toThrow("projection resolver cannot return a Subject collection");
  });

  test("requires a collection to name one explicit command Subject kind provider", () => {
    const collection = facet({
      id: "items",
      kind: "collection",
      renderer: "collection",
      subjectKind: subjectKindRef("item", ".items"),
      resolver: resolver({ returns: "swawkit.subject-collection/v2" }),
    });
    expect(normalizeFacets([collection], "facets", invalid)[0].subjectKind).toEqual({
      kind: "item",
      provider: { type: "command", source: "kernel", address: ".items" },
    });
    expect(() => normalizeFacets([
      { ...collection, subjectKind: "item" },
    ], "facets", invalid)).toThrow("subjectKind must be an object");
  });

  test("keeps operation resolvers distinct from returned documents", () => {
    expect(() => normalizeFacets([
      facet({
        id: "run",
        kind: "operation",
        renderer: "run",
        resolver: resolver(),
      }),
    ], "facets", invalid)).toThrow("cannot return a document");
  });

  test("does not silently route custom commands through the Core Help renderer", () => {
    expect(() => normalizeFacets([
      facet({
        id: "help",
        kind: "operation",
        renderer: "help",
        resolver: resolver({ returns: undefined }),
      }),
    ], "facets", invalid)).toThrow("requires .help");
  });
});
