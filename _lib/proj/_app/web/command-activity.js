import { isCommandRunSupported } from "./command-run-model.js";

const DEFAULT_ACTIVITY = "overview";
const PROFILE_SETTER_HANDLER = "entry.profile.set";
const activityPresentation = {
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
  if (!command || command.handler === PROFILE_SETTER_HANDLER) {
    return [];
  }
  const activities = ["overview", "help"];
  if (isCommandRunSupported(command)) {
    activities.push("run");
  }
  return activities;
}

export function createCommandActivityView(elements) {
  const panes = new Map([
    ["overview", elements.commandDetail],
    ["help", elements.commandHelpActivity],
    ["run", elements.commandRunActivity],
  ]);
  let available = [];
  let selected = DEFAULT_ACTIVITY;
  let selectedAddress = null;

  function render() {
    const active = new Set(available);
    elements.commandWorkspace.hidden = active.size === 0;
    for (const [name, pane] of panes) {
      pane.hidden = name !== selected || !active.has(name);
    }
  }

  function selectCommand(command, { activity = DEFAULT_ACTIVITY } = {}) {
    available = commandActivities(command);
    selected = available.includes(activity) ? activity : DEFAULT_ACTIVITY;
    selectedAddress = command?.address ?? null;
    render();
  }

  function items(command) {
    return commandActivities(command).map((name) => ({
      name,
      ...activityPresentation[name],
      selected: command.address === selectedAddress && name === selected,
    }));
  }
  render();

  return { items, selectCommand };
}
