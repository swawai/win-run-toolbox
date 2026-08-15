import { describe, expect, test } from "bun:test";

import {
  commandAtPath,
  commandPath,
  parseCommandPath,
  parseCommandView,
  updateCommandPath,
} from "./navigation.js";

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

  test("round-trips non-default views without encoding default UI state", () => {
    expect(parseCommandView("")).toBeNull();
    expect(parseCommandView("?view=help")).toBe("help");
    expect(parseCommandView("?view=edit")).toBe("edit");
    expect(parseCommandView("?view=logs")).toBe("logs");
    expect(() => parseCommandView("?view=unknown")).toThrow("未知");
    expect(() => parseCommandView("?view=help&view=run")).toThrow("只能");
    expect(() => parseCommandView("?view=help&draft=1")).toThrow("参数");

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
      defaultView: "children",
      view: "help",
    });
    updateCommandPath(history, location, command, {
      defaultView: "children",
      view: "children",
    });

    expect(calls).toEqual([
      "/commands/action/proj/build?view=help",
    ]);
  });
});
