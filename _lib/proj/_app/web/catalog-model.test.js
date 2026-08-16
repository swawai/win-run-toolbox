import { describe, expect, test } from "bun:test";
import {
  childrenOf,
  createCatalog,
  isGroup,
} from "./catalog-model.js";

const protocol = "swawkit.command-catalog/v13";

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
    module: null,
    help: null,
    subjectKinds: [],
    facets: [],
    view: null,
    diagnostic: null,
    ...overrides,
  };
}

function payload(commands, overrides = {}) {
  return {
    protocol,
    entryName: "swawkit",
    language: "zh-CN",
    commands,
    ...overrides,
  };
}

describe("Catalog v13 model", () => {
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

  test("keeps typed Profile settings and their ancestors available during setup", () => {
    const catalog = createCatalog(payload([
      node(".dev"),
      node(".dev.bun", { parent: ".dev" }),
      node(".dev.bun.mode", {
        parent: ".dev.bun",
        runnable: true,
        entry: "run.core.json",
        adapter: "core",
        handler: "entry.profile.set",
      }),
      node(".dev.exec", { parent: ".dev" }),
    ]));

    expect(catalog.commandByAddress.get(".dev").setupAvailable).toBe(true);
    expect(catalog.commandByAddress.get(".dev.bun").setupAvailable).toBe(true);
    expect(catalog.commandByAddress.get(".dev.bun.mode").setupAvailable).toBe(true);
    expect(catalog.commandByAddress.get(".dev.exec").setupAvailable).toBe(false);
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
      .toThrow("protocol 必须是 swawkit.command-catalog/v13");
  });

  test("normalizes declared module requirements and provisions", () => {
    const catalog = createCatalog(payload([
      node(".consumer", {
        module: {
          schema: "swawkit.command-module/v4",
          requires: [{ provider: ".provider", contract: "swawkit.fixture/v1" }],
          provides: [{ contract: "swawkit.consumer/v1" }],
        },
      }),
    ]));
    expect(catalog.commandByAddress.get(".consumer").module).toEqual({
      schema: "swawkit.command-module/v4",
      requires: [{ provider: ".provider", contract: "swawkit.fixture/v1" }],
      provides: [{ contract: "swawkit.consumer/v1" }],
    });
  });

  test("rejects the removed command-module/v3 contract", () => {
    expect(() => createCatalog(payload([
      node(".legacy", {
        module: {
          schema: "swawkit.command-module/v3",
          requires: [],
          provides: [{ contract: "legacy/v1" }],
        },
      }),
    ]))).toThrow("swawkit.command-module/v4");
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

  test("accepts Entry commands and rejects unknown sources", () => {
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

  test("allows the dedicated edit renderer to target a typed Control setting", () => {
    const address = "..entry.language";
    const catalog = createCatalog(payload([
      node(address, {
        source: "control",
        runnable: true,
        entry: "run.core.json",
        adapter: "core",
        handler: "entry.profile.set",
        facets: [{
          id: "edit",
          kind: "operation",
          renderer: "edit",
          icon: "*",
          label: "Language",
          summary: "Set language",
          resolver: { type: "command", address, arguments: [] },
        }],
      }),
    ]));
    expect(catalog.commandByAddress.get(address).facets[0].renderer).toBe("edit");
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
      node(".dev.rust", {
        source: "kernel",
        view: { childrenColumn: { width: "wide" } },
      }),
    ]));

    expect(catalog.commandByAddress.get(".dev.rust").childrenColumnWidth)
      .toBe("wide");
    expect(() => createCatalog(payload([
      node(".broken", {
        view: { childrenColumn: { width: "480px" } },
      }),
    ]))).toThrow("width 只能是 normal 或 wide");
  });

  test("normalizes fixed Web run operations and rejects ambiguous declarations", () => {
    const catalog = createCatalog(payload([
      node("maintenance.cleanup", {
        source: "action",
        runnable: true,
        entry: "run.ps1",
        adapter: "pwsh",
        view: {
          run: {
            operations: [
              { id: "preview", label: "预览", arguments: [] },
              {
                id: "apply",
                label: "清理",
                arguments: ["--apply"],
                confirmation: "确认清理？",
              },
            ],
          },
        },
      }),
    ]));

    expect(catalog.commandByAddress.get("maintenance.cleanup").runOperations)
      .toEqual([
        { id: "preview", label: "预览", arguments: [], confirmation: null },
        {
          id: "apply",
          label: "清理",
          arguments: ["--apply"],
          confirmation: "确认清理？",
        },
      ]);
    expect(() => createCatalog(payload([
      node(".broken", {
        view: {
          run: {
            operations: [
              { id: "apply", label: "清理", arguments: [] },
              { id: "apply", label: "再次清理", arguments: [] },
            ],
          },
        },
      }),
    ]))).toThrow("id 必须唯一");
  });

  test("normalizes a command-resolved Subject collection Facet", () => {
    const catalog = createCatalog(payload([
      node(".context.list", {
        runnable: true,
        entry: "run.ps1",
        adapter: "pwsh",
      }),
      node(".context", {
        subjectKinds: [{
          kind: "context",
          facets: [{
            id: "overview",
            kind: "projection",
            renderer: "overview",
            icon: "i",
            label: "概览",
            summary: "查看 Context",
            resolver: {
              type: "command",
              address: ".context.list",
              arguments: [{ bind: "subject.id" }],
              returns: "swawkit.context/v1",
            },
          }],
        }],
        facets: [{
          id: "contexts",
          kind: "collection",
          renderer: "collection",
          icon: "◆",
          label: "上下文",
          summary: "浏览持久化的 Agent 上下文",
          resolver: {
            type: "command",
            address: ".context.list",
            arguments: ["--json"],
            returns: "swawkit.subject-collection/v2",
          },
          subjectKind: {
            kind: "context",
            provider: { type: "command", source: "kernel", address: ".context" },
          },
        }],
      }),
    ]));

    const context = catalog.commandByAddress.get(".context");
    expect(context.facets[0].resolver).toEqual({
      acceptsTail: false,
      address: ".context.list",
      arguments: ["--json"],
      confirmation: null,
      returns: "swawkit.subject-collection/v2",
      type: "command",
    });
  });

  test("resolves a collection through an explicit cross-command Subject kind provider", () => {
    const runs = {
      kind: "run",
      facets: [{
        id: "overview",
        kind: "projection",
        renderer: "overview",
        icon: "i",
        label: "Overview",
        summary: "Inspect Run",
        resolver: {
          type: "command",
          address: ".logs",
          arguments: [{ bind: "subject.id" }],
          returns: "swawkit.command-run-journal/v1",
        },
      }],
    };
    const runsFacet = {
      id: "runs",
      kind: "collection",
      renderer: "collection",
      icon: "=",
      label: "Runs",
      summary: "Browse Runs",
      resolver: {
        type: "command",
        address: ".logs",
        arguments: ["--json", "kernel/.tool"],
        returns: "swawkit.subject-collection/v2",
      },
      subjectKind: {
        kind: "run",
        provider: { type: "command", source: "kernel", address: ".logs" },
      },
    };
    const catalog = createCatalog(payload([
      node(".logs", {
        runnable: true,
        entry: "run.core.json",
        adapter: "core",
        handler: "meta.logs",
        subjectKinds: [runs],
      }),
      node(".tool", {
        runnable: true,
        entry: "run.cmd",
        adapter: "cmd",
        facets: [runsFacet],
      }),
    ]));

    expect(catalog.commandByAddress.get(".tool").facets[0].subjectKind.provider.address)
      .toBe(".logs");

    const missingProvider = structuredClone(runsFacet);
    missingProvider.subjectKind.provider.address = ".tool";
    expect(() => createCatalog(payload([
      node(".logs", {
        runnable: true,
        entry: "run.core.json",
        adapter: "core",
        handler: "meta.logs",
        subjectKinds: [runs],
      }),
      node(".tool", {
        runnable: true,
        entry: "run.cmd",
        adapter: "cmd",
        facets: [missingProvider],
      }),
    ]))).toThrow("unavailable Subject kind");
  });

  test("normalizes resolved Facets with exact CLI commands", () => {
    const catalog = createCatalog(payload([
      node(".check", {
        runnable: true,
        entry: "run.core.json",
        adapter: "core",
        handler: "meta.check",
      }),
      node(".context.list", {
        facets: [
          {
            id: "overview",
            kind: "projection",
            renderer: "overview",
            icon: "i",
            label: "概览",
            summary: "查看命令",
          },
          {
            id: "check",
            kind: "operation",
            renderer: "run",
            icon: "!",
            label: "检查",
            summary: "检查模块",
            resolver: {
              type: "command",
              address: ".check",
              arguments: [".context.list"],
            },
          },
        ],
      }),
    ]));

    const facets = catalog.commandByAddress.get(".context.list").facets;
    expect(facets[0].id).toBe("overview");
    expect(facets[0].resolver).toBeNull();
    expect(facets[1].id).toBe("check");
    expect(facets[1].resolver).toEqual({
      acceptsTail: false,
      address: ".check",
      arguments: [".context.list"],
      confirmation: null,
      returns: null,
      type: "command",
    });
    expect(() => createCatalog(payload([
      node(".context.list", {
        facets: [{
          id: "check",
          kind: "operation",
          renderer: "run",
          icon: "!",
          label: "检查",
          summary: "检查模块",
          resolver: { type: "command", address: ".missing", arguments: [] },
        }],
      }),
    ]))).toThrow("引用了不存在的命令");
  });
});
