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

export const ACTIVE_COMMAND_RUN_KEY = "swawkit.command-run.active.v1";

function contractError(message) {
  return new CommandRunError(`命令执行协议无效：${message}`);
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
  let editorAddress = null;

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
      input.setAttribute("aria-label", `参数 ${index + 1}`);
      input.nextElementSibling?.setAttribute("aria-label", `删除参数 ${index + 1}`);
    }
    elements.commandRunEmpty.hidden = rows.length !== 0;
  }

  function clearArguments() {
    elements.commandRunArguments.replaceChildren();
    editorAddress = selectedCommand?.address ?? null;
    updateArgumentRows();
  }

  function addArgument(value = "") {
    const row = documentObject.createElement("div");
    const input = documentObject.createElement("input");
    const remove = documentObject.createElement("button");
    row.className = "command-run-argument-row";
    input.className = "command-run-argument";
    input.type = "text";
    input.value = value;
    input.autocomplete = "off";
    input.spellcheck = false;
    remove.className = "secondary-button command-run-remove";
    remove.type = "button";
    remove.textContent = "−";
    remove.addEventListener("click", () => {
      row.remove();
      updateArgumentRows();
    });
    row.append(input, remove);
    elements.commandRunArguments.append(row);
    updateArgumentRows();
    input.focus();
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
    elements.commandRunAdd.disabled = !runnable || editorBlocked;
    commandOperations.render({ blocked: editorBlocked, runnable });
    for (const input of inputs()) {
      input.disabled = editorBlocked;
      input.nextElementSibling.disabled = editorBlocked;
    }
    elements.commandRunCancel.hidden = !active;
    elements.commandRunActions.hidden = commandOperations.usesOperations() && !active;
    elements.commandRunCancel.disabled = canceling || snapshot?.state === "canceling";
    elements.commandRunResult.hidden = snapshot === null;
    elements.commandRunAddress.textContent = snapshot?.address ?? "";
    elements.commandRunExitCode.textContent = snapshot?.state === "exited"
      ? `退出码 ${snapshot.exitCode}`
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
      throw contractError("轮询响应与当前执行不匹配。");
    }
    if (reset) {
      resetOutput();
    }
    commandOutput.append(next.events, cursor);
    cursor = next.nextCursor;
    snapshot = { ...next, events: [] };
    rememberRun();
    if (!isCommandRunActive(snapshot) && selectedCommand?.address !== editorAddress) {
      clearArguments();
    }
    if (snapshot.state === "failed" && snapshot.error) {
      setFeedback(snapshot.error, "error");
    } else if (snapshot.state === "canceling") {
      setFeedback("Host 正在终止进程树…");
    } else {
      setFeedback();
    }
    render();
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
          error: "Host 中的执行记录已失效。",
          events: [],
        });
        return;
      }
      setFeedback(
        error instanceof Error ? `读取执行状态失败：${error.message}` : "读取执行状态失败。",
        "error",
      );
      if (isRetryableReadError(error)) {
        schedulePoll(version, Math.max(pollDelay, 1000));
      } else {
        setFeedback(
          error instanceof Error
            ? `无法继续确认执行状态：${error.message}。请刷新页面后重新确认。`
            : "无法继续确认执行状态。请刷新页面后重新确认。",
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
    setFeedback("正在启动…");
    render();
    try {
      const arguments_ = explicitArguments === undefined
        ? argumentValues(inputs())
        : [...explicitArguments];
      const next = await startCommandRun(command.address, arguments_, fetchRun);
      if (next.address !== command.address) {
        throw contractError("创建响应返回了不同的命令地址。");
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
        error instanceof Error ? error.message : "启动命令时发生未知错误。",
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
        setFeedback("Host 正在终止进程树…");
        render();
        schedulePoll(version, 0);
      }
    } catch (error) {
      setFeedback(
        error instanceof Error ? error.message : "终止命令时发生未知错误。",
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
        setFeedback("上次执行记录已失效。", "error");
        render();
        return;
      }
      if (isRetryableReadError(error)) {
        setFeedback(
          error instanceof Error
            ? `恢复执行暂时失败：${error.message}。Host 恢复后将自动重试。`
            : "恢复执行暂时失败。Host 恢复后将自动重试。",
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
          ? `无法确认上次执行：${error.message}。请刷新页面后重新确认。`
          : "无法确认上次执行。请刷新页面后重新确认。",
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
    setFeedback("正在恢复上次执行…");
    render();
    await restoreAttempt(id, version);
  }

  function select(command) {
    const previous = selectedCommand?.address ?? null;
    selectedCommand = command ?? null;
    commandOperations.select(selectedCommand);
    if (!isCommandRunActive(snapshot) && previous !== selectedCommand?.address) {
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
