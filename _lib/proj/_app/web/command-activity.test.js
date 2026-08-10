import { describe, expect, test } from "bun:test";

import {
  commandActivities,
  commandViews,
  defaultCommandView,
} from "./command-activity.js";

describe("command activities", () => {
  test("keeps hierarchy separate from command presentation", () => {
    expect(commandActivities({
      source: "action",
      address: "proj.build.app",
      runnable: true,
    })).toEqual(["overview", "help", "run"]);
    expect(commandActivities({
      source: "control",
      address: "..entry",
      runnable: true,
    })).toEqual(["overview", "help"]);
    expect(commandActivities({
      source: "kernel",
      address: ".group",
      runnable: false,
    })).toEqual(["overview", "help"]);
    expect(commandActivities(null)).toEqual([]);
  });

  test("does not add generic activities beside the dedicated Profile editor", () => {
    expect(commandActivities({
      source: "control",
      address: "..entry.env.default-shell",
      handler: "entry.profile.set",
      runnable: true,
    })).toEqual([]);
  });

  test("treats subcommands as a local UI view and defaults groups to it", () => {
    const command = {
      source: "action",
      address: "proj.build",
      runnable: false,
    };
    expect(commandViews(command, { hasChildren: true }).map(({ name }) => name))
      .toEqual(["children", "overview", "help"]);
    expect(defaultCommandView(command, { hasChildren: true })).toBe("children");
    expect(defaultCommandView(command)).toBe("overview");
    expect(defaultCommandView(null)).toBeNull();
  });
});
