import { describe, expect, test } from "bun:test";

import { createSubjectCollection } from "./subject-collection-model.js";

function collectionFacet() {
  return {
    icon: "#",
    id: "runs",
    kind: "collection",
    label: "Runs",
    renderer: "collection",
    resolver: {
      acceptsTail: false,
      address: ".logs",
      arguments: ["--json"],
      confirmation: null,
      returns: "swawkit.subject-collection/v2",
      type: "command",
    },
    subjectKind: {
      kind: "run",
      provider: { type: "command", source: "kernel", address: ".logs" },
    },
    summary: "Browse runs",
  };
}

function runKind() {
  return {
    kind: "run",
    facets: [
      {
        id: "overview",
        kind: "projection",
        renderer: "overview",
        icon: "i",
        label: "Overview",
        summary: "Inspect run",
        subjectKind: null,
        resolver: {
          type: "command",
          address: ".logs",
          arguments: [{ bind: "subject.id" }],
          acceptsTail: false,
          confirmation: null,
          returns: "fixture.run/v1",
        },
      },
      {
        id: "open",
        kind: "operation",
        renderer: "run",
        icon: ">",
        label: "Open",
        summary: "Open output",
        subjectKind: null,
        resolver: {
          type: "command",
          address: "artifact.open",
          arguments: [{ bind: "subject.id" }],
          acceptsTail: false,
          confirmation: null,
          returns: null,
        },
      },
    ],
  };
}

function catalog() {
  const command = {
    address: ".logs",
    aliasOf: null,
    runnable: true,
    source: "kernel",
  };
  const subjectKind = runKind();
  return {
    subjectKindByKind: new Map([["run", { command, subjectKind }]]),
  };
}

function collection(facetIds = ["overview", "open"]) {
  return {
    facet: "runs",
    owner: { type: "command", source: "kernel", address: ".logs" },
    protocol: "swawkit.subject-collection/v2",
    subjects: [{
      ref: { type: "instance", kind: "run", id: "run-01" },
      label: "::run/run-01",
      summary: "successful run",
      facetIds,
    }],
  };
}

const owner = { type: "command", source: "kernel", address: ".logs" };

describe("Subject collection v2 model", () => {
  test("binds trusted Subject templates to an instance ref", () => {
    const result = createSubjectCollection(
      collection(),
      catalog(),
      owner,
      collectionFacet(),
    );
    expect(result.subjects[0].canonicalRef).toBe("::run/run-01");
    expect(result.subjects[0].ref).toEqual({ type: "instance", kind: "run", id: "run-01" });
    expect(result.subjects[0].facets[1].resolver.arguments).toEqual(["run-01"]);
  });

  test("lets collection state expose a strict template subset", () => {
    const result = createSubjectCollection(
      collection(["overview"]),
      catalog(),
      owner,
      collectionFacet(),
    );
    expect(result.subjects[0].facets.map(({ id }) => id)).toEqual(["overview"]);
  });

  test("reuses a provider-owned Subject kind from another command collection", () => {
    const document = collection();
    document.owner = { type: "command", source: "kernel", address: ".tool" };
    const result = createSubjectCollection(
      document,
      catalog(),
      document.owner,
      collectionFacet(),
    );

    expect(result.ownerRef.address).toBe(".tool");
    expect(result.subjects[0].canonicalRef).toBe("::run/run-01");
    expect(result.subjects[0].facets.map(({ id }) => id)).toEqual(["overview", "open"]);
  });

  test("rejects owner, kind, and Facet capability mismatches", () => {
    expect(() => createSubjectCollection(
      collection(),
      catalog(),
      { type: "command", source: "kernel", address: ".check" },
      collectionFacet(),
    )).toThrow("owner does not match");

    const wrongKind = collection();
    wrongKind.subjects[0].ref.kind = "artifact";
    expect(() => createSubjectCollection(
      wrongKind,
      catalog(),
      owner,
      collectionFacet(),
    )).toThrow("does not match the collection Subject kind");

    expect(() => createSubjectCollection(
      collection(["missing"]),
      catalog(),
      owner,
      collectionFacet(),
    )).toThrow("unknown Facet");
  });

  test("rejects executable definitions and recursive owners in collection data", () => {
    const legacy = collection();
    legacy.subjects[0].facets = [];
    expect(() => createSubjectCollection(
      legacy,
      catalog(),
      owner,
      collectionFacet(),
    )).toThrow("not part of SubjectCollection v2");

    const nested = collection();
    nested.owner = { type: "instance", kind: "run", id: "parent" };
    expect(() => createSubjectCollection(
      nested,
      catalog(),
      nested.owner,
      collectionFacet(),
    )).toThrow("owner must identify a command Subject");
  });

  test("uses the same bounded instance identity and display text as the wire protocol", () => {
    for (const id of ["Bad", "has/slash", "has space"]) {
      const document = collection();
      document.subjects[0].ref.id = id;
      expect(() => createSubjectCollection(
        document,
        catalog(),
        owner,
        collectionFacet(),
      )).toThrow("typed instance Subject");
    }

    const numeric = collection();
    numeric.subjects[0].ref.id = "20260816-001";
    expect(createSubjectCollection(
      numeric,
      catalog(),
      owner,
      collectionFacet(),
    ).subjects[0].canonicalRef).toBe("::run/20260816-001");

    const oversized = collection();
    oversized.subjects[0].label = "x".repeat(129);
    expect(() => createSubjectCollection(
      oversized,
      catalog(),
      owner,
      collectionFacet(),
    )).toThrow("at most 128 characters");
  });
});
