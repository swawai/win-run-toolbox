import { commandRunStatus } from "./command-run-model.js";
import { createRunProjection, RUN_JOURNAL_PROTOCOL } from "./run-projection-model.js";

const sourceLabels = { cli: "CLI", web: "Web" };

export function createRunProjectionRenderer(elements, options = {}) {
  const documentObject = options.document ?? globalThis.document;

  function clear() {
    elements.runProjectionTitle.textContent = "";
    elements.runProjectionRef.textContent = "";
    elements.runProjectionMeta.textContent = "";
    elements.runProjectionState.textContent = "";
    elements.runProjectionError.textContent = "";
    elements.runProjectionError.hidden = true;
    elements.runProjectionOutput.replaceChildren();
    elements.runProjectionTruncated.hidden = true;
  }

  function hide() {
    elements.runProjectionPane.hidden = true;
    clear();
  }

  function formatTime(milliseconds) {
    return new Date(milliseconds).toLocaleString();
  }

  function formatEventTime(milliseconds) {
    const value = new Date(milliseconds);
    const two = (part) => String(part).padStart(2, "0");
    return `${two(value.getHours())}:${two(value.getMinutes())}:${two(value.getSeconds())}.${String(value.getMilliseconds()).padStart(3, "0")}`;
  }

  function appendEvents(events) {
    for (const event of events) {
      const line = documentObject.createElement("span");
      const stamp = formatEventTime(event.timestampUnixMs);
      line.dataset.kind = event.kind;
      if (event.kind === "output") {
        line.dataset.stream = event.stream;
        line.textContent = `[${stamp}] [${event.phase}] ${event.text}`;
      } else {
        line.dataset.state = event.state;
        const amount = event.current === null
          ? ""
          : event.total === null
            ? ` · ${event.current} ${event.unit}`
            : ` · ${event.current}/${event.total} ${event.unit}`;
        line.textContent = `[${stamp}] [${event.phase}] [progress:${event.state}] ${event.message}${amount}`;
      }
      elements.runProjectionOutput.append(line);
    }
  }

  function render(subject, payload) {
    const run = createRunProjection(payload, subject.ref.id);
    const status = commandRunStatus(run);
    elements.runProjectionTitle.textContent = subject.label;
    elements.runProjectionRef.textContent = subject.canonicalRef;
    elements.runProjectionMeta.textContent = `${run.address} · ${sourceLabels[run.source]} · ${formatTime(run.startedAtUnixMs)}`;
    elements.runProjectionState.textContent = status.label;
    elements.runProjectionState.dataset.state = status.tone;
    elements.runProjectionError.textContent = run.error ?? "";
    elements.runProjectionError.hidden = run.error === null;
    elements.runProjectionOutput.replaceChildren();
    appendEvents(run.events);
    elements.runProjectionTruncated.hidden = !run.truncated;
    elements.runProjectionPane.hidden = false;
  }

  return { hide, protocol: RUN_JOURNAL_PROTOCOL, render };
}
