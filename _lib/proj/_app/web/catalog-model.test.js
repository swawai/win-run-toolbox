import { describe, expect, test } from "bun:test";
import {
  childrenOf,
  createCatalog,
  isGroup,
} from "./catalog-model.js";

const protocol = "swawkit.command-catalog/v4";

function node(address, overrides = {}) {
  return {
    address,
    source: "kernel",
    parent: "",
    aliasOf: null,
    runnable: false,
    entry: null,
    adapter: null,
    handler: null,
    help: null,
    view: null,
    diagnostic: null,
    ...overrides,
  };
}

function payload(commands, overrides = {}) {
  return {
    protocol,
    entryName: "swawkit",
    commands,
    ...overrides,
  };
}

describe("Catalog v4 model", () => {
  test("derives a non-runnable group only from its children", () => {
    const catalog = createCatalog(payload([
      node(".dev"),
      node(".dev.setup", {
        parent: ".dev",
        runnable: true,
        entry: "run.ps1",
        adapter: "pwsh",
      }),
    ]));
    const group = catalog.commandByAddress.get(".dev");

    expect(group.runnable).toBe(false);
    expect(isGroup(catalog, group)).toBe(true);
    expect(childrenOf(catalog, group.address).map(({ address }) => address))
      .toEqual([".dev.setup"]);
  });

  test("keeps runnable capability independent from group capability", () => {
    const catalog = createCatalog(payload([
      node(".tool", {
        runnable: true,
        entry: "run.exe",
        adapter: "exe",
      }),
      node(".tool.status", {
        parent: ".tool",
        runnable: true,
        entry: "run.ps1",
        adapter: "pwsh",
      }),
    ]));
    const group = catalog.commandByAddress.get(".tool");

    expect(group.runnable).toBe(true);
    expect(isGroup(catalog, group)).toBe(true);
  });

  test("keeps a diagnostic leaf distinct from a command group", () => {
    const catalog = createCatalog(payload([
      node(".broken", { diagnostic: "multiple run entries" }),
    ]));
    const command = catalog.commandByAddress.get(".broken");

    expect(command.issue).toBe("multiple run entries");
    expect(command.runnable).toBe(false);
    expect(isGroup(catalog, command)).toBe(false);
  });

  test("keeps runnable capability independent from diagnostics", () => {
    const catalog = createCatalog(payload([
      node(".documented", {
        runnable: true,
        entry: "run.ps1",
        adapter: "pwsh",
        diagnostic: "help file is empty",
      }),
    ]));
    const command = catalog.commandByAddress.get(".documented");

    expect(command.runnable).toBe(true);
    expect(command.issue).toBe("help file is empty");
  });

  test("rejects an unknown protocol version", () => {
    expect(() => createCatalog(payload([], { protocol: "catalog/v2" })))
      .toThrow("protocol 必须是 swawkit.command-catalog/v4");
  });

  test("rejects a missing entry name", () => {
    const document = payload([]);
    delete document.entryName;

    expect(() => createCatalog(document)).toThrow("entryName 必须是非空字符串");
  });

  test("rejects inconsistent runnable entry fields", () => {
    expect(() => createCatalog(payload([
      node(".broken", { runnable: true }),
    ]))).toThrow("runnable 必须与 entry 是否存在一致");

    expect(() => createCatalog(payload([
      node(".broken", { adapter: "pwsh" }),
    ]))).toThrow("adapter 必须与 entry 同时存在或同时为空");
  });

  test("accepts Control Plane commands and rejects unknown sources", () => {
    const catalog = createCatalog(payload([
      node("..entry", {
        source: "control",
        runnable: true,
        entry: "run.core.json",
        adapter: "core",
        handler: "entry.profile",
      }),
    ]));
    expect(catalog.commandByAddress.get("..entry").handler)
      .toBe("entry.profile");

    expect(() => createCatalog(payload([
      node(".legacy", { source: "project" }),
    ]))).toThrow("source 只能是 control、kernel 或 action");
  });

  test("accepts handlers owned by core and toolchain adapters only", () => {
    const catalog = createCatalog(payload([
      node(".dev.setup", {
        runnable: true,
        entry: "run.toolchain.json",
        adapter: "toolchain",
        handler: "dev.setup",
      }),
    ]));
    expect(catalog.commandByAddress.get(".dev.setup").handler)
      .toBe("dev.setup");

    expect(() => createCatalog(payload([
      node(".broken", {
        runnable: true,
        entry: "run.toolchain.json",
        adapter: "toolchain",
      }),
    ]))).toThrow("handler");
    expect(() => createCatalog(payload([
      node(".broken", {
        runnable: true,
        entry: "run.ps1",
        adapter: "pwsh",
        handler: "dev.setup",
      }),
    ]))).toThrow("handler");
  });

  test("normalizes the parent-owned child column width", () => {
    const catalog = createCatalog(payload([
      node("..entry.env.rust", {
        source: "control",
        view: { childrenColumn: { width: "wide" } },
      }),
    ]));

    expect(catalog.commandByAddress.get("..entry.env.rust").childrenColumnWidth)
      .toBe("wide");
    expect(() => createCatalog(payload([
      node(".broken", {
        view: { childrenColumn: { width: "480px" } },
      }),
    ]))).toThrow("width 只能是 normal 或 wide");
  });
});
