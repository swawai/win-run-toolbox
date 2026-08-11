const ACTIVE_STATES = new Set(["running", "canceling"]);
const COMMAND_RUN_SOURCES = new Set(["kernel", "action"]);

export function argumentValues(inputs) {
  return [...inputs].map((input) => String(input.value));
}

export function isCommandRunActive(snapshot) {
  return snapshot !== null && ACTIVE_STATES.has(snapshot.state);
}

export function isCommandRunSupported(command) {
  return command?.runnable === true
    && isCommandJournalSupported(command);
}

export function isCommandJournalSupported(command) {
  return typeof command?.address === "string"
    && command.address.length > 0
    && COMMAND_RUN_SOURCES.has(command.source);
}

export function commandJournalLocator(command) {
  return isCommandJournalSupported(command)
    ? `${command.source}/${command.address}`
    : null;
}

export function commandRunStatus(snapshot) {
  switch (snapshot?.state) {
    case "running": return { label: "运行中", tone: "" };
    case "canceling": return { label: "正在终止…", tone: "warning" };
    case "exited":
      return snapshot.exitCode === 0
        ? { label: "执行成功", tone: "success" }
        : { label: "执行失败", tone: "error" };
    case "canceled": return { label: "已终止", tone: "warning" };
    case "failed": return { label: "执行异常", tone: "error" };
    default: return { label: "", tone: "" };
  }
}
