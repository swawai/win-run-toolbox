import { describe, expect, test } from "bun:test";

import { CONTEXT_PROTOCOL } from "./context-projection.js";
import { createDocumentProjectionView } from "./document-projection.js";

function element(hidden = false) {
  return { dataset: {}, hidden, textContent: "" };
}

function elements() {
  return {
    documentProjectionFeedback: element(),
    documentProjectionJson: element(true),
    documentProjectionPane: element(true),
    documentProjectionProtocol: element(),
    documentProjectionRef: element(),
    documentProjectionTitle: element(),
  };
}

function projection(protocol = "fixture.report/v1") {
  return {
    id: "report",
    kind: "projection",
    label: "Report",
    resolver: { returns: protocol, type: "command" },
  };
}

async function flush() {
  await Promise.resolve();
  await Promise.resolve();
}

describe("document projection view", () => {
  test("resolves a static Command projection and renders unknown protocols as JSON", async () => {
    const nodes = elements();
    const calls = [];
    const command = { address: ".report", source: "kernel" };
    const facet = projection();
    const view = createDocumentProjectionView(nodes, {
      async resolveDocument(subject, selectedFacet) {
        calls.push([subject, selectedFacet]);
        return { protocol: "fixture.report/v1", value: 17 };
      },
    });

    expect(view.select(command, facet)).toBeTrue();
    await flush();

    expect(calls).toEqual([[command, facet]]);
    expect(nodes.documentProjectionPane.hidden).toBeFalse();
    expect(nodes.documentProjectionProtocol.textContent).toBe("fixture.report/v1");
    expect(nodes.documentProjectionJson.hidden).toBeFalse();
    expect(JSON.parse(nodes.documentProjectionJson.textContent)).toEqual({
      protocol: "fixture.report/v1",
      value: 17,
    });
  });

  test("dispatches the Context protocol through its registered renderer", async () => {
    const nodes = elements();
    const rendered = [];
    const hidden = [];
    const subject = {
      canonicalRef: "::context/test",
      ref: { id: "test", kind: "context", type: "instance" },
    };
    const document = { id: "test", schema: CONTEXT_PROTOCOL };
    const view = createDocumentProjectionView(nodes, {
      renderers: [{
        hide() { hidden.push(true); },
        protocol: CONTEXT_PROTOCOL,
        render(...arguments_) { rendered.push(arguments_); },
      }],
      async resolveDocument() { return document; },
    });

    view.select(subject, projection(CONTEXT_PROTOCOL));
    await flush();

    expect(hidden.length).toBeGreaterThan(0);
    expect(rendered).toEqual([[subject, document]]);
    expect(nodes.documentProjectionPane.hidden).toBeTrue();
  });

  test("ignores stale projection responses after selection changes", async () => {
    const nodes = elements();
    const pending = new Map();
    const view = createDocumentProjectionView(nodes, {
      resolveDocument(subject) {
        return new Promise((resolve) => pending.set(subject.address, resolve));
      },
    });
    const facet = projection();

    view.select({ address: ".first", source: "kernel" }, facet);
    view.select({ address: ".second", source: "kernel" }, facet);
    pending.get(".second")({ value: "new" });
    await flush();
    pending.get(".first")({ value: "old" });
    await flush();

    expect(JSON.parse(nodes.documentProjectionJson.textContent)).toEqual({ value: "new" });
    expect(nodes.documentProjectionRef.textContent).toBe("kernel:.second");
  });

  test("shows registered renderer failures in the generic error pane", async () => {
    const nodes = elements();
    const view = createDocumentProjectionView(nodes, {
      renderers: [{
        hide() {},
        protocol: CONTEXT_PROTOCOL,
        render() { throw new Error("Invalid Context document."); },
      }],
      async resolveDocument() { return {}; },
    });

    view.select({ address: ".report", source: "kernel" }, projection(CONTEXT_PROTOCOL));
    await flush();

    expect(nodes.documentProjectionPane.hidden).toBeFalse();
    expect(nodes.documentProjectionFeedback.dataset.state).toBe("error");
    expect(nodes.documentProjectionFeedback.textContent).toBe("Invalid Context document.");
  });

  test("leaves the resolver-free Command base detail outside Facet resolution", () => {
    const nodes = elements();
    let called = false;
    const view = createDocumentProjectionView(nodes, {
      resolveDocument() { called = true; },
    });

    expect(view.select({ address: ".report", source: "kernel" }, null)).toBeFalse();
    expect(called).toBeFalse();
    expect(nodes.documentProjectionPane.hidden).toBeTrue();
  });
});
