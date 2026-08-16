import { describe, expect, test } from "bun:test";

import { RUN_JOURNAL_PROTOCOL } from "./run-projection-model.js";
import { createRunProjectionRenderer } from "./run-projection.js";

function element(hidden = false) {
  return {
    children: [],
    dataset: {},
    hidden,
    textContent: "",
    append(...children) { this.children.push(...children); },
    replaceChildren(...children) { this.children = children; },
  };
}

function elements() {
  return {
    runProjectionError: element(true),
    runProjectionMeta: element(),
    runProjectionOutput: element(),
    runProjectionPane: element(true),
    runProjectionRef: element(),
    runProjectionState: element(),
    runProjectionTitle: element(),
    runProjectionTruncated: element(true),
  };
}

describe("Run projection renderer", () => {
  test("renders one protocol document selected through a Run Subject", () => {
    const nodes = elements();
    const id = "000000000000000018cc320bd7eaa8b8-00014b4c-0000000000000004";
    const renderer = createRunProjectionRenderer(nodes, {
      document: { createElement: () => element() },
    });

    renderer.render({
      canonicalRef: `::run/${id}`,
      label: "2026-08-16 05:29:38.607Z",
      ref: { id, kind: "run", type: "instance" },
    }, {
      protocol: RUN_JOURNAL_PROTOCOL,
      id,
      address: ".dev.status",
      source: "web",
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
    });

    expect(nodes.runProjectionPane.hidden).toBeFalse();
    expect(nodes.runProjectionRef.textContent).toBe(`::run/${id}`);
    expect(nodes.runProjectionState.dataset.state).toBe("success");
    expect(nodes.runProjectionOutput.children[0].textContent).toContain("ok\n");
  });
});
