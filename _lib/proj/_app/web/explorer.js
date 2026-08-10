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
    const expandable = hasChildren(catalog, command);
    const selected = selectedPath[depth] === command.address;
    const disabled = commandDisabledDuringSetup(setupRequired, command);

    button.type = "button";
    button.className = "command-row";
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

  function createChildColumn(parentAddress, depth) {
    const column = document.createElement("div");
    const parent = catalog.commandByAddress.get(parentAddress);
    column.className = "finder-column";
    column.id = `finder-column-${depth}`;
    column.dataset.depth = String(depth);
    column.dataset.scrollKey = `children:${parentAddress}`;
    column.dataset.width = childrenColumnWidth(parent);
    column.setAttribute("role", "group");
    column.setAttribute("aria-label", `${parent.address} 子命令`);
    appendSection(column, null, childrenOf(catalog, parentAddress), depth);
    return column;
  }

  function renderColumns({ focusKey = null, focusDetail = false } = {}) {
    const scrollOffsets = captureColumnScrollOffsets(columns);
    columns.replaceChildren(createRootColumn());
    for (const [depth, address] of selectedPath.entries()) {
      const command = catalog.commandByAddress.get(address);
      if (command && hasChildren(catalog, command)) {
        columns.append(createChildColumn(address, depth + 1));
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
    const button = event.target.closest(".command-row");
    if (!button) {
      return;
    }
    const rows = [...button.closest(".finder-column")?.querySelectorAll(".command-row") ?? []];
    const index = rows.indexOf(button);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const offset = event.key === "ArrowDown" ? 1 : -1;
      rows[(index + offset + rows.length) % rows.length]?.focus();
      return;
    }

    const depth = Number(button.dataset.depth);
    if (event.key === "ArrowRight") {
      const children = childrenOf(catalog, button.dataset.address);
      if (children.length > 0) {
        event.preventDefault();
        selectCommand(button.dataset.address, depth, { history: "push" });
        requestAnimationFrame(() => {
          const nextColumn = columns.querySelector(`[data-depth="${depth + 1}"]`);
          nextColumn?.querySelector(".command-row")?.focus();
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
