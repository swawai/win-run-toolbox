import { t } from "./i18n.js";

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
    && typeof command?.address === "string"
    && command.address.length > 0
    && COMMAND_RUN_SOURCES.has(command.source);
}

export function commandRunStatus(snapshot) {
  switch (snapshot?.state) {
    case "running": return { label: t("运行中", "Running"), tone: "" };
    case "canceling": return { label: t("正在终止…", "Canceling…"), tone: "warning" };
    case "exited":
      return snapshot.exitCode === 0
        ? { label: t("执行成功", "Succeeded"), tone: "success" }
        : { label: t("执行失败", "Failed"), tone: "error" };
    case "canceled": return { label: t("已终止", "Canceled"), tone: "warning" };
    case "failed": return { label: t("执行异常", "Run error"), tone: "error" };
    default: return { label: "", tone: "" };
  }
}
