import { describe, expect, test } from "bun:test";

import {
  createSubjectFacetView,
  defaultCommandFacet,
  defaultSubjectFacet,
  subjectFacetItems,
  subjectFacets,
} from "./subject-facet.js";

function facet(id, renderer = id) {
  return {
    icon: id.slice(0, 1),
    id,
    kind: renderer === "overview" ? "projection" : "operation",
    label: id,
    renderer,
    resolver: {
        acceptsTail: false,
        address: ".fixture",
        arguments: [],
        confirmation: null,
        returns: renderer === "overview" ? "fixture.document/v1" : null,
        type: "command",
      },
    summary: `${id} summary`,
  };
}

function collection(id, resolver) {
  return {
    icon: id.slice(0, 1),
    id,
    kind: "collection",
    label: id,
    renderer: "collection",
    resolver,
    summary: `${id} summary`,
  };
}

describe("Subject Facets", () => {
  test("uses only Facets resolved by the Catalog", () => {
    const facets = [
      facet("overview"),
      facet("help"),
      facet("run"),
    ];
    expect(subjectFacets({ facets })).toBe(facets);
    expect(subjectFacets({ facets: [] })).toEqual([]);
    expect(subjectFacets(null)).toEqual([]);
  });

  test("uses structural and command collection resolvers without adapters", () => {
    const subject = {
      facets: [
        collection("children", { relation: "children", type: "catalog" }),
        collection("runs", {
          acceptsTail: false,
          address: ".runs",
          arguments: [],
          confirmation: null,
          returns: "swawkit.subject-collection/v2",
          type: "command",
        }),
        facet("overview"),
      ],
      address: "proj.build",
    };
    expect(subjectFacetItems(subject).map(({ name }) => name)).toEqual([
      "children",
      "runs",
      "overview",
    ]);
    expect(defaultSubjectFacet(subject)).toBe("children");
    expect(defaultSubjectFacet(null)).toBeNull();
  });

  test("opens the stateful Runtime root on overview", () => {
    const subject = {
      facets: [facet("overview")],
      address: "..runtime",
      handler: "runtime.status",
    };
    expect(defaultSubjectFacet(subject)).toBe("overview");
    expect(defaultCommandFacet(subject)).toBeNull();
  });

  test("keeps ordinary Command details outside Facet selection", () => {
    const subject = {
      facets: [facet("help"), collection("runs", {
        acceptsTail: false,
        address: ".runs",
        arguments: [],
        confirmation: null,
        returns: "swawkit.subject-collection/v2",
        type: "command",
      })],
      address: ".fixture",
    };
    expect(defaultCommandFacet(subject)).toBeNull();

    const pane = () => ({ hidden: false });
    const elements = {
      commandWorkspace: pane(),
      entryProfileDetail: pane(),
      commandDetail: pane(),
      commandHelpPane: pane(),
      commandRunPane: pane(),
    };
    const view = createSubjectFacetView(elements, {
      defaultFacet: defaultCommandFacet,
      fallbackRenderer: "overview",
    });
    expect(view.select(subject)).toEqual({
      defaultFacet: null,
      facet: null,
      selectedFacet: null,
    });
    expect(elements.commandWorkspace.hidden).toBeFalse();
    expect(elements.commandDetail.hidden).toBeFalse();
  });

  test("renders a custom Facet through its declared renderer", () => {
    const pane = () => ({ hidden: false });
    const elements = {
      commandWorkspace: pane(),
      entryProfileDetail: pane(),
      commandDetail: pane(),
      commandHelpPane: pane(),
      commandRunPane: pane(),
    };
    const view = createSubjectFacetView(elements);
    const command = {
      facets: [facet("overview"), facet("validate", "run")],
      address: ".fixture",
    };

    expect(view.select(command, { facet: "validate" })).toEqual({
      defaultFacet: "overview",
      facet: expect.objectContaining({ name: "validate", renderer: "run" }),
      selectedFacet: "validate",
    });
    expect(elements.commandRunPane.hidden).toBeFalse();
    expect(elements.commandWorkspace.hidden).toBeFalse();
    expect(elements.commandDetail.hidden).toBeTrue();
  });

  test("routes overridden conventional ids only by their declared renderer", () => {
    for (const id of ["help", "runs"]) {
      const pane = () => ({ hidden: false });
      const elements = {
        commandWorkspace: pane(),
        entryProfileDetail: pane(),
        commandDetail: pane(),
        commandHelpPane: pane(),
        commandRunPane: pane(),
      };
      const view = createSubjectFacetView(elements);
      view.select({
        address: ".fixture",
        facets: [facet("overview"), facet(id, "run")],
      }, { facet: id });

      expect(elements.commandRunPane.hidden).toBeFalse();
      expect(elements.commandHelpPane.hidden).toBeTrue();
    }
  });

  test("selects a collection without opening a projection pane", () => {
    const pane = () => ({ hidden: false });
    const elements = {
      commandWorkspace: pane(),
      entryProfileDetail: pane(),
      commandDetail: pane(),
      commandHelpPane: pane(),
      commandRunPane: pane(),
    };
    const view = createSubjectFacetView(elements);
    const command = {
      facets: [
        collection("runs", {
          acceptsTail: false,
          address: ".runs",
          arguments: [],
          confirmation: null,
          returns: "swawkit.subject-collection/v2",
          type: "command",
        }),
        facet("overview"),
      ],
      address: ".fixture",
    };

    expect(view.select(command, { facet: "runs" })).toEqual({
      defaultFacet: "runs",
      facet: expect.objectContaining({ kind: "collection", name: "runs" }),
      selectedFacet: "runs",
    });
    expect(elements.commandWorkspace.hidden).toBeTrue();
    expect(view.items(command).find(({ name }) => name === "runs")?.selected).toBeTrue();
  });
});
