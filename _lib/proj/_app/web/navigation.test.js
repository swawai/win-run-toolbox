import { describe, expect, test } from "bun:test";

import {
  commandAtPath,
  commandPath,
  parseCommandPath,
  parseCommandSelection,
  parseCommandFacet,
  restoreCommandSelection,
  updateCommandPath,
} from "./navigation.js";
import { createSubjectFacetView } from "./subject-facet.js";

describe("command URL contract", () => {
  test("maps every command source to a namespaced path", () => {
    expect(commandPath({ source: "action", address: "proj.build.launcher" }))
      .toBe("/commands/action/proj/build/launcher");
    expect(commandPath({ source: "kernel", address: ".dev.setup" }))
      .toBe("/commands/kernel/dev/setup");
    expect(commandPath({ source: "kernel", address: "" }))
      .toBe("/commands/kernel");
    expect(commandPath({ source: "kernel", address: ".dev.rust" }))
      .toBe("/commands/kernel/dev/rust");
    expect(commandPath({
      source: "kernel",
      address: ".dev.rust.mode",
    })).toBe(
      "/commands/kernel/dev/rust/mode",
    );
  });

  test("parses canonical paths without relying on dot segments", () => {
    expect(parseCommandPath("/commands/action/proj/build/app"))
      .toEqual({ source: "action", address: "proj.build.app" });
    expect(parseCommandPath("/commands/kernel/dev/setup"))
      .toEqual({ source: "kernel", address: ".dev.setup" });
    expect(parseCommandPath("/commands/control/entry"))
      .toEqual({ source: "control", address: "..entry" });
    expect(parseCommandPath(
      "/commands/kernel/dev/rust/mode",
    )).toEqual({
      source: "kernel",
      address: ".dev.rust.mode",
    });
    expect(parseCommandPath("/commands/kernel"))
      .toEqual({ source: "kernel", address: "" });
    expect(parseCommandPath("/")).toBeNull();
  });

  test("rejects invalid or missing commands", () => {
    expect(() => parseCommandPath("/other/action/demo")).toThrow("不是有效");
    expect(() => parseCommandPath("/commands/action")).toThrow("缺少");
    expect(() => parseCommandPath("/commands/action/Bad")).toThrow("无效");
    expect(() => parseCommandPath(
      "/commands/control/entry/env/SWAWKIT_PROJ_BUN_MODE",
    )).toThrow("无效");
    const catalog = {
      commandByAddress: new Map([
        ["demo", { source: "action", address: "demo" }],
      ]),
    };
    expect(() => commandAtPath(catalog, "/commands/action/missing"))
      .toThrow("不存在");
    expect(commandAtPath(
      catalog,
      "/commands/action/missing",
      { allowMissing: true },
    )).toBeNull();
  });

  test("pushes user navigation and replaces initialization", () => {
    const calls = [];
    const history = {
      pushState(_state, _title, path) { calls.push(["push", path]); },
      replaceState(_state, _title, path) { calls.push(["replace", path]); },
    };
    const location = { pathname: "/", search: "" };
    const command = { source: "action", address: "demo" };

    updateCommandPath(history, location, command, { mode: "replace" });
    updateCommandPath(history, location, command, { mode: "push" });
    location.pathname = "/commands/action/demo";
    updateCommandPath(history, location, command, { mode: "push" });

    expect(calls).toEqual([
      ["replace", "/commands/action/demo"],
      ["push", "/commands/action/demo"],
    ]);
  });

  test("round-trips non-default Facets without encoding default UI state", () => {
    expect(parseCommandFacet("")).toBeNull();
    expect(parseCommandFacet("?facet=help")).toBe("help");
    expect(parseCommandFacet("?facet=edit")).toBe("edit");
    expect(parseCommandFacet("?facet=runs")).toBe("runs");
    expect(parseCommandFacet("?facet=runs")).toBe("runs");
    expect(parseCommandFacet("?facet=validate")).toBe("validate");
    expect(() => parseCommandFacet("?facet=Invalid")).toThrow("无效");
    expect(() => parseCommandFacet("?facet=help&facet=run")).toThrow("只能");
    expect(() => parseCommandFacet("?facet=help&draft=1")).toThrow("参数");

    const calls = [];
    const history = {
      pushState(_state, _title, path) { calls.push(path); },
      replaceState() {},
    };
    const location = {
      pathname: "/commands/action/proj/build",
      search: "",
    };
    const command = { source: "action", address: "proj.build" };

    updateCommandPath(history, location, command, {
      defaultFacet: "children",
      facet: "help",
    });
    updateCommandPath(history, location, command, {
      defaultFacet: "children",
      facet: "children",
    });
    updateCommandPath(history, location, command, {
      defaultFacet: "children",
      facet: "runs",
    });

    expect(calls).toEqual([
      "/commands/action/proj/build?facet=help",
      "/commands/action/proj/build?facet=runs",
    ]);
  });

  test("keeps typed Subject identity and both Facets in query state", () => {
    expect(parseCommandSelection("?facet=runs&subject=%3A%3Arun%2F20260816-001&subject-facet=cancel"))
      .toEqual({
        facet: "runs",
        subject: "::run/20260816-001",
        subjectFacet: "cancel",
      });
    expect(() => parseCommandSelection("?subject=%3A%3Arun%2Frun-01"))
      .toThrow("Facet");
    expect(() => parseCommandSelection("?facet=runs&subject=run-01"))
      .toThrow("无效");
    for (const invalid of ["::run/Bad", "::run/has/slash", "::run/has space"]) {
      expect(() => parseCommandSelection(`?facet=runs&subject=${encodeURIComponent(invalid)}`))
        .toThrow("无效");
    }
    expect(() => parseCommandSelection("?facet=runs&subject=%3A%3Arun%2Fone&subject=%3A%3Arun%2Ftwo"))
      .toThrow("只能");

    expect(parseCommandSelection("?facet=runs")).toEqual({
      facet: "runs",
      subject: null,
      subjectFacet: null,
    });

    const calls = [];
    const history = {
      pushState(_state, _title, path) { calls.push(path); },
      replaceState() {},
    };
    updateCommandPath(
      history,
      { pathname: "/commands/kernel/context", search: "" },
      { source: "kernel", address: ".context" },
      {
        defaultSubjectFacet: "overview",
        facet: "runs",
        subject: "::run/run-01",
        subjectFacet: "cancel",
      },
    );
    expect(calls).toEqual([
      "/commands/kernel/context?facet=runs&subject=%3A%3Arun%2Frun-01&subject-facet=cancel",
    ]);
  });

  test("restores the owner before an asynchronous collection and its Subject", async () => {
    const events = [];
    const facetView = createSubjectFacetView();
    let finishCollection;
    let loadedCollection = null;
    const restored = restoreCommandSelection({
      collectionFacet: "contexts",
      loadCollection(owner, facet) {
        events.push(["load", owner, facet]);
        return new Promise((resolve) => {
          finishCollection = (collection) => {
            loadedCollection = collection;
            resolve(collection);
          };
        });
      },
      ownerAddress: ".context",
      selectOwner() {
        events.push(["owner", ".context", "contexts"]);
        return true;
      },
      selectSubject(owner, facet, subject, options) {
        const selected = loadedCollection.subjects.find(
          (candidate) => candidate.canonicalRef === subject,
        );
        const selectedFacet = facetView.select(selected, options).selectedFacet;
        events.push(["subject", owner, facet, subject, selectedFacet]);
        return Boolean(selected);
      },
      subjectFacet: null,
      subjectRef: "::context/test",
    });

    expect(events).toEqual([
      ["owner", ".context", "contexts"],
      ["load", ".context", "contexts"],
    ]);
    finishCollection({
      subjects: [{
        canonicalRef: "::context/test",
        facets: [{
          icon: "i",
          id: "overview",
          kind: "projection",
          label: "Overview",
          renderer: "overview",
          resolver: {
            acceptsTail: false,
            address: ".context.show",
            arguments: ["test"],
            confirmation: null,
            returns: "swawkit.context/v1",
            type: "command",
          },
          summary: "Inspect Context",
        }],
      }],
    });
    expect(await restored).toBeTrue();
    expect(events[2]).toEqual([
      "subject",
      ".context",
      "contexts",
      "::context/test",
      "overview",
    ]);
  });
});
