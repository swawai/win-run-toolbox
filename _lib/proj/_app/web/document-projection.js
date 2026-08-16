import { t } from "./i18n.js";

export function createDocumentProjectionView(elements, options = {}) {
  const renderers = new Map((options.renderers ?? []).map((renderer) => [
    renderer.protocol,
    renderer,
  ]));
  const resolveDocument = options.resolveDocument ?? (() => {
    throw new Error("No document projection resolver is configured.");
  });
  let requestVersion = 0;
  let selectedKey = null;

  function subjectKey(subject) {
    return subject?.canonicalRef ?? `${subject?.source}:${subject?.address}`;
  }

  function hideRenderers() {
    for (const renderer of renderers.values()) {
      renderer.hide();
    }
  }

  function showGeneric(subject, facet, state, message = "") {
    elements.documentProjectionTitle.textContent = facet?.label ?? "";
    elements.documentProjectionRef.textContent = subjectKey(subject);
    elements.documentProjectionProtocol.textContent = facet?.resolver?.returns ?? "";
    elements.documentProjectionFeedback.dataset.state = state;
    elements.documentProjectionFeedback.textContent = message;
    elements.documentProjectionJson.hidden = true;
    elements.documentProjectionPane.hidden = false;
  }

  function clear() {
    requestVersion += 1;
    selectedKey = null;
    hideRenderers();
    elements.documentProjectionPane.hidden = true;
    elements.documentProjectionFeedback.textContent = "";
    elements.documentProjectionJson.textContent = "";
    elements.documentProjectionJson.hidden = true;
  }

  async function load(subject, facet, version, key) {
    try {
      const document_ = await resolveDocument(subject, facet);
      if (version !== requestVersion || selectedKey !== key) {
        return;
      }
      const renderer = renderers.get(facet.resolver.returns);
      if (renderer) {
        renderer.render(subject, document_);
        elements.documentProjectionPane.hidden = true;
        return;
      }
      elements.documentProjectionFeedback.textContent = "";
      elements.documentProjectionJson.textContent = JSON.stringify(document_, null, 2);
      elements.documentProjectionJson.hidden = false;
    } catch (error) {
      if (version !== requestVersion || selectedKey !== key) {
        return;
      }
      hideRenderers();
      elements.documentProjectionPane.hidden = false;
      elements.documentProjectionFeedback.dataset.state = "error";
      elements.documentProjectionFeedback.textContent = error instanceof Error
        ? error.message
        : t("解析文档 Facet 时发生未知错误。", "An unknown error occurred while resolving the document Facet.");
    }
  }

  function select(subject, facet) {
    clear();
    if (facet?.kind !== "projection" || facet.resolver?.type !== "command") {
      return false;
    }
    const key = `${subjectKey(subject)}#${facet.id}`;
    selectedKey = key;
    showGeneric(subject, facet, "", t("正在解析文档…", "Resolving document…"));
    void load(subject, facet, requestVersion, key);
    return true;
  }

  return { clear, select };
}
