import {
  openCommandJournalDirectory,
  readCommandJournal,
  readCommandJournalHistory,
} from "./command-journal-client.js";
import {
  commandJournalLocator,
  commandRunStatus,
  isCommandJournalSupported,
} from "./command-run-model.js";
import { t } from "./i18n.js";

const sourceLabels = { cli: "CLI", web: "Web" };

function journalStatus(value) {
  if (value?.state === "running") {
    return { label: t("运行中或未完成", "Running or incomplete"), tone: "warning" };
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
      const summary = documentObject.createElement("span");
      const meta = documentObject.createElement("small");
      const presentation = journalStatus(run);
      item.className = "command-journal-record";
      button.type = "button";
      button.className = "command-journal-select";
      button.disabled = loading;
      button.dataset.selected = String(run.id === selectedId);
      button.addEventListener("click", () => void loadRun(run.id, { reset: true }));
      status.textContent = presentation.label;
      status.dataset.state = presentation.tone;
      time.textContent = formatTime(run.startedAtUnixMs);
      heading.append(time);
      meta.textContent = t(
        `${sourceLabels[run.source]} · ${run.eventCount} 条事件`,
        `${sourceLabels[run.source]} · ${run.eventCount} events`,
      );
      summary.className = "command-journal-record-summary";
      summary.append(meta, status);
      button.append(heading, summary);
      item.append(button);
      elements.commandJournalList.append(item);
    }
  }

  async function openDirectory() {
    const locator = commandJournalLocator(command);
    const id = journal?.id;
    if (!active || !locator || !id) {
      return;
    }
    elements.commandJournalOpen.disabled = true;
    elements.commandJournalDirectoryFeedback.textContent = t(
      "正在打开日志目录…",
      "Opening the log folder…",
    );
    elements.commandJournalDirectoryFeedback.dataset.state = "";
    try {
      await openCommandJournalDirectory(locator, id, fetchJournal);
      if (active && commandJournalLocator(command) === locator && journal?.id === id) {
        elements.commandJournalDirectoryFeedback.textContent = t(
          "已在资源管理器中打开日志目录。",
          "Opened the log folder in File Explorer.",
        );
        elements.commandJournalDirectoryFeedback.dataset.state = "success";
      }
    } catch (error) {
      if (active && commandJournalLocator(command) === locator && journal?.id === id) {
        elements.commandJournalDirectoryFeedback.textContent = error instanceof Error
          ? error.message
          : t("打开日志目录失败。", "Failed to open the log folder.");
        elements.commandJournalDirectoryFeedback.dataset.state = "error";
      }
    } finally {
      if (active && commandJournalLocator(command) === locator && journal?.id === id) {
        elements.commandJournalOpen.disabled = false;
      }
    }
  }

  function resetOutput() {
    elements.commandJournalOutput.replaceChildren();
    elements.commandJournalDetail.hidden = true;
    elements.commandJournalDetailEmpty.hidden = false;
    elements.commandJournalError.hidden = true;
    elements.commandJournalError.textContent = "";
    elements.commandJournalTruncated.hidden = true;
    elements.commandJournalDirectoryFeedback.textContent = "";
    elements.commandJournalDirectoryFeedback.dataset.state = "";
    elements.commandJournalOpen.disabled = true;
    journal = null;
    cursor = 0;
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
      elements.commandJournalOutput.append(line);
    }
    if (events.length > 0) {
      elements.commandJournalOutput.scrollTop = elements.commandJournalOutput.scrollHeight;
    }
  }

  function renderJournal() {
    elements.commandJournalDetail.hidden = journal === null;
    elements.commandJournalDetailEmpty.hidden = journal !== null;
    if (!journal) {
      return;
    }
    const presentation = journalStatus(journal);
    elements.commandJournalState.textContent = presentation.label;
    elements.commandJournalState.dataset.state = presentation.tone;
    elements.commandJournalMeta.textContent = `${sourceLabels[journal.source]} · ${formatTime(journal.startedAtUnixMs)}`;
    elements.commandJournalError.textContent = journal.error ?? "";
    elements.commandJournalError.hidden = journal.error === null;
    elements.commandJournalOpen.disabled = false;
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
    feedback(t("正在读取日志…", "Loading log…"));
    try {
      const next = await readCommandJournal(
        commandJournalLocator(command),
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
        feedback(
          error instanceof Error ? error.message : t("读取命令日志失败。", "Failed to load command log."),
          "error",
        );
      }
    }
  }

  async function refresh() {
    if (!active || !isCommandJournalSupported(command) || loading) {
      return;
    }
    const expectedVersion = ++version;
    stopTimer();
    loading = true;
    elements.commandJournalRefresh.disabled = true;
    renderHistory();
    feedback(t("正在读取历史运行…", "Loading run history…"));
    try {
      const next = await readCommandJournalHistory(commandJournalLocator(command), fetchJournal);
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
        feedback(
          error instanceof Error ? error.message : t("读取历史运行失败。", "Failed to load run history."),
          "error",
        );
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
    const changed = commandJournalLocator(command) !== commandJournalLocator(nextCommand);
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
  elements.commandJournalOpen.addEventListener("click", () => void openDirectory());
  renderHistory();
  resetOutput();
  return { refresh, select };
}
