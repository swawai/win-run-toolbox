import { t } from "./i18n.js";
import { normalizeFacets as normalizeFacetDocuments } from "./facet-model.js";
import { normalizeSubjectKinds } from "./subject-kind-model.js";

const CATALOG_PROTOCOL = "swawkit.command-catalog/v13";
const MODULE_PROTOCOL = "swawkit.command-module/v4";

function contractError(message) {
  return new Error(`${t("Catalog 协议无效", "Invalid Catalog protocol")}: ${message}`);
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

function normalizeModule(value, index) {
  if (value === null) {
    return null;
  }
  const field = `commands[${index}].module`;
  const module = requireObject(value, field);
  if (module.schema !== MODULE_PROTOCOL) {
    throw contractError(`${field}.schema 必须是 ${MODULE_PROTOCOL}。`);
  }
  if (!Array.isArray(module.requires) || !Array.isArray(module.provides)) {
    throw contractError(`${field} 必须包含 requires 和 provides 数组。`);
  }
  const requires = module.requires.map((raw, requirementIndex) => {
    const requirementField = `${field}.requires[${requirementIndex}]`;
    const requirement = requireObject(raw, requirementField);
    return {
      contract: requireString(requirement.contract, `${requirementField}.contract`, {
        allowEmpty: false,
      }),
      provider: requireString(requirement.provider, `${requirementField}.provider`, {
        allowEmpty: false,
      }),
    };
  });
  const provides = module.provides.map((raw, provisionIndex) => {
    const provisionField = `${field}.provides[${provisionIndex}]`;
    const provision = requireObject(raw, provisionField);
    return {
      contract: requireString(provision.contract, `${provisionField}.contract`, {
        allowEmpty: false,
      }),
    };
  });
  return { provides, requires, schema: MODULE_PROTOCOL };
}

function normalizeView(value, index) {
  if (value === null) {
    return null;
  }
  const view = requireObject(value, `commands[${index}].view`);
  let width = "normal";
  if (view.childrenColumn !== undefined) {
    const childrenColumn = requireObject(
      view.childrenColumn,
      `commands[${index}].view.childrenColumn`,
    );
    width = requireString(
      childrenColumn.width,
      `commands[${index}].view.childrenColumn.width`,
      { allowEmpty: false },
    );
    if (!new Set(["normal", "wide"]).has(width)) {
      throw contractError(
        `commands[${index}].view.childrenColumn.width 只能是 normal 或 wide。`,
      );
    }
  }

  let runOperations = [];
  if (view.run !== undefined) {
    const run = requireObject(view.run, `commands[${index}].view.run`);
    if (
      !Array.isArray(run.operations)
      || run.operations.length === 0
      || run.operations.length > 8
    ) {
      throw contractError(
        `commands[${index}].view.run.operations 必须包含 1 至 8 个操作。`,
      );
    }
    const identifiers = new Set();
    runOperations = run.operations.map((rawOperation, operationIndex) => {
      const field = `commands[${index}].view.run.operations[${operationIndex}]`;
      const operation = requireObject(rawOperation, field);
      const id = requireString(operation.id, `${field}.id`, { allowEmpty: false });
      if (!/^[a-z][a-z0-9-]{0,31}$/.test(id) || identifiers.has(id)) {
        throw contractError(`${field}.id 必须唯一并匹配 [a-z][a-z0-9-]{0,31}。`);
      }
      identifiers.add(id);
      const label = requireString(operation.label, `${field}.label`, {
        allowEmpty: false,
      });
      if (label.trim() !== label || label.length > 64) {
        throw contractError(`${field}.label 必须是 1 至 64 个字符的无首尾空白文本。`);
      }
      if (
        !Array.isArray(operation.arguments)
        || operation.arguments.length > 32
        || operation.arguments.some((argument) => (
          typeof argument !== "string" || argument.length > 4096
        ))
      ) {
        throw contractError(`${field}.arguments 必须是最多 32 项的字符串数组。`);
      }
      let confirmation = null;
      if (operation.confirmation !== undefined) {
        confirmation = requireString(operation.confirmation, `${field}.confirmation`, {
          allowEmpty: false,
        });
        if (confirmation.trim() !== confirmation || confirmation.length > 500) {
          throw contractError(
            `${field}.confirmation 必须是 1 至 500 个字符的无首尾空白文本。`,
          );
        }
      }
      return {
        arguments: [...operation.arguments],
        confirmation,
        id,
        label,
      };
    });
  }
  if (view.childrenColumn === undefined && view.run === undefined) {
    throw contractError(`commands[${index}].view 必须声明 childrenColumn 或 run。`);
  }
  return { childrenColumnWidth: width, runOperations };
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
  const module = normalizeModule(command.module, index);
  const view = normalizeView(command.view, index);
  const facets = normalizeFacetDocuments(
    command.facets,
    `commands[${index}].facets`,
    contractError,
  );
  const subjectKinds = normalizeSubjectKinds(
    command.subjectKinds,
    `commands[${index}].subjectKinds`,
    contractError,
  );
  return {
    facets,
    address,
    adapter: adapter ?? "",
    aliasOf: nullableString(command.aliasOf, field("aliasOf")),
    childrenColumnWidth: view?.childrenColumnWidth ?? "normal",
    entry: entry ?? "",
    help: help?.text ?? "",
    handler: handler ?? "",
    issue: nullableString(command.diagnostic, field("diagnostic")) ?? "",
    module,
    parent: nullableString(command.parent, field("parent"), {
      allowEmpty: true,
    }),
    runOperations: view?.runOperations ?? [],
    runnable: command.runnable,
    setupAvailable: source === "control",
    source,
    subjectKinds,
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
  const language = requireString(payload.language, "language", {
    allowEmpty: false,
  });
  if (!new Set(["zh-CN", "en"]).has(language)) {
    throw contractError(t("language 只能是 zh-CN 或 en。", "language must be zh-CN or en."));
  }
  if (!Array.isArray(payload.commands)) {
    throw contractError("commands 必须是数组。");
  }

  const commandByAddress = new Map();
  const subjectKindByKind = new Map();
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
    for (const subjectKind of command.subjectKinds) {
      if (subjectKindByKind.has(subjectKind.kind)) {
        throw contractError(`Subject kind ${subjectKind.kind} is declared more than once.`);
      }
      subjectKindByKind.set(subjectKind.kind, { command, subjectKind });
    }
  }

  for (const command of commandByAddress.values()) {
    for (const facet of command.facets) {
      if (
        facet.subjectKind !== null
        && (
          !subjectKindByKind.has(facet.subjectKind.kind)
          || subjectKindByKind.get(facet.subjectKind.kind).command.address
            !== facet.subjectKind.provider.address
          || subjectKindByKind.get(facet.subjectKind.kind).command.source
            !== facet.subjectKind.provider.source
        )
      ) {
        throw contractError(
          `${command.address}.facets.${facet.id} references an unavailable Subject kind.`,
        );
      }
      if (facet.resolver?.type !== "command") {
        continue;
      }
      const target = commandByAddress.get(facet.resolver.address);
      if (!target) {
        throw contractError(
          `${command.address}.facets.${facet.id} 引用了不存在的命令。`,
        );
      }
      const controlEdit = facet.renderer === "edit"
        && target.source === "control"
        && target.handler === "entry.profile.set";
      if (!target.runnable || (target.source === "control" && !controlEdit) || target.aliasOf) {
        throw contractError(
          `${command.address}.facets.${facet.id} 不是可由 Web 执行的精确命令。`,
        );
      }
    }
    for (const subjectKind of command.subjectKinds) {
      for (const facet of subjectKind.facets) {
        const target = commandByAddress.get(facet.resolver.address);
        if (!target || !target.runnable || target.source === "control" || target.aliasOf) {
          throw contractError(
            `${command.address}.subjectKinds.${subjectKind.kind}.${facet.id} has an invalid resolver target.`,
          );
        }
      }
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

  for (const command of commandByAddress.values()) {
    if (command.handler !== "entry.profile.set") {
      continue;
    }
    let current = command;
    while (current) {
      current.setupAvailable = true;
      current = current.parent ? commandByAddress.get(current.parent) : null;
    }
  }

  return {
    childrenByParent,
    commandByAddress,
    commands: [...commandByAddress.values()],
    entryName,
    language,
    collator: new Intl.Collator(language, {
      numeric: true,
      sensitivity: "base",
    }),
    protocol: CATALOG_PROTOCOL,
    roots,
    subjectKindByKind,
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
  return [...commands].sort((left, right) => (
    catalog.collator.compare(left.address, right.address)
  ));
}

export function leafName(address) {
  const parts = address.split(/[./\\]+/).filter(Boolean);
  return parts.at(-1) || address;
}

export function cliInvocation(catalog, command) {
  return `${catalog.entryName} ${command.address}`;
}
