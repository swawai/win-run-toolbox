import { describe, expect, test } from "bun:test";

import { commandActivities } from "./command-activity.js";

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
});
