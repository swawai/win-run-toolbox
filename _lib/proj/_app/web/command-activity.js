import {
  isCommandJournalSupported,
  isCommandRunSupported,
} from "./command-run-model.js";
import { t } from "./i18n.js";

const PROFILE_SETTER_HANDLER = "entry.profile.set";
const RUNTIME_STATUS_HANDLER = "runtime.status";
function viewPresentation() {
  return {
    children: {
      icon: "⌑",
      label: t("子命令", "Subcommands"),
      summary: t("继续浏览下一级命令", "Browse the next command level"),
    },
    edit: {
      icon: "✎",
      label: t("设置", "Setting"),
      summary: t("修改并保存配置值", "Edit and save a configuration value"),
    },
    overview: {
      icon: "i",
      label: t("概览", "Overview"),
      summary: t("查看调用与命令属性", "Inspect invocation and command properties"),
    },
    help: {
      icon: "?",
      label: t("帮助", "Help"),
      summary: t("阅读命令说明", "Read command help"),
    },
    run: {
      icon: "▶",
      label: t("执行", "Run"),
      summary: t("设置参数并启动命令", "Set arguments and start the command"),
    },
    logs: {
      icon: "≡",
      label: t("日志", "Logs"),
      summary: t("查看 CLI 与 Web 历史运行", "View CLI and Web run history"),
    },
  };
}

export function commandActivities(command) {
  if (!command) {
    return [];
  }
  if (command.handler === PROFILE_SETTER_HANDLER) {
    return ["edit", "overview", "help"];
  }
  const activities = ["overview", "help"];
  if (isCommandRunSupported(command)) {
    activities.push("run");
  }
  if (isCommandJournalSupported(command)) {
    activities.push("logs");
  }
  return activities;
}

export function commandViews(command, { hasChildren = false } = {}) {
  if (!command) {
    return [];
  }
  const names = [
    ...(hasChildren ? ["children"] : []),
    ...commandActivities(command),
  ];
  const presentation = viewPresentation();
  return names.map((name) => ({ name, ...presentation[name] }));
}

export function defaultCommandView(command, options = {}) {
  const views = commandViews(command, options);
  if (command?.handler === RUNTIME_STATUS_HANDLER) {
    return views.find((view) => view.name === "overview")?.name ?? null;
  }
  return views.find((view) => view.name === "children")?.name
    ?? views[0]?.name
    ?? null;
}

export function createCommandActivityView(elements) {
  const panes = new Map([
    ["edit", elements.entryProfileDetail],
    ["overview", elements.commandDetail],
    ["help", elements.commandHelpActivity],
    ["run", elements.commandRunActivity],
    ["logs", elements.commandJournalActivity],
  ]);
  const workspaceViews = new Set(["overview", "help", "run", "logs"]);
  let available = [];
  let selected = null;
  let selectedAddress = null;

  function render() {
    const active = new Set(available.filter((name) => name !== "children"));
    elements.commandWorkspace.hidden = !workspaceViews.has(selected)
      || !active.has(selected);
    for (const [name, pane] of panes) {
      pane.hidden = name !== selected || !active.has(name);
    }
  }

  function selectCommand(command, { hasChildren = false, view = null } = {}) {
    const views = commandViews(command, { hasChildren });
    available = views.map((item) => item.name);
    const defaultView = defaultCommandView(command, { hasChildren });
    selected = available.includes(view) ? view : defaultView;
    selectedAddress = command?.address ?? null;
    render();
    return { defaultView, view: selected };
  }

  function items(command, options = {}) {
    return commandViews(command, options).map((view) => ({
      ...view,
      selected: command.address === selectedAddress && view.name === selected,
    }));
  }
  render();

  return { items, selectCommand };
}
