const CATALOG_PROTOCOL = "swawkit.command-catalog/v4";

const collator = new Intl.Collator("zh-CN", {
  numeric: true,
  sensitivity: "base",
});

function contractError(message) {
  return new Error(`Catalog 协议无效：${message}`);
}

function requireObject(value, field) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw contractError(`${field} 必须是对象。`);
  }
  return value;
}

function requireString(value, field, { allowEmpty = true } = {}) {
  if (
    typeof value !== "string"
    || (!allowEmpty && value.trim().length === 0)
  ) {
    const expectation = allowEmpty ? "字符串" : "非空字符串";
    throw contractError(`${field} 必须是${expectation}。`);
  }
  return value;
}

function nullableString(value, field, { allowEmpty = false } = {}) {
  return value === null
    ? null
    : requireString(value, field, { allowEmpty });
}

function normalizeHelp(value, index) {
  if (value === null) {
    return null;
  }

  const help = requireObject(value, `commands[${index}].help`);
  return {
    summary: requireString(help.summary, `commands[${index}].help.summary`),
    text: requireString(help.text, `commands[${index}].help.text`),
  };
}

function normalizeView(value, index) {
  if (value === null) {
    return null;
  }
  const view = requireObject(value, `commands[${index}].view`);
  const childrenColumn = requireObject(
    view.childrenColumn,
    `commands[${index}].view.childrenColumn`,
  );
  const width = requireString(
    childrenColumn.width,
    `commands[${index}].view.childrenColumn.width`,
    { allowEmpty: false },
  );
  if (!new Set(["normal", "wide"]).has(width)) {
    throw contractError(
      `commands[${index}].view.childrenColumn.width 只能是 normal 或 wide。`,
    );
  }
  return { childrenColumnWidth: width };
}

function normalizeCommand(value, index) {
  const command = requireObject(value, `commands[${index}]`);
  const field = (name) => `commands[${index}].${name}`;
  const address = requireString(command.address, field("address"));
  const source = requireString(command.source, field("source"), {
    allowEmpty: false,
  });
  if (!new Set(["control", "kernel", "action"]).has(source)) {
    throw contractError(`${field("source")} 只能是 control、kernel 或 action。`);
  }
  if (typeof command.runnable !== "boolean") {
    throw contractError(`${field("runnable")} 必须是布尔值。`);
  }

  const entry = nullableString(command.entry, field("entry"));
  const adapter = nullableString(command.adapter, field("adapter"));
  const handler = nullableString(command.handler, field("handler"));
  if (command.runnable !== (entry !== null)) {
    throw contractError(`${field("runnable")} 必须与 entry 是否存在一致。`);
  }
  if ((entry === null) !== (adapter === null)) {
    throw contractError(`${field("adapter")} 必须与 entry 同时存在或同时为空。`);
  }
  const handlerAdapter = adapter === "core" || adapter === "toolchain";
  if (handlerAdapter !== (handler !== null)) {
    throw contractError(
      `${field("handler")} 必须且只能由 core 或 toolchain adapter 声明。`,
    );
  }

  const help = normalizeHelp(command.help, index);
  const view = normalizeView(command.view, index);
  return {
    address,
    adapter: adapter ?? "",
    aliasOf: nullableString(command.aliasOf, field("aliasOf")),
    childrenColumnWidth: view?.childrenColumnWidth ?? "normal",
    entry: entry ?? "",
    help: help?.text ?? "",
    handler: handler ?? "",
    issue: nullableString(command.diagnostic, field("diagnostic")) ?? "",
    parent: nullableString(command.parent, field("parent"), {
      allowEmpty: true,
    }),
    runnable: command.runnable,
    source,
    summary: help?.summary ?? "",
  };
}

export function createCatalog(document) {
  const payload = requireObject(document, "Catalog");
  if (payload.protocol !== CATALOG_PROTOCOL) {
    throw contractError(`protocol 必须是 ${CATALOG_PROTOCOL}。`);
  }
  const entryName = requireString(payload.entryName, "entryName", {
    allowEmpty: false,
  });
  if (!Array.isArray(payload.commands)) {
    throw contractError("commands 必须是数组。");
  }

  const commandByAddress = new Map();
  const seenAddresses = new Set();
  for (const [index, rawCommand] of payload.commands.entries()) {
    const command = normalizeCommand(rawCommand, index);
    if (seenAddresses.has(command.address)) {
      throw contractError(`命令地址 ${command.address || "<root>"} 重复。`);
    }
    seenAddresses.add(command.address);

    if (command.address && !command.aliasOf) {
      commandByAddress.set(command.address, command);
    }
  }

  const childrenByParent = new Map();
  const roots = [];
  for (const command of commandByAddress.values()) {
    if (
      command.parent
      && command.parent !== command.address
      && commandByAddress.has(command.parent)
    ) {
      const siblings = childrenByParent.get(command.parent) ?? [];
      siblings.push(command);
      childrenByParent.set(command.parent, siblings);
    } else {
      roots.push(command);
    }
  }

  return {
    childrenByParent,
    commandByAddress,
    commands: [...commandByAddress.values()],
    entryName,
    protocol: CATALOG_PROTOCOL,
    roots,
  };
}

export function childrenOf(catalog, address) {
  return catalog.childrenByParent.get(address) ?? [];
}

export function hasChildren(catalog, command) {
  return childrenOf(catalog, command.address).length > 0;
}

export function isGroup(catalog, command) {
  return hasChildren(catalog, command);
}

export function sortCommands(catalog, commands) {
  return [...commands].sort((left, right) => {
    const groupOrder = Number(isGroup(catalog, right))
      - Number(isGroup(catalog, left));
    return groupOrder || collator.compare(left.address, right.address);
  });
}

export function leafName(address) {
  const parts = address.split(/[./\\]+/).filter(Boolean);
  return parts.at(-1) || address;
}

export function cliInvocation(catalog, command) {
  return `${catalog.entryName} ${command.address}`;
}
