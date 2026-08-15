import {
  childrenOf,
  isGroup,
  leafName,
  sortCommands,
} from "./catalog-model.js";
import {
  availableCommand,
  childrenColumnWidth,
  choiceColumnModels,
  commandDisabledDuringSetup,
  commandHasChoices,
  commandMenuExpanded,
  selectedCommandView,
} from "./explorer-model.js";
import { t } from "./i18n.js";

export {
  availableCommand,
  childrenColumnWidth,
  choiceColumnModels,
  commandDisabledDuringSetup,
  commandHasChoices,
  commandMenuExpanded,
} from "./explorer-model.js";

function sourceLabel(source) {
  return source === "kernel"
    ? t("内核命令", "Kernel Commands")
    : t("项目操作", "Project Actions");
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
  columns,
  detailPanel,
  getCommandViews = () => [],
  onSelectCommand,
}) {
  let catalog = null;
  let selectedPath = [];
  let setupRequired = false;
  const commandStates = new Map();

  function createCommandRow(command, depth) {
    const item = document.createElement("li");
    const button = document.createElement("button");
    const icon = document.createElement("span");
    const copy = document.createElement("span");
    const name = document.createElement("span");
    const summary = document.createElement("span");
    const chevron = document.createElement("span");
    const group = isGroup(catalog, command);
    const views = getCommandViews(command);
    const expandable = commandHasChoices(catalog, command, views);
    const selected = selectedPath[depth] === command.address;
    const menuExpanded = commandMenuExpanded(
      selectedPath,
      command.address,
      depth,
    );
    const disabled = commandDisabledDuringSetup(setupRequired, command);
    const state = commandStates.get(command.address);

    button.type = "button";
    button.className = "finder-choice command-row";
    button.dataset.address = command.address;
    button.dataset.depth = String(depth);
    button.dataset.kind = group ? "group" : "command";
    button.dataset.navigationKey = command.address;
    button.disabled = disabled;
    button.dataset.selected = String(selected);
    if (state?.tone) {
      button.dataset.stateTone = state.tone;
    }
    if (menuExpanded) {
      button.setAttribute("aria-current", "page");
    }
    if (expandable) {
      button.setAttribute("aria-expanded", String(selected));
      if (menuExpanded && views.length > 0) {
        button.setAttribute("aria-controls", `command-view-menu-${depth}`);
      } else if (selected && group) {
        button.setAttribute("aria-controls", `finder-column-${depth + 1}`);
      }
    }
    button.title = disabled
      ? t("完成首次设置后可用", "Available after initial setup")
      : command.address;

    icon.className = "row-icon";
    icon.textContent = state?.icon ?? (group ? "⌑" : ">_");
    icon.setAttribute("aria-hidden", "true");
    copy.className = "row-copy";
    name.className = "row-name";
    name.textContent = depth === 0 ? command.address : leafName(command.address);
    summary.className = "row-summary";
    summary.textContent = (state?.summary ?? command.summary) || (
      command.issue
        ? t("存在协议问题", "Protocol issue")
        : expandable && command.runnable
          ? t("可运行命令组", "Runnable command group")
          : expandable
            ? t("命令组", "Command group")
            : command.runnable
              ? t("可运行命令", "Runnable command")
              : t("不可运行", "Not runnable")
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
    item.className = "command-item";
    item.append(button);
    if (menuExpanded && views.length > 0) {
      item.append(createCommandViewMenu(command, depth, views));
    }
    return item;
  }

  function createCommandViewRow(command, depth, view) {
    const item = document.createElement("li");
    const button = document.createElement("button");
    const icon = document.createElement("span");
    const name = document.createElement("span");

    button.type = "button";
    button.className = "finder-choice command-view-row";
    button.dataset.view = view.name;
    button.dataset.parentAddress = command.address;
    button.dataset.parentDepth = String(depth);
    button.dataset.navigationKey = `${command.address}#${view.name}`;
    button.setAttribute("aria-pressed", String(view.selected));
    button.title = view.summary;

    icon.className = "row-icon view-icon";
    icon.textContent = view.icon;
    icon.setAttribute("aria-hidden", "true");
    name.className = "row-name";
    name.textContent = view.label;

    button.append(icon, name);
    button.addEventListener("click", () => {
      selectCommand(command.address, depth, {
        focusDetail: view.name !== "children",
        history: "push",
        view: view.name,
      });
    });
    item.append(button);
    return item;
  }

  function createCommandViewMenu(command, depth, views) {
    const list = document.createElement("ul");
    list.className = "command-view-menu";
    list.id = `command-view-menu-${depth}`;
    list.setAttribute(
      "aria-label",
      t(`${command.address} 视图`, `${command.address} views`),
    );
    for (const view of views) {
      list.append(createCommandViewRow(command, depth, view));
    }
    return list;
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
        source === "control" ? catalog.entryName : sourceLabel(source),
        catalog.roots.filter((command) => command.source === source),
        0,
      );
    }
    if (catalog.roots.length === 0) {
      const empty = document.createElement("p");
      empty.className = "empty-column";
      empty.textContent = t(
        "Catalog 中没有可显示的命令。",
        "The Catalog has no commands to display.",
      );
      column.append(empty);
    }
    return column;
  }

  function createChoiceColumn(parentAddress, depth) {
    const column = document.createElement("div");
    const parent = catalog.commandByAddress.get(parentAddress);
    const children = childrenOf(catalog, parentAddress);
    column.className = "finder-column";
    column.id = `finder-column-${depth}`;
    column.dataset.depth = String(depth);
    column.dataset.scrollKey = `choices:${parentAddress}`;
    column.dataset.width = childrenColumnWidth(parent);
    column.setAttribute("role", "group");
    column.setAttribute(
      "aria-label",
      t(`${parent.address} 子命令`, `${parent.address} subcommands`),
    );
    appendSection(
      column,
      t("子命令", "Subcommands"),
      children,
      depth,
    );
    return column;
  }

  function renderColumns({ focusKey = null, focusDetail = false } = {}) {
    const scrollOffsets = captureColumnScrollOffsets(columns);
    columns.replaceChildren(createRootColumn());
    const models = choiceColumnModels(
      catalog,
      selectedPath,
      getCommandViews,
    );
    for (const { command, depth } of models) {
      columns.append(createChoiceColumn(command.address, depth));
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

  function selectCommand(address, depth, options = {}) {
    const command = availableCommand(catalog, setupRequired, address);
    if (!command) {
      return false;
    }
    selectedPath = [...selectedPath.slice(0, depth), address];
    onSelectCommand(command, options);
    const view = selectedCommandView(getCommandViews(command));
    renderColumns({
      focusKey: address,
      focusDetail: options.focusDetail === true && view !== "children",
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
    const view = selectedCommandView(getCommandViews(command));
    renderColumns({
      focusKey: address,
      focusDetail: options.focusDetail === true && view !== "children",
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
      const views = command ? getCommandViews(command) : [];
      if (command && commandHasChoices(catalog, command, views)) {
        event.preventDefault();
        selectCommand(button.dataset.address, depth, { history: "push" });
        requestAnimationFrame(() => {
          const selectedView = selectedCommandView(getCommandViews(command));
          const target = selectedView === "children"
            ? columns
              .querySelector(`[data-depth="${depth + 1}"]`)
              ?.querySelector(".finder-choice")
            : columns
              .querySelector(`#command-view-menu-${depth}`)
              ?.querySelector(".finder-choice");
          target?.focus();
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
      selectAddress(preferred, {
        history: options.history ?? "none",
        view: options.view,
      });
      return;
    }
    const command = defaultCommand();
    if (command) {
      selectAddress(command.address, { history: options.history ?? "none" });
    } else {
      selectedPath = [];
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

  function setCommandState(address, state) {
    if (state) {
      commandStates.set(address, state);
    } else {
      commandStates.delete(address);
    }
    if (catalog) {
      renderColumns();
    }
  }

  return {
    handleKeyboard,
    selectAddress,
    selectCommand,
    setCatalog,
    setCommandState,
    setSetupRequired,
  };
}
