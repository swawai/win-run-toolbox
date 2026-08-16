import { describe, expect, test } from "bun:test";
import { createContextProjection } from "./context-projection-model.js";

describe("Context projection model", () => {
  test("validates the selected Context document", () => {
    expect(createContextProjection({
      schema: "swawkit.context/v1",
      id: "release-check",
      commands: [{ source: "kernel", address: ".dev.status" }],
      notes: ["Inspect"],
      prompt: "Build",
    }, "release-check").commands[0].address).toBe(".dev.status");
  });
});
