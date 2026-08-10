import { describe, expect, test } from "bun:test";

import {
  commandAtPath,
  commandPath,
  parseCommandPath,
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
    expect(commandPath({ source: "control", address: "..entry.env.rust" }))
      .toBe("/commands/control/entry/env/rust");
    expect(commandPath({
      source: "control",
      address: "..entry.env.rust.SWAWKIT_PROJ_RUST_MODE",
    })).toBe(
      "/commands/control/entry/env/rust/SWAWKIT_PROJ_RUST_MODE",
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
      "/commands/control/entry/env/rust/SWAWKIT_PROJ_RUST_MODE",
    )).toEqual({
      source: "control",
      address: "..entry.env.rust.SWAWKIT_PROJ_RUST_MODE",
    });
    expect(parseCommandPath("/commands/kernel"))
      .toEqual({ source: "kernel", address: "" });
    expect(parseCommandPath("/")).toBeNull();
  });

  test("rejects invalid or missing commands", () => {
    expect(() => parseCommandPath("/other/action/demo")).toThrow("不是有效");
    expect(() => parseCommandPath("/commands/action")).toThrow("缺少");
    expect(() => parseCommandPath("/commands/action/Bad")).toThrow("无效");
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
    const location = { pathname: "/" };
    const command = { source: "action", address: "demo" };

    updateCommandPath(history, location, command, "replace");
    updateCommandPath(history, location, command, "push");
    location.pathname = "/commands/action/demo";
    updateCommandPath(history, location, command, "push");

    expect(calls).toEqual([
      ["replace", "/commands/action/demo"],
      ["push", "/commands/action/demo"],
    ]);
  });
});
