import {
  cancelCommandRun,
  CommandRunError,
  readCommandRun,
  startCommandRun,
} from "./command-run-client.js";
import {
  argumentValues,
  commandRunStatus,
  isCommandRunActive,
  isCommandRunSupported,
} from "./command-run-model.js";
import { createCommandRunOutput } from "./command-run-output.js";
import { createCommandRunOperations } from "./command-run-operations.js";
import { t } from "./i18n.js";

export const ACTIVE_COMMAND_RUN_KEY = "swawkit.command-run.active.v1";

function contractError(message) {
  return new CommandRunError(t(
    `命令执行协议无效：${message}`,
    `Invalid command-run protocol: ${message}`,
  ));
}

function isRetryableReadError(error) {
  return error instanceof TypeError
    || (error instanceof CommandRunError && error.status >= 500);
}

function browserStorage() {
  try {
    return globalThis.sessionStorage ?? null;
  } catch {
    return null;
  }
}

export function createCommandRunView(elements, options = {}) {
  const fetchRun = options.fetchRun ?? globalThis.fetch;
  const storage = options.storage ?? browserStorage();
  const documentObject = options.document ?? globalThis.document;
  const setTimer = options.setTimer ?? globalThis.setTimeout;
  const clearTimer = options.clearTimer ?? globalThis.clearTimeout;
  const pollDelay = options.pollDelay ?? 400;
  const onCompleted = options.onCompleted ?? (() => {});
  const commandOutput = createCommandRunOutput(elements, options);
  const commandOperations = createCommandRunOperations(elements, {
    document: documentObject,
    onExecute: (arguments_) => void execute(arguments_),
  });
  let selectedCommand = null;
  let snapshot = null;
  let cursor = 0;
  let timer = null;
  let pollVersion = 0;
  let submitting = false;
  let canceling = false;
  let restoring = false;
  let recoveryUncertain = false;
  let selectedFixedArguments = [];
  let selectedAcceptsTail = true;
  let selectedEditorKey = null;
  let editorKey = null;
  const completedRunIds = new Set();

  function storageGet() {
    try { return storage?.getItem(ACTIVE_COMMAND_RUN_KEY) || ""; } catch { return ""; }
  }

  function storageSet(id) {
    try { storage?.setItem(ACTIVE_COMMAND_RUN_KEY, id); } catch { /* Optional recovery only. */ }
  }

  function storageRemove() {
    try { storage?.removeItem(ACTIVE_COMMAND_RUN_KEY); } catch { /* Optional recovery only. */ }
  }

  function setFeedback(message = "", state = "") {
    elements.commandRunFeedback.textContent = message;
    elements.commandRunFeedback.dataset.state = state;
  }

  function inputs() {
    return elements.commandRunArguments.querySelectorAll(".command-run-argument");
  }

  function updateArgumentRows() {
    const rows = [...inputs()];
    for (const [index, input] of rows.entries()) {
      input.setAttribute("aria-label", t(`参数 ${index + 1}`, `Argument ${index + 1}`));
      input.nextElementSibling?.setAttribute(
        "aria-label",
        t(`删除参数 ${index + 1}`, `Remove argument ${index + 1}`),
      );
    }
    elements.commandRunEmpty.hidden = rows.length !== 0;
  }

  function clearArguments() {
    elements.commandRunArguments.replaceChildren();
    for (const argument of selectedFixedArguments) {
      addArgument(argument, { fixed: true, focus: false });
    }
    editorKey = selectedEditorKey;
    updateArgumentRows();
  }

  function addArgument(value = "", { fixed = false, focus = true } = {}) {
    const row = documentObject.createElement("div");
    const input = documentObject.createElement("input");
    const remove = documentObject.createElement("button");
    row.className = "command-run-argument-row";
    input.className = "command-run-argument";
    input.type = "text";
    input.value = value;
    input.autocomplete = "off";
    input.spellcheck = false;
    input.readOnly = fixed;
    input.dataset.fixed = String(fixed);
    if (fixed) {
      row.dataset.fixed = "true";
    }
    remove.className = "secondary-button command-run-remove";
    remove.type = "button";
    remove.textContent = "−";
    remove.addEventListener("click", () => {
      row.remove();
      updateArgumentRows();
    });
    row.append(input);
    if (!fixed) {
      row.append(remove);
    }
    elements.commandRunArguments.append(row);
    updateArgumentRows();
    if (focus) {
      input.focus();
    }
  }

  function resetOutput() {
    commandOutput.reset();
    cursor = 0;
  }

  function render() {
    const active = isCommandRunActive(snapshot);
    const runnable = isCommandRunSupported(selectedCommand);
    const status = commandRunStatus(snapshot);
    const editorBlocked = active || submitting || restoring || recoveryUncertain;
    elements.commandRunSection.hidden = !runnable && !active;
    elements.commandRunState.textContent = status.label;
    elements.commandRunState.dataset.state = status.tone;
    elements.commandRunSubmit.disabled = !runnable || editorBlocked;
    elements.commandRunAdd.disabled = !runnable || editorBlocked || !selectedAcceptsTail;
    commandOperations.render({ blocked: editorBlocked, runnable });
    for (const input of inputs()) {
      input.disabled = editorBlocked;
      if (input.nextElementSibling) {
        input.nextElementSibling.disabled = editorBlocked;
      }
    }
    elements.commandRunCancel.hidden = !active;
    elements.commandRunActions.hidden = commandOperations.usesOperations() && !active;
    elements.commandRunCancel.disabled = canceling || snapshot?.state === "canceling";
    elements.commandRunResult.hidden = snapshot === null;
    elements.commandRunAddress.textContent = snapshot?.address ?? "";
    elements.commandRunExitCode.textContent = snapshot?.state === "exited"
      ? t(`退出码 ${snapshot.exitCode}`, `Exit code ${snapshot.exitCode}`)
      : "";
    commandOutput.render(snapshot?.truncated === true);
  }

  function rememberRun() {
    if (isCommandRunActive(snapshot)) {
      storageSet(snapshot.id);
    } else {
      storageRemove();
    }
  }

  function adoptSnapshot(next, { reset = false } = {}) {
    if (snapshot && (snapshot.id !== next.id || snapshot.address !== next.address)) {
      throw contractError(t(
        "轮询响应与当前执行不匹配。",
        "The poll response does not match the current run.",
      ));
    }
    if (reset) {
      resetOutput();
    }
    commandOutput.append(next.events, cursor);
    cursor = next.nextCursor;
    snapshot = { ...next, events: [] };
    rememberRun();
    if (!isCommandRunActive(snapshot) && selectedEditorKey !== editorKey) {
      clearArguments();
    }
    if (snapshot.state === "failed" && snapshot.error) {
      setFeedback(snapshot.error, "error");
    } else if (snapshot.state === "canceling") {
      setFeedback(t("Host 正在终止进程树…", "Host is terminating the process tree…"));
    } else {
      setFeedback();
    }
    render();
    if (
      snapshot.state === "exited"
      && snapshot.exitCode === 0
      && !completedRunIds.has(snapshot.id)
    ) {
      completedRunIds.add(snapshot.id);
      onCompleted(snapshot);
    }
  }

  function stopTimer() {
    if (timer !== null) {
      clearTimer(timer);
      timer = null;
    }
  }

  function schedulePoll(version, delay = pollDelay) {
    stopTimer();
    timer = setTimer(() => {
      timer = null;
      void poll(version);
    }, delay);
  }

  async function poll(version) {
    const expected = snapshot;
    if (!expected || version !== pollVersion || !isCommandRunActive(expected)) {
      return;
    }
    try {
      const next = await readCommandRun(expected.id, cursor, fetchRun);
      if (version !== pollVersion) {
        return;
      }
      adoptSnapshot(next);
      if (isCommandRunActive(snapshot)) {
        schedulePoll(version);
      }
    } catch (error) {
      if (version !== pollVersion) {
        return;
      }
      if (error instanceof CommandRunError && error.status === 404) {
        adoptSnapshot({
          ...expected,
          state: "failed",
          exitCode: null,
          error: t("Host 中的执行记录已失效。", "The run record in Host is no longer available."),
          events: [],
        });
        return;
      }
      setFeedback(
        error instanceof Error
          ? t(`读取执行状态失败：${error.message}`, `Failed to read run state: ${error.message}`)
          : t("读取执行状态失败。", "Failed to read run state."),
        "error",
      );
      if (isRetryableReadError(error)) {
        schedulePoll(version, Math.max(pollDelay, 1000));
      } else {
        setFeedback(
          error instanceof Error
            ? t(
              `无法继续确认执行状态：${error.message}。请刷新页面后重新确认。`,
              `Cannot continue checking run state: ${error.message}. Reload the page to verify it again.`,
            )
            : t(
              "无法继续确认执行状态。请刷新页面后重新确认。",
              "Cannot continue checking run state. Reload the page to verify it again.",
            ),
          "error",
        );
      }
    }
  }

  async function execute(explicitArguments) {
    const command = selectedCommand;
    if (
      !isCommandRunSupported(command)
      || isCommandRunActive(snapshot)
      || submitting
      || restoring
      || recoveryUncertain
    ) {
      return;
    }
    submitting = true;
    setFeedback(t("正在启动…", "Starting…"));
    render();
    try {
      const arguments_ = explicitArguments === undefined
        ? argumentValues(inputs())
        : [...explicitArguments];
      const next = await startCommandRun(command.address, arguments_, fetchRun);
      if (next.address !== command.address) {
        throw contractError(t(
          "创建响应返回了不同的命令地址。",
          "The create response returned a different command address.",
        ));
      }
      pollVersion += 1;
      stopTimer();
      snapshot = null;
      adoptSnapshot(next, { reset: true });
      if (isCommandRunActive(snapshot)) {
        schedulePoll(pollVersion);
      }
    } catch (error) {
      setFeedback(
        error instanceof Error
          ? error.message
          : t("启动命令时发生未知错误。", "An unknown error occurred while starting the command."),
        "error",
      );
    } finally {
      submitting = false;
      render();
    }
  }

  async function cancel() {
    const expected = snapshot;
    if (!isCommandRunActive(expected) || canceling) {
      return;
    }
    const version = ++pollVersion;
    stopTimer();
    canceling = true;
    render();
    try {
      await cancelCommandRun(expected.id, fetchRun);
      if (snapshot?.id === expected.id && isCommandRunActive(snapshot)) {
        snapshot = { ...snapshot, state: "canceling" };
        rememberRun();
        setFeedback(t("Host 正在终止进程树…", "Host is terminating the process tree…"));
        render();
        schedulePoll(version, 0);
      }
    } catch (error) {
      setFeedback(
        error instanceof Error
          ? error.message
          : t("终止命令时发生未知错误。", "An unknown error occurred while canceling the command."),
        "error",
      );
      if (snapshot?.id === expected.id && isCommandRunActive(snapshot)) {
        schedulePoll(version);
      }
    } finally {
      canceling = false;
      render();
    }
  }

  function scheduleRestore(id, version, delay = Math.max(pollDelay, 1000)) {
    stopTimer();
    timer = setTimer(() => {
      timer = null;
      void restoreAttempt(id, version);
    }, delay);
  }

  async function restoreAttempt(id, version) {
    try {
      const next = await readCommandRun(id, 0, fetchRun);
      if (version !== pollVersion) {
        return;
      }
      restoring = false;
      recoveryUncertain = false;
      snapshot = null;
      adoptSnapshot(next, { reset: true });
      if (isCommandRunActive(snapshot)) {
        schedulePoll(version);
      }
    } catch (error) {
      if (version !== pollVersion) {
        return;
      }
      if (error instanceof CommandRunError && error.status === 404) {
        storageRemove();
        restoring = false;
        recoveryUncertain = false;
        setFeedback(t("上次执行记录已失效。", "The previous run record is no longer available."), "error");
        render();
        return;
      }
      if (isRetryableReadError(error)) {
        setFeedback(
          error instanceof Error
            ? t(
              `恢复执行暂时失败：${error.message}。Host 恢复后将自动重试。`,
              `Temporarily failed to restore the run: ${error.message}. It will retry when Host recovers.`,
            )
            : t(
              "恢复执行暂时失败。Host 恢复后将自动重试。",
              "Temporarily failed to restore the run. It will retry when Host recovers.",
            ),
          "error",
        );
        render();
        scheduleRestore(id, version);
        return;
      }
      restoring = false;
      recoveryUncertain = true;
      setFeedback(
        error instanceof Error
          ? t(
            `无法确认上次执行：${error.message}。请刷新页面后重新确认。`,
            `Cannot verify the previous run: ${error.message}. Reload the page to verify it again.`,
          )
          : t(
            "无法确认上次执行。请刷新页面后重新确认。",
            "Cannot verify the previous run. Reload the page to verify it again.",
          ),
        "error",
      );
      render();
    }
  }

  async function restore() {
    const id = storageGet();
    if (!id || restoring || recoveryUncertain) {
      return;
    }
    restoring = true;
    recoveryUncertain = false;
    const version = ++pollVersion;
    setFeedback(t("正在恢复上次执行…", "Restoring the previous run…"));
    render();
    await restoreAttempt(id, version);
  }

  function select(
    command,
    {
      acceptsTail = true,
      arguments: arguments_ = [],
      confirmation = null,
      key = null,
      label = null,
      useOperations = true,
    } = {},
  ) {
    const previous = selectedEditorKey;
    selectedCommand = command ?? null;
    selectedFixedArguments = [...arguments_];
    selectedAcceptsTail = acceptsTail;
    selectedEditorKey = selectedCommand
      ? key ?? `${selectedCommand.address}\u0000${JSON.stringify({
        acceptsTail: selectedAcceptsTail,
        arguments: selectedFixedArguments,
        confirmation,
      })}`
      : null;
    const confirmationCommand = selectedCommand && confirmation
      ? {
        ...selectedCommand,
        runOperations: [{
          arguments: selectedFixedArguments,
          confirmation,
          id: key ?? "invoke",
          label: label ?? selectedCommand.address,
        }],
      }
      : null;
    commandOperations.select(
      confirmationCommand ?? (useOperations ? selectedCommand : null),
    );
    if (!isCommandRunActive(snapshot) && previous !== selectedEditorKey) {
      clearArguments();
    }
    render();
  }

  elements.commandRunAdd.addEventListener("click", () => addArgument());
  elements.commandRunForm.addEventListener("submit", (event) => {
    event.preventDefault();
    void execute();
  });
  elements.commandRunCancel.addEventListener("click", () => void cancel());
  clearArguments();
  render();

  return { cancel, execute, restore, select };
}
