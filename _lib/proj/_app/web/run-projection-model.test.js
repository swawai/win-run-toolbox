import { describe, expect, test } from "bun:test";

import { createRunProjection, RUN_JOURNAL_PROTOCOL } from "./run-projection-model.js";

function run(overrides = {}) {
  return {
    protocol: RUN_JOURNAL_PROTOCOL,
    id: "000001a009035cb9-00002aac-0000000000000001",
    address: ".dev.status",
    source: "cli",
    state: "exited",
    startedAtUnixMs: 1,
    finishedAtUnixMs: 2,
    exitCode: 0,
    error: null,
    argumentCount: 0,
    profileRevision: "sha256-fixture",
    nextCursor: 1,
    events: [{
      sequence: 1,
      timestampUnixMs: 1,
      phase: "run",
      kind: "output",
      stream: "stdout",
      text: "ok\n",
    }],
    truncated: false,
    ...overrides,
  };
}

describe("Run projection model", () => {
  test("validates a Journal against the selected Run", () => {
    const value = run();
    expect(createRunProjection(value, value.id)).toEqual(expect.objectContaining({
      address: ".dev.status",
      id: value.id,
      state: "exited",
    }));
  });

  test("rejects a resolver response for a different Run", () => {
    expect(() => createRunProjection(run(), "different-run")).toThrow("id 与选中的 Run 不一致");
  });
});
