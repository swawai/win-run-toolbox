import { isCommandRunSupported } from "./command-run-model.js";

const DEFAULT_ACTIVITY = "overview";

export function commandActivities(command) {
  if (!command) {
    return [];
  }
  const activities = ["overview", "help"];
  if (isCommandRunSupported(command)) {
    activities.push("run");
  }
  return activities;
}

export function createCommandActivityView(elements) {
  const buttons = [...elements.commandActivities.querySelectorAll(
    "[data-command-activity]",
  )];
  const panes = new Map([
    ["overview", elements.commandDetail],
    ["help", elements.commandHelpActivity],
    ["run", elements.commandRunActivity],
  ]);
  let available = [];
  let selected = DEFAULT_ACTIVITY;

  function render() {
    const active = new Set(available);
    elements.commandWorkspace.hidden = active.size === 0;
    for (const button of buttons) {
      const name = button.dataset.commandActivity;
      button.hidden = !active.has(name);
      if (name === selected && active.has(name)) {
        button.setAttribute("aria-current", "page");
      } else {
        button.removeAttribute("aria-current");
      }
    }
    for (const [name, pane] of panes) {
      pane.hidden = name !== selected || !active.has(name);
    }
  }

  function select(name, { focus = false } = {}) {
    if (!available.includes(name)) {
      return;
    }
    selected = name;
    render();
    if (focus) {
      panes.get(name)?.focus({ preventScroll: true });
    }
  }

  function selectCommand(command) {
    available = commandActivities(command);
    selected = DEFAULT_ACTIVITY;
    render();
  }

  for (const button of buttons) {
    button.addEventListener("click", () => {
      select(button.dataset.commandActivity, { focus: true });
    });
  }
  render();

  return { select, selectCommand };
}
