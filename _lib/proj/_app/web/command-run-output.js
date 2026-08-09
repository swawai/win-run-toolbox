const DEFAULT_MAX_OUTPUT_BYTES = 1024 * 1024;
const DEFAULT_MAX_OUTPUT_EVENTS = 4096;
const UTF8_ENCODER = new TextEncoder();

export function createCommandRunOutput(elements, options = {}) {
  const documentObject = options.document ?? globalThis.document;
  const maxOutputBytes = options.maxOutputBytes ?? DEFAULT_MAX_OUTPUT_BYTES;
  const maxOutputEvents = options.maxOutputEvents ?? DEFAULT_MAX_OUTPUT_EVENTS;
  let outputBytes = 0;
  let clientTruncated = false;

  function reset() {
    elements.commandRunOutput.replaceChildren();
    outputBytes = 0;
    clientTruncated = false;
  }

  function append(events, afterCursor) {
    const output = elements.commandRunOutput;
    const pinned = output.scrollHeight - output.scrollTop - output.clientHeight <= 24;
    for (const event of events.filter((event) => event.sequence > afterCursor)) {
      const chunk = documentObject.createElement("span");
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
