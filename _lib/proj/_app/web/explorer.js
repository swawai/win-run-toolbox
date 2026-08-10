import {
  childrenOf,
  hasChildren,
  isGroup,
  leafName,
  sortCommands,
} from "./catalog-model.js";

const sourceLabels = {
  control: "Control Plane",
  kernel: "Kernel Commands",
  action: "Project Actions",
};

export function commandDisabledDuringSetup(setupRequired, command) {
  return setupRequired && command.source !== "control";
}

export function availableCommand(catalog, setupRequired, address) {
  const command = catalog.commandByAddress.get(address);
  return command && !commandDisabledDuringSetup(setupRequired, command)
    ? command
    : null;
}

export function controlledColumnId(_command, depth) {
  return `finder-column-${depth + 1}`;
}

export function childrenColumnWidth(command) {
  return command.childrenColumnWidth || "normal";
}

export function commandHasChoices(catalog, command, activities) {
  return hasChildren(catalog, command) || activities.length > 0;
}

export function captureColumnScrollOffsets(columns) {
  return new Map(
    [...columns.querySelectorAll(".finder-column")]
      .map((column) => [column.dataset.scrollKey, column.scrollTop]),
  );
}

export function restoreColumnScrollOffsets(columns, offsets) {
  for (const column of columns.querySelectorAll(".finder-column")) {
    const offset = offsets.get(column.dataset.scrollKey);
    if (offset !== undefined) {
      column.scrollTop = offset;
    }
  }
}

export function createExplorerView({
  breadcrumb,
  columns,
  detailPanel,
  getCommandActivities = () => [],
  onSelectCommand,
}) {
  let catalog = null;
  let selectedPath = [];
  let setupRequired = false;

  function createCommandRow(command, depth) {
    const item = document.createElement("li");
    const button = document.createElement("button");
    const icon = document.createElement("span");
    const copy = document.createElement("span");
    const name = document.createElement("span");
    const summary = document.createElement("span");
    const chevron = document.createElement("span");
    const group = isGroup(catalog, command);
    const activities = getCommandActivities(command);
    const expandable = commandHasChoices(catalog, command, activities);
    const selected = selectedPath[depth] === command.address;
    const disabled = commandDisabledDuringSetup(setupRequired, command);

    button.type = "button";
    button.className = "finder-choice command-row";
    button.dataset.address = command.address;
    button.dataset.depth = String(depth);
    button.dataset.kind = group ? "group" : "command";
    button.dataset.navigationKey = command.address;
    button.disabled = disabled;
    if (selected) {
      button.setAttribute("aria-current", "page");
    }
    if (expandable) {
      button.setAttribute("aria-expanded", String(selected));
      if (selected) {
        button.setAttribute("aria-controls", controlledColumnId(command, depth));
      }
    }
    button.title = disabled ? "完成首次设置后可用" : command.address;

    icon.className = "row-icon";
    icon.textContent = group ? "⌑" : ">_";
    icon.setAttribute("aria-hidden", "true");
    copy.className = "row-copy";
    name.className = "row-name";
    name.textContent = depth === 0 ? command.address : leafName(command.address);
    summary.className = "row-summary";
    summary.textContent = command.summary || (
      command.issue
        ? "存在协议问题"
        : expandable && command.runnable
          ? "可运行命令组"
          : expandable
            ? "命令组"
            : command.runnable
              ? "可运行命令"
              : "不可运行"
    );
    copy.append(name, summary);
    chevron.className = "row-chevron";
    chevron.textContent = expandable ? "›" : "";
    chevron.setAttribute("aria-hidden", "true");

    button.append(icon, copy, chevron);
    button.addEventListener("click", (event) => {
      selectCommand(command.address, depth, {
        focusDetail: event.detail === 0,
        history: "push",
      });
    });
    item.append(button);
    return item;
  }

  function createActivityRow(command, depth, activity) {
    const item = document.createElement("li");
    const button = document.createElement("button");
    const icon = document.createElement("span");
    const copy = document.createElement("span");
    const name = document.createElement("span");
    const summary = document.createElement("span");

    button.type = "button";
    button.className = "finder-choice command-activity-row";
    button.dataset.activity = activity.name;
    button.dataset.parentAddress = command.address;
    button.dataset.parentDepth = String(depth - 1);
    button.dataset.navigationKey = `${command.address}#${activity.name}`;
    button.setAttribute("aria-pressed", String(activity.selected));

    icon.className = "row-icon activity-icon";
    icon.textContent = activity.icon;
    icon.setAttribute("aria-hidden", "true");
    copy.className = "row-copy";
    name.className = "row-name";
    name.textContent = activity.label;
    summary.className = "row-summary";
    summary.textContent = activity.summary;
    copy.append(name, summary);

    button.append(icon, copy);
    button.addEventListener("click", () => {
      selectCommand(command.address, depth - 1, {
        activity: activity.name,
        focusDetail: true,
        history: "push",
      });
    });
    item.append(button);
    return item;
  }

  function appendSection(column, label, commands, depth) {
    if (commands.length === 0) {
      return;
    }
    const section = document.createElement("section");
    const list = document.createElement("ul");
    section.className = "column-section";
    list.className = "column-list";
    for (const command of sortCommands(catalog, commands)) {
      list.append(createCommandRow(command, depth));
    }
    if (label) {
      const heading = document.createElement("h2");
      heading.className = "column-label";
      heading.textContent = label;
      section.append(heading);
    }
    section.append(list);
    column.append(section);
  }

  function appendActivitySection(column, command, activities, depth) {
    if (activities.length === 0) {
      return;
    }
    const section = document.createElement("section");
    const heading = document.createElement("h2");
    const list = document.createElement("ul");
    section.className = "column-section command-activity-section";
    heading.className = "column-label";
    heading.textContent = "当前命令";
    list.className = "column-list";
    for (const activity of activities) {
      list.append(createActivityRow(command, depth, activity));
    }
    section.append(heading, list);
    column.append(section);
  }

  function createRootColumn() {
    const column = document.createElement("div");
    column.className = "finder-column";
    column.id = "finder-column-0";
    column.dataset.depth = "0";
    column.dataset.scrollKey = "root";
    column.dataset.width = "normal";
    for (const source of ["control", "kernel", "action"]) {
      appendSection(
        column,
        sourceLabels[source],
        catalog.roots.filter((command) => command.source === source),
        0,
      );
    }
    if (catalog.roots.length === 0) {
      const empty = document.createElement("p");
      empty.className = "empty-column";
      empty.textContent = "Catalog 中没有可显示的命令。";
      column.append(empty);
    }
    return column;
  }

  function createChoiceColumn(parentAddress, depth, activities = []) {
    const column = document.createElement("div");
    const parent = catalog.commandByAddress.get(parentAddress);
    const children = childrenOf(catalog, parentAddress);
    column.className = "finder-column";
    column.id = `finder-column-${depth}`;
    column.dataset.depth = String(depth);
    column.dataset.scrollKey = `choices:${parentAddress}`;
    column.dataset.width = childrenColumnWidth(parent);
    column.setAttribute("role", "group");
    const choicesLabel = activities.length > 0 && children.length > 0
      ? "可用操作和子命令"
      : activities.length > 0
        ? "可用操作"
        : "子命令";
    column.setAttribute("aria-label", `${parent.address} ${choicesLabel}`);
    appendActivitySection(column, parent, activities, depth);
    appendSection(
      column,
      "子命令",
      children,
      depth,
    );
    return column;
  }

  function renderColumns({ focusKey = null, focusDetail = false } = {}) {
    const scrollOffsets = captureColumnScrollOffsets(columns);
    columns.replaceChildren(createRootColumn());
    for (const [depth, address] of selectedPath.entries()) {
      const command = catalog.commandByAddress.get(address);
      const current = depth === selectedPath.length - 1;
      const activities = command && current
        ? getCommandActivities(command)
        : [];
      if (command && commandHasChoices(catalog, command, activities)) {
        columns.append(createChoiceColumn(address, depth + 1, activities));
      }
    }
    restoreColumnScrollOffsets(columns, scrollOffsets);

    requestAnimationFrame(() => {
      const focusTarget = focusKey
        ? [...columns.querySelectorAll(".command-row")]
          .find((row) => row.dataset.navigationKey === focusKey)
        : null;
      if (focusDetail) {
        detailPanel.focus({ preventScroll: true });
        detailPanel.scrollIntoView({ block: "nearest", inline: "nearest" });
      } else {
        focusTarget?.focus({ preventScroll: true });
        columns.lastElementChild?.scrollIntoView({ block: "nearest", inline: "nearest" });
      }
      // Browser focus and scrollIntoView may also move a nested scroll container.
      // Reapply the semantic column offsets after those side effects settle.
      restoreColumnScrollOffsets(columns, scrollOffsets);
    });
  }

  function renderBreadcrumb() {
    const fragment = document.createDocumentFragment();
    const home = document.createElement("span");
    home.className = "breadcrumb-home";
    home.textContent = "控制台";
    fragment.append(home);
    for (const [depth, address] of selectedPath.entries()) {
      const separator = document.createElement("span");
      const item = document.createElement("span");
      separator.className = "breadcrumb-separator";
      separator.textContent = "›";
      separator.setAttribute("aria-hidden", "true");
      item.className = "breadcrumb-item";
      item.textContent = depth === 0 ? address : leafName(address);
      fragment.append(separator, item);
    }
    breadcrumb.replaceChildren(fragment);
    breadcrumb.scrollLeft = breadcrumb.scrollWidth;
  }

  function selectCommand(address, depth, options = {}) {
    const command = availableCommand(catalog, setupRequired, address);
    if (!command) {
      return false;
    }
    selectedPath = [...selectedPath.slice(0, depth), address];
    onSelectCommand(command, options);
    renderBreadcrumb();
    renderColumns({
      focusKey: address,
      focusDetail: options.focusDetail === true,
    });
    return true;
  }

  function addressPath(address) {
    const path = [];
    let current = catalog.commandByAddress.get(address);
    while (current) {
      path.unshift(current.address);
      current = current.parent
        ? catalog.commandByAddress.get(current.parent)
        : null;
    }
    return path;
  }

  function selectAddress(address, options = {}) {
    const command = availableCommand(catalog, setupRequired, address);
    if (!command) {
      return false;
    }
    selectedPath = addressPath(address);
    onSelectCommand(command, options);
    renderBreadcrumb();
    renderColumns({
      focusKey: address,
      focusDetail: options.focusDetail === true,
    });
    return true;
  }

  function defaultCommand() {
    const available = catalog.roots.filter(
      (command) => !commandDisabledDuringSetup(setupRequired, command),
    );
    return sortCommands(catalog, available)[0] ?? null;
  }

  function handleKeyboard(event) {
    const button = event.target.closest(".finder-choice");
    if (!button) {
      return;
    }
    const rows = [...button.closest(".finder-column")?.querySelectorAll(".finder-choice") ?? []];
    const index = rows.indexOf(button);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const offset = event.key === "ArrowDown" ? 1 : -1;
      rows[(index + offset + rows.length) % rows.length]?.focus();
      return;
    }

    if (!button.classList.contains("command-row")) {
      if (event.key === "ArrowLeft") {
        event.preventDefault();
        const parentAddress = button.dataset.parentAddress;
        columns.querySelector(
          `.command-row[data-address="${CSS.escape(parentAddress)}"]`,
        )?.focus();
      }
      return;
    }

    const depth = Number(button.dataset.depth);
    if (event.key === "ArrowRight") {
      const command = catalog.commandByAddress.get(button.dataset.address);
      const activities = command ? getCommandActivities(command) : [];
      if (command && commandHasChoices(catalog, command, activities)) {
        event.preventDefault();
        selectCommand(button.dataset.address, depth, { history: "push" });
        requestAnimationFrame(() => {
          const nextColumn = columns.querySelector(`[data-depth="${depth + 1}"]`);
          nextColumn?.querySelector(".finder-choice")?.focus();
        });
      }
    } else if (event.key === "ArrowLeft" && depth > 0) {
      event.preventDefault();
      selectAddress(selectedPath[depth - 1], { history: "push" });
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      selectCommand(button.dataset.address, depth, {
        focusDetail: true,
        history: "push",
      });
    }
  }

  function setCatalog(nextCatalog, options = {}) {
    const previous = selectedPath.at(-1);
    catalog = nextCatalog;
    const preferred = options.address ?? previous;
    const preferredCommand = preferred
      ? availableCommand(catalog, setupRequired, preferred)
      : null;
    if (preferredCommand) {
      selectAddress(preferred, { history: options.history ?? "none" });
      return;
    }
    const command = defaultCommand();
    if (command) {
      selectAddress(command.address, { history: options.history ?? "none" });
    } else {
      selectedPath = [];
      renderBreadcrumb();
      renderColumns();
    }
  }

  function setSetupRequired(required) {
    setupRequired = required;
    if (!catalog) {
      return;
    }
    const selected = catalog.commandByAddress.get(selectedPath.at(-1));
    if (selected && !commandDisabledDuringSetup(setupRequired, selected)) {
      renderColumns();
      return;
    }
    const command = defaultCommand();
    if (command) {
      selectAddress(command.address, { history: "replace" });
    }
  }

  return {
    handleKeyboard,
    selectAddress,
    selectCommand,
    setCatalog,
    setSetupRequired,
  };
}
