import { createContextProjection } from "./context-projection-model.js";

export const CONTEXT_PROTOCOL = "swawkit.context/v1";

export function createContextProjectionRenderer(elements) {
  function renderList(list, empty, values, renderValue) {
    list.replaceChildren(...values.map((value) => {
      const item = document.createElement("li");
      renderValue(item, value);
      return item;
    }));
    empty.hidden = values.length !== 0;
  }

  function clear() {
    elements.contextProjectionCommands.replaceChildren();
    elements.contextProjectionNotes.replaceChildren();
    elements.contextProjectionPrompt.textContent = "";
    elements.contextProjectionCommandEmpty.hidden = false;
    elements.contextProjectionNotesEmpty.hidden = false;
    elements.contextProjectionPrompt.hidden = true;
    elements.contextProjectionPromptEmpty.hidden = false;
  }

  function hide() {
    elements.contextProjectionPane.hidden = true;
    clear();
  }

  function render(subject, payload) {
    const document_ = createContextProjection(payload, subject.ref.id);
    elements.contextProjectionTitle.textContent = subject.label;
    elements.contextProjectionRef.textContent = subject.canonicalRef;
    elements.contextProjectionSummary.textContent = subject.summary;
    renderList(
      elements.contextProjectionCommands,
      elements.contextProjectionCommandEmpty,
      document_.commands,
      (item, command) => {
        const address = document.createElement("code");
        address.textContent = command.address;
        const source = document.createElement("span");
        source.textContent = command.source;
        item.append(address, source);
      },
    );
    renderList(
      elements.contextProjectionNotes,
      elements.contextProjectionNotesEmpty,
      document_.notes,
      (item, note) => { item.textContent = note; },
    );
    elements.contextProjectionPrompt.textContent = document_.prompt;
    elements.contextProjectionPrompt.hidden = document_.prompt.length === 0;
    elements.contextProjectionPromptEmpty.hidden = document_.prompt.length !== 0;
    elements.contextProjectionPane.hidden = false;
  }

  return { hide, protocol: CONTEXT_PROTOCOL, render };
}
