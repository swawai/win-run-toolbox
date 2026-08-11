import {
  readCommandJournal,
  readCommandJournalHistory,
} from "./command-journal-client.js";
import { commandRunStatus, isCommandRunSupported } from "./command-run-model.js";

const sourceLabels = { cli: "CLI", web: "Web" };

function journalStatus(value) {
  if (value?.state === "running") {
    return { label: "运行中或未完成", tone: "warning" };
  }
  return commandRunStatus(value);
}

export function createCommandJournalView(elements, options = {}) {
  const fetchJournal = options.fetchJournal ?? globalThis.fetch;
  const documentObject = options.document ?? globalThis.document;
  const setTimer = options.setTimer ?? globalThis.setTimeout;
  const clearTimer = options.clearTimer ?? globalThis.clearTimeout;
  const pollDelay = options.pollDelay ?? 800;
  let command = null;
  let active = false;
  let history = [];
  let selectedId = null;
  let journal = null;
  let cursor = 0;
  let version = 0;
  let timer = null;
  let loading = false;

  function stopTimer() {
    if (timer !== null) {
      clearTimer(timer);
      timer = null;
    }
  }

  function feedback(message = "", tone = "") {
    elements.commandJournalFeedback.textContent = message;
    elements.commandJournalFeedback.dataset.state = tone;
  }

  function formatTime(milliseconds) {
    return new Date(milliseconds).toLocaleString();
  }

  function formatEventTime(milliseconds) {
    const value = new Date(milliseconds);
    const two = (part) => String(part).padStart(2, "0");
    return `${two(value.getHours())}:${two(value.getMinutes())}:${two(value.getSeconds())}.${String(value.getMilliseconds()).padStart(3, "0")}`;
  }

  function renderHistory() {
    elements.commandJournalList.replaceChildren();
    elements.commandJournalEmpty.hidden = history.length !== 0 || loading;
    for (const run of history) {
      const item = documentObject.createElement("li");
      const button = documentObject.createElement("button");
      const heading = documentObject.createElement("span");
      const status = documentObject.createElement("strong");
      const time = documentObject.createElement("time");
      const meta = documentObject.createElement("small");
      const presentation = journalStatus(run);
      button.type = "button";
      button.disabled = loading;
      button.dataset.selected = String(run.id === selectedId);
      button.addEventListener("click", () => void loadRun(run.id, { reset: true }));
      status.textContent = presentation.label;
      status.dataset.state = presentation.tone;
      time.textContent = formatTime(run.startedAtUnixMs);
      heading.append(status, time);
      meta.textContent = `${sourceLabels[run.source]} · ${run.eventCount} 条事件`;
      button.append(heading, meta);
      item.append(button);
      elements.commandJournalList.append(item);
    }
  }

  function resetOutput() {
    elements.commandJournalOutput.replaceChildren();
    elements.commandJournalDetail.hidden = true;
    elements.commandJournalTruncated.hidden = true;
    journal = null;
    cursor = 0;
  }

  function appendEvents(events) {
    for (const event of events) {
      const line = documentObject.createElement("span");
      const stamp = formatEventTime(event.timestampUnixMs);
      line.dataset.stream = event.stream;
      line.textContent = `[${stamp}] [${event.phase}] ${event.text}`;
      elements.commandJournalOutput.append(line);
    }
    if (events.length > 0) {
      elements.commandJournalOutput.scrollTop = elements.commandJournalOutput.scrollHeight;
    }
  }

  function renderJournal() {
    elements.commandJournalDetail.hidden = journal === null;
    if (!journal) {
      return;
    }
    const presentation = journalStatus(journal);
    elements.commandJournalState.textContent = presentation.label;
    elements.commandJournalState.dataset.state = presentation.tone;
    elements.commandJournalMeta.textContent = `${sourceLabels[journal.source]} · ${formatTime(journal.startedAtUnixMs)}`;
    elements.commandJournalTruncated.hidden = !journal.truncated;
  }

  function schedulePoll(expectedVersion) {
    stopTimer();
    timer = setTimer(() => {
      timer = null;
      void poll(expectedVersion);
    }, pollDelay);
  }

  async function poll(expectedVersion) {
    if (!active || !journal || journal.state !== "running" || expectedVersion !== version) {
      return;
    }
    await loadRun(journal.id, { reset: false, expectedVersion });
  }

  async function loadRun(id, { reset, expectedVersion = ++version }) {
    stopTimer();
    selectedId = id;
    if (reset) {
      resetOutput();
      renderHistory();
    }
    feedback("正在读取日志…");
    try {
      const next = await readCommandJournal(
        command.address,
        id,
        reset ? 0 : cursor,
        fetchJournal,
      );
      if (expectedVersion !== version || !active || selectedId !== id) {
        return;
      }
      appendEvents(next.events);
      cursor = next.nextCursor;
      journal = {
        ...next,
        events: [],
        truncated: journal?.truncated === true || next.truncated,
      };
      const summary = history.find((run) => run.id === id);
      if (summary) {
        Object.assign(summary, {
          state: journal.state,
          finishedAtUnixMs: journal.finishedAtUnixMs,
          exitCode: journal.exitCode,
          error: journal.error,
          eventCount: journal.nextCursor,
          truncated: journal.truncated,
        });
        renderHistory();
      }
      feedback();
      renderJournal();
      if (journal.state === "running") {
        schedulePoll(expectedVersion);
      }
    } catch (error) {
      if (expectedVersion === version) {
        feedback(error instanceof Error ? error.message : "读取命令日志失败。", "error");
      }
    }
  }

  async function refresh() {
    if (!active || !isCommandRunSupported(command) || loading) {
      return;
    }
    const expectedVersion = ++version;
    stopTimer();
    loading = true;
    elements.commandJournalRefresh.disabled = true;
    renderHistory();
    feedback("正在读取历史运行…");
    try {
      const next = await readCommandJournalHistory(command.address, fetchJournal);
      if (expectedVersion !== version || !active) {
        return;
      }
      history = next.runs;
      const retained = history.some((run) => run.id === selectedId);
      selectedId = retained ? selectedId : history[0]?.id ?? null;
      renderHistory();
      feedback();
      if (selectedId) {
        await loadRun(selectedId, { reset: true, expectedVersion });
      } else {
        resetOutput();
      }
    } catch (error) {
      if (expectedVersion === version) {
        feedback(error instanceof Error ? error.message : "读取历史运行失败。", "error");
      }
    } finally {
      if (expectedVersion === version) {
        loading = false;
        elements.commandJournalRefresh.disabled = false;
        renderHistory();
      }
    }
  }

  function select(nextCommand, options = {}) {
    const nextActive = options.active === true;
    const entering = !active && nextActive;
    const changed = command?.address !== nextCommand?.address;
    command = nextCommand ?? null;
    active = nextActive;
    elements.commandJournalAddress.textContent = command?.address ?? "";
    if (changed) {
      loading = false;
      elements.commandJournalRefresh.disabled = false;
      history = [];
      selectedId = null;
      resetOutput();
      renderHistory();
    }
    if (entering && !changed) {
      selectedId = null;
      resetOutput();
      renderHistory();
    }
    if (!active) {
      loading = false;
      elements.commandJournalRefresh.disabled = false;
      version += 1;
      stopTimer();
      return;
    }
    void refresh();
  }

  elements.commandJournalRefresh.addEventListener("click", () => void refresh());
  renderHistory();
  resetOutput();
  return { refresh, select };
}
