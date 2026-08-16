import { describe, expect, test } from "bun:test";

import { MODULE_CHECK_PROTOCOL } from "./module-check-projection-model.js";
import { createModuleCheckProjectionRenderer } from "./module-check-projection.js";

function element(hidden = false) {
  return {
    children: [],
    className: "",
    dataset: {},
    hidden,
    textContent: "",
    append(...children) { this.children.push(...children); },
    replaceChildren(...children) { this.children = children; },
  };
}

function elements() {
  return {
    moduleCheckDependencies: element(),
    moduleCheckDiagnostic: element(true),
    moduleCheckGuards: element(),
    moduleCheckMeta: element(),
    moduleCheckPane: element(true),
    moduleCheckPublications: element(),
    moduleCheckState: element(),
    moduleCheckTitle: element(),
  };
}

function publication(overrides = {}) {
  return {
    provider: ".provider",
    contract: "fixture/v1",
    ready: false,
    status: "missing",
    message: "Build the provider first.",
    statePath: null,
    exportRoot: null,
    exports: [],
    exportsTruncated: false,
    ...overrides,
  };
}

describe("Module check projection renderer", () => {
  test("renders blocked dependencies as a structured check result", () => {
    const nodes = elements();
    const renderer = createModuleCheckProjectionRenderer(nodes, {
      document: { createElement: () => element() },
    });

    renderer.render({ address: ".tool", source: "kernel" }, {
      protocol: MODULE_CHECK_PROTOCOL,
      command: {
        address: ".tool",
        source: "kernel",
        runnable: true,
        adapter: "pwsh",
        diagnostic: null,
      },
      guards: [{ scope: "module", entry: "guard.ps1" }],
      dependencies: [{
        provider: ".provider",
        contract: "fixture/v1",
        ready: false,
        status: "missing",
        message: "Build the provider first.",
        publication: publication(),
        dependencies: [],
      }],
      publications: [],
      ok: false,
    });

    expect(renderer.protocol).toBe(MODULE_CHECK_PROTOCOL);
    expect(nodes.moduleCheckPane.hidden).toBeFalse();
    expect(nodes.moduleCheckTitle.textContent).toBe(".tool");
    expect(nodes.moduleCheckState.dataset.state).toBe("blocked");
    expect(nodes.moduleCheckDependencies.children[0].dataset.state).toBe("blocked");
    expect(nodes.moduleCheckDependencies.children[0].children.at(-1).children).toHaveLength(1);
  });
});
