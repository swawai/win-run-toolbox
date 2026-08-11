import { describe, expect, test } from "bun:test";

import { createCommandRunOutput } from "./command-run-output.js";
import {
  documentObject,
  elements,
  progressEvent,
} from "./command-run-test-support.js";

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

  test("updates one progress item in place", () => {
    const ui = elements();
    const output = createCommandRunOutput(ui, { document: documentObject() });

    output.append([progressEvent(1)], 0);
    output.append([progressEvent(2, "completed", {
      current: 42,
      total: 42,
      message: "Downloaded fixture.zip",
    })], 1);

    expect(ui.commandRunOutput.children).toHaveLength(1);
    const progress = ui.commandRunOutput.children[0];
    expect(progress.dataset.state).toBe("completed");
    expect(progress.children[0].textContent).toBe("Downloaded fixture.zip");
    expect(progress.children[2].textContent).toContain("42/42 bytes");
    expect(progress.children[1].attributes.get("value")).toBe("42");
  });
});
