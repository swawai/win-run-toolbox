import { describe, expect, test } from "bun:test";

import { createCommandRunOutput } from "./command-run-output.js";
import { documentObject, elements } from "./command-run-test-support.js";

describe("command run output", () => {
  test("bounds retained output by event count and labels both streams", () => {
    const ui = elements();
    const output = createCommandRunOutput(ui, {
      document: documentObject(),
      maxOutputBytes: 100,
      maxOutputEvents: 2,
    });

    output.append([
      { sequence: 1, stream: "stdout", text: "first" },
      { sequence: 2, stream: "stdout", text: "second" },
      { sequence: 3, stream: "stderr", text: "third" },
    ], 0);
    output.render(false);

    expect(ui.commandRunOutput.children.map((child) => child.textContent))
      .toEqual(["second", "third"]);
    expect(ui.commandRunOutput.children[1].attributes.get("aria-describedby"))
      .toBe("command-run-stream-stderr");
    expect(ui.commandRunTruncated.hidden).toBe(false);
  });

  test("measures the retained byte limit as UTF-8", () => {
    const ui = elements();
    const output = createCommandRunOutput(ui, {
      document: documentObject(),
      maxOutputBytes: 5,
      maxOutputEvents: 10,
    });

    output.append([
      { sequence: 1, stream: "stdout", text: "你" },
      { sequence: 2, stream: "stdout", text: "好" },
    ], 0);
    output.render(false);

    expect(ui.commandRunOutput.children.map((child) => child.textContent))
      .toEqual(["好"]);
    expect(ui.commandRunTruncated.hidden).toBe(false);
  });
});
