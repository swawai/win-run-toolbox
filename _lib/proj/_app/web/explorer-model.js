import { hasChildren } from "./catalog-model.js";

export function commandDisabledDuringSetup(setupRequired, command) {
  return setupRequired && command.source !== "control";
}

export function availableCommand(catalog, setupRequired, address) {
  const command = catalog.commandByAddress.get(address);
  return command && !commandDisabledDuringSetup(setupRequired, command)
    ? command
    : null;
}

export function childrenColumnWidth(command) {
  return command.childrenColumnWidth || "normal";
}

export function commandHasChoices(catalog, command, views) {
  return hasChildren(catalog, command) || views.length > 0;
}

export function commandMenuExpanded(selectedPath, address, depth) {
  return selectedPath[depth] === address && depth === selectedPath.length - 1;
}

export function selectedCommandView(views) {
  return views.find((view) => view.selected)?.name ?? null;
}

export function choiceColumnModels(catalog, selectedPath, getViews) {
  return selectedPath.flatMap((address, index) => {
    const command = catalog.commandByAddress.get(address);
    if (!command || !hasChildren(catalog, command)) {
      return [];
    }
    const terminal = index === selectedPath.length - 1;
    const revealsChildren = !terminal
      || selectedCommandView(getViews(command)) === "children";
    return revealsChildren ? [{ command, depth: index + 1 }] : [];
  });
}
