import { isCommandRunSupported } from "./command-run-model.js";

const PROFILE_SETTER_HANDLER = "entry.profile.set";
const viewPresentation = {
  children: {
    icon: "⌑",
    label: "子命令",
    summary: "继续浏览下一级命令",
  },
  edit: {
    icon: "✎",
    label: "设置",
    summary: "修改并保存变量值",
  },
  overview: {
    icon: "i",
    label: "概览",
    summary: "查看调用与命令属性",
  },
  help: {
    icon: "?",
    label: "帮助",
    summary: "阅读命令说明",
  },
  run: {
    icon: "▶",
    label: "执行",
    summary: "设置参数并启动命令",
  },
};

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
  return names.map((name) => ({ name, ...viewPresentation[name] }));
}

export function defaultCommandView(command, options = {}) {
  const views = commandViews(command, options);
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
  ]);
  const workspaceViews = new Set(["overview", "help", "run"]);
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
