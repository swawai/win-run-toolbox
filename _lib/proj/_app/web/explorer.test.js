import { describe, expect, test } from "bun:test";

import {
  availableCommand,
  captureColumnScrollOffsets,
  childrenColumnWidth,
  choiceColumnModels,
  commandHasChoices,
  commandDisabledDuringSetup,
  commandMenuExpanded,
  restoreColumnScrollOffsets,
} from "./explorer.js";

describe("Explorer control-plane behavior", () => {
  test("keeps Control commands available during first setup", () => {
    expect(commandDisabledDuringSetup(true, { source: "control" })).toBe(false);
    expect(commandDisabledDuringSetup(true, { source: "kernel" })).toBe(true);
    expect(commandDisabledDuringSetup(true, {
      source: "kernel",
      setupAvailable: true,
    })).toBe(false);
    expect(commandDisabledDuringSetup(true, { source: "action" })).toBe(true);
    expect(commandDisabledDuringSetup(false, { source: "action" })).toBe(false);
  });

  test("falls back instead of selecting a disabled routed command", () => {
    const control = { address: "..entry", source: "control" };
    const action = { address: "proj", source: "action" };
    const catalog = {
      commandByAddress: new Map([
        [control.address, control],
        [action.address, action],
      ]),
    };

    expect(availableCommand(catalog, true, action.address)).toBeNull();
    expect(availableCommand(catalog, true, control.address)).toBe(control);
    expect(availableCommand(catalog, false, action.address)).toBe(action);
    expect(availableCommand(catalog, false, "missing")).toBeNull();
  });

  test("expands a local view menu only for the terminal selection", () => {
    const path = [".dev", ".dev.rust", ".dev.rust.cargo"];
    expect(commandMenuExpanded(path, ".dev", 0)).toBe(false);
    expect(commandMenuExpanded(path, ".dev.rust", 1)).toBe(false);
    expect(commandMenuExpanded(path, ".dev.rust.cargo", 2)).toBe(true);
    expect(commandMenuExpanded(path, ".dev.bun", 2)).toBe(false);
  });

  test("uses the parent command's declared child column width", () => {
    expect(childrenColumnWidth({ childrenColumnWidth: "wide" })).toBe("wide");
    expect(childrenColumnWidth({})).toBe("normal");
  });

  test("reveals choices only through declared Facets", () => {
    const parent = { address: "proj" };
    const leaf = { address: "proj.build" };
    const catalog = {
      childrenByParent: new Map([[parent.address, [leaf]]]),
    };

    expect(commandHasChoices(catalog, parent, [{ name: "children" }])).toBe(true);
    expect(commandHasChoices(catalog, leaf, [{ name: "overview" }])).toBe(true);
    expect(commandHasChoices(catalog, leaf, [])).toBe(false);
  });

  test("keeps ancestor child columns but obeys the terminal command view", () => {
    const entry = { address: ".dev" };
    const env = { address: ".dev.rust" };
    const bun = { address: ".dev.rust.cargo" };
    const catalog = {
      commandByAddress: new Map([
        [entry.address, entry],
        [env.address, env],
        [bun.address, bun],
      ]),
      childrenByParent: new Map([
        [entry.address, [env]],
        [env.address, [bun]],
      ]),
    };
    const overviewModels = choiceColumnModels(
      catalog,
      [entry.address, env.address, bun.address],
      (command) => command.address === bun.address
        ? [{ name: "overview", selected: true }]
        : [{
          kind: "collection",
          name: "children",
          resolver: { relation: "children", type: "catalog" },
          selected: false,
        }],
    );

    expect(overviewModels.map(({ command }) => command.address)).toEqual([
      entry.address,
      env.address,
    ]);
    expect(overviewModels.map(({ mode }) => mode)).toEqual([
      "children",
      "children",
    ]);

    catalog.childrenByParent.set(bun.address, [{
      address: `${bun.address}.mode`,
    }]);
    const childrenModels = choiceColumnModels(
      catalog,
      [entry.address, env.address, bun.address],
      (command) => [{
        kind: "collection",
        name: "children",
        resolver: { relation: "children", type: "catalog" },
        selected: command.address === bun.address,
      }],
    );
    expect(childrenModels.map(({ command }) => command.address)).toEqual([
      entry.address,
      env.address,
      bun.address,
    ]);
  });

  test("keeps the collection column visible for a selected Subject", () => {
    const context = { address: ".context" };
    const catalog = {
      commandByAddress: new Map([[context.address, context]]),
      childrenByParent: new Map(),
    };
    const models = choiceColumnModels(
      catalog,
      [context.address],
      () => [{ name: "overview", selected: true }],
      { owner: context.address, facet: "contexts" },
    );
    expect(models.map(({ command }) => command.address)).toEqual([context.address]);
    expect(models.map(({ mode }) => mode)).toEqual(["contexts"]);
  });

  test("treats structural, Subject, and projection Facets as mutually exclusive", () => {
    const context = { address: ".context" };
    const list = { address: ".context.list" };
    const catalog = {
      commandByAddress: new Map([[context.address, context]]),
      childrenByParent: new Map([[context.address, [list]]]),
    };
    const modelsFor = (selected) => choiceColumnModels(
      catalog,
      [context.address],
      () => [
        {
          kind: "collection",
          name: "children",
          resolver: { relation: "children", type: "catalog" },
          selected: selected === "children",
        },
        { kind: "collection", name: "contexts", selected: selected === "contexts" },
        { name: "logs", selected: selected === "logs" },
      ],
    );

    expect(modelsFor("children").map(({ mode }) => mode)).toEqual(["children"]);
    expect(modelsFor("contexts").map(({ mode }) => mode)).toEqual(["contexts"]);
    expect(modelsFor("logs")).toEqual([]);
  });

  test("restores vertical offsets only for columns representing the same parent", () => {
    let rendered = [
      { dataset: { scrollKey: "root" }, scrollTop: 17 },
      { dataset: { scrollKey: "children:.dev.rust" }, scrollTop: 559 },
    ];
    const columns = {
      querySelectorAll() {
        return rendered;
      },
    };
    const offsets = captureColumnScrollOffsets(columns);

    rendered = [
      { dataset: { scrollKey: "root" }, scrollTop: 0 },
      { dataset: { scrollKey: "children:.dev.rust" }, scrollTop: 0 },
      { dataset: { scrollKey: "children:proj" }, scrollTop: 0 },
    ];
    restoreColumnScrollOffsets(columns, offsets);

    expect(rendered.map((column) => column.scrollTop)).toEqual([17, 559, 0]);
  });
});
