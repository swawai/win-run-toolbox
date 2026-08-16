import { describe, expect, test } from "bun:test";

import {
  createModuleCheckProjection,
  MODULE_CHECK_PROTOCOL,
} from "./module-check-projection-model.js";

function document(overrides = {}) {
  return {
    protocol: MODULE_CHECK_PROTOCOL,
    command: {
      address: ".tool",
      source: "kernel",
      runnable: true,
      adapter: "pwsh",
      diagnostic: null,
    },
    guards: [],
    dependencies: [],
    publications: [],
    ok: true,
    ...overrides,
  };
}

describe("Module check projection model", () => {
  test("normalizes the check document for the selected exact command", () => {
    expect(createModuleCheckProjection(document(), {
      address: ".tool",
      source: "kernel",
    })).toEqual(document());
  });

  test("rejects a document belonging to another command namespace", () => {
    expect(() => createModuleCheckProjection(document(), {
      address: "tool",
      source: "action",
    })).toThrow("command.address");
  });

  test("rejects an inconsistent aggregate readiness state", () => {
    expect(() => createModuleCheckProjection(document({ ok: false }), {
      address: ".tool",
      source: "kernel",
    })).toThrow("ok");
  });
});
