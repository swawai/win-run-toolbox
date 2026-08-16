import { t } from "./i18n.js";
import {
  createModuleCheckProjection,
  MODULE_CHECK_PROTOCOL,
} from "./module-check-projection-model.js";

export function createModuleCheckProjectionRenderer(elements, options = {}) {
  const documentObject = options.document ?? globalThis.document;

  function emptyItem(text) {
    const item = documentObject.createElement("li");
    item.className = "module-check-empty";
    item.textContent = text;
    return item;
  }

  function detail(parent, text) {
    if (!text) {
      return;
    }
    const value = documentObject.createElement("span");
    value.className = "module-check-item-detail";
    value.textContent = text;
    parent.append(value);
  }

  function statusItem(item) {
    const row = documentObject.createElement("li");
    const heading = documentObject.createElement("div");
    const marker = documentObject.createElement("span");
    const name = documentObject.createElement("code");
    row.className = "module-check-item";
    row.dataset.state = item.ready ? "ready" : "blocked";
    heading.className = "module-check-item-heading";
    marker.className = "module-check-marker";
    marker.textContent = item.ready ? "✓" : "!";
    name.textContent = `${item.provider} · ${item.contract}`;
    heading.append(marker, name);
    row.append(heading);
    detail(row, item.message);
    detail(row, item.exportRoot);
    return row;
  }

  function appendDependency(list, dependency) {
    const row = statusItem(dependency);
    if (dependency.publication || dependency.dependencies.length > 0) {
      const children = documentObject.createElement("ul");
      children.className = "module-check-tree";
      if (dependency.publication) {
        appendPublication(children, dependency.publication);
      }
      for (const child of dependency.dependencies) {
        appendDependency(children, child);
      }
      row.append(children);
    }
    list.append(row);
  }

  function appendPublication(list, publication) {
    const row = statusItem(publication);
    if (publication.exports.length > 0) {
      const exports = documentObject.createElement("ul");
      exports.className = "module-check-exports";
      for (const item of publication.exports) {
        const exported = documentObject.createElement("li");
        exported.textContent = `${item.name} · ${item.kind}`;
        exports.append(exported);
      }
      row.append(exports);
    }
    list.append(row);
  }

  function clear() {
    elements.moduleCheckTitle.textContent = "";
    elements.moduleCheckMeta.textContent = "";
    elements.moduleCheckState.textContent = "";
    elements.moduleCheckDiagnostic.textContent = "";
    elements.moduleCheckDiagnostic.hidden = true;
    elements.moduleCheckGuards.replaceChildren();
    elements.moduleCheckDependencies.replaceChildren();
    elements.moduleCheckPublications.replaceChildren();
  }

  function hide() {
    elements.moduleCheckPane.hidden = true;
    clear();
  }

  function render(subject, payload) {
    const document_ = createModuleCheckProjection(payload, subject);
    const command = document_.command;
    elements.moduleCheckTitle.textContent = command.address;
    elements.moduleCheckMeta.textContent = [command.source, command.adapter ?? "none"]
      .join(" · ");
    elements.moduleCheckState.textContent = document_.ok
      ? t("已就绪", "Ready")
      : t("未就绪", "Not ready");
    elements.moduleCheckState.dataset.state = document_.ok ? "ready" : "blocked";
    elements.moduleCheckDiagnostic.textContent = command.diagnostic ?? "";
    elements.moduleCheckDiagnostic.hidden = command.diagnostic === null;

    elements.moduleCheckGuards.replaceChildren(...(
      document_.guards.length === 0
        ? [emptyItem(t("无 Guard", "No guards"))]
        : document_.guards.map((guard) => {
          const item = documentObject.createElement("li");
          item.textContent = `${guard.scope} · ${guard.entry}`;
          return item;
        })
    ));
    elements.moduleCheckDependencies.replaceChildren();
    if (document_.dependencies.length === 0) {
      elements.moduleCheckDependencies.append(emptyItem(t("无声明依赖", "No declared dependencies")));
    } else {
      for (const dependency of document_.dependencies) {
        appendDependency(elements.moduleCheckDependencies, dependency);
      }
    }
    elements.moduleCheckPublications.replaceChildren();
    if (document_.publications.length === 0) {
      elements.moduleCheckPublications.append(emptyItem(t("无声明产物", "No declared publications")));
    } else {
      for (const publication of document_.publications) {
        appendPublication(elements.moduleCheckPublications, publication);
      }
    }
    elements.moduleCheckPane.hidden = false;
  }

  return { hide, protocol: MODULE_CHECK_PROTOCOL, render };
}
