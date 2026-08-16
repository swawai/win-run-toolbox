import { describe, expect, test } from "bun:test";

import {
  argumentValues,
  commandRunStatus,
  isCommandRunActive,
  isCommandRunSupported,
} from "./command-run-model.js";
import { snapshot } from "./command-run-test-support.js";

describe("command run model", () => {
  test("keeps zero arguments distinct from one empty argument", () => {
    expect(argumentValues([])).toEqual([]);
    expect(argumentValues([{ value: "" }])).toEqual([""]);
    expect(argumentValues([{ value: "A B" }, { value: "\"quoted\"" }]))
      .toEqual(["A B", "\"quoted\""]);
  });

  test("derives active and terminal labels from the protocol state", () => {
    expect(isCommandRunActive(snapshot())).toBe(true);
    expect(isCommandRunActive(snapshot({ state: "canceling" }))).toBe(true);
    expect(isCommandRunActive(snapshot({ state: "exited", exitCode: 0 }))).toBe(false);
    expect(commandRunStatus(snapshot({ state: "exited", exitCode: 0 })))
      .toEqual({ label: "执行成功", tone: "success" });
    expect(commandRunStatus(snapshot({ state: "exited", exitCode: 3 })))
      .toEqual({ label: "执行失败", tone: "error" });
  });

  test("keeps Entry commands on their dedicated Web APIs", () => {
    expect(isCommandRunSupported({ address: "..web", runnable: true, source: "control" })).toBe(false);
    expect(isCommandRunSupported({ address: "", runnable: true, source: "kernel" })).toBe(false);
    expect(isCommandRunSupported({ address: ".dev", runnable: true, source: "kernel" })).toBe(true);
    expect(isCommandRunSupported({ address: "build", runnable: true, source: "action" })).toBe(true);
  });
});
