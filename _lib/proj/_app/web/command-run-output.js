import { t } from "./i18n.js";

const DEFAULT_MAX_OUTPUT_BYTES = 1024 * 1024;
const DEFAULT_MAX_OUTPUT_EVENTS = 4096;
const UTF8_ENCODER = new TextEncoder();

export function createCommandRunOutput(elements, options = {}) {
  const documentObject = options.document ?? globalThis.document;
  const maxOutputBytes = options.maxOutputBytes ?? DEFAULT_MAX_OUTPUT_BYTES;
  const maxOutputEvents = options.maxOutputEvents ?? DEFAULT_MAX_OUTPUT_EVENTS;
  let outputBytes = 0;
  let clientTruncated = false;
  const progressItems = new Map();

  function reset() {
    elements.commandRunOutput.replaceChildren();
    outputBytes = 0;
    clientTruncated = false;
    progressItems.clear();
  }

  function progressDetail(event) {
    const state = {
      running: t("进行中", "Running"),
      completed: t("已完成", "Completed"),
      failed: t("失败", "Failed"),
    }[event.state];
    if (event.current === null) {
      return state;
    }
    const unit = { bytes: "bytes", items: "items", percent: "%" }[event.unit];
    const amount = event.total === null
      ? `${event.current} ${unit}`
      : `${event.current}/${event.total} ${unit}`;
    return `${state} · ${amount}`;
  }

  function appendProgress(output, event) {
    let item = progressItems.get(event.id);
    if (!item || item.root.parentElement !== output) {
      const root = documentObject.createElement("span");
      const label = documentObject.createElement("span");
      const meter = documentObject.createElement("progress");
      const detail = documentObject.createElement("span");
      root.className = "command-run-progress";
      label.className = "command-run-progress-label";
      meter.className = "command-run-progress-meter";
      detail.className = "command-run-progress-detail";
      root.append(label, meter, detail);
      output.append(root);
      item = { root, label, meter, detail };
      progressItems.set(event.id, item);
    } else {
      outputBytes -= Number(item.root.dataset.outputBytes);
    }
    item.root.dataset.kind = "progress";
    item.root.dataset.progressId = event.id;
    item.root.dataset.state = event.state;
    item.root.dataset.sequence = String(event.sequence);
    item.label.textContent = event.message;
    item.detail.textContent = progressDetail(event);
    item.meter.setAttribute("aria-label", event.message);
    if (event.current !== null && event.total !== null) {
      item.meter.max = event.total;
      item.meter.value = event.current;
      item.meter.setAttribute("max", event.total);
      item.meter.setAttribute("value", event.current);
    } else {
      item.meter.removeAttribute("max");
      item.meter.removeAttribute("value");
    }
    const bytes = UTF8_ENCODER.encode(`${event.message}\n${item.detail.textContent}`).byteLength;
    item.root.dataset.outputBytes = String(bytes);
    outputBytes += bytes;
  }

  function append(events, afterCursor) {
    const output = elements.commandRunOutput;
    const pinned = output.scrollHeight - output.scrollTop - output.clientHeight <= 24;
    for (const event of events.filter((event) => event.sequence > afterCursor)) {
      if (event.kind === "progress") {
        appendProgress(output, event);
        continue;
      }
      const chunk = documentObject.createElement("span");
      chunk.dataset.kind = "output";
      chunk.dataset.stream = event.stream;
      chunk.dataset.outputBytes = String(UTF8_ENCODER.encode(event.text).byteLength);
      chunk.setAttribute("aria-describedby", `command-run-stream-${event.stream}`);
      chunk.textContent = event.text;
      output.append(chunk);
      outputBytes += Number(chunk.dataset.outputBytes);
    }
    while (
      outputBytes > maxOutputBytes
      || output.children.length > maxOutputEvents
    ) {
      const oldest = output.children[0];
      outputBytes -= Number(oldest.dataset.outputBytes);
      if (oldest.dataset.progressId) {
        progressItems.delete(oldest.dataset.progressId);
      }
      oldest.remove();
      clientTruncated = true;
    }
    if (pinned) {
      output.scrollTop = output.scrollHeight;
    }
  }

  function render(serverTruncated) {
    elements.commandRunTruncated.hidden = !serverTruncated && !clientTruncated;
  }

  return { append, render, reset };
}
