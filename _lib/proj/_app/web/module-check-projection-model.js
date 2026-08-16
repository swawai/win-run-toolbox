export const MODULE_CHECK_PROTOCOL = "swawkit.module-check/v1";

const COMMAND_SOURCES = new Set(["kernel", "action"]);
const MAX_ITEMS = 512;

function invalid(message) {
  return new Error(`模块检查协议无效：${message}`);
}

function object(value, field) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw invalid(`${field} 必须是对象。`);
  }
  return value;
}

function string(value, field) {
  if (typeof value !== "string" || value.length === 0) {
    throw invalid(`${field} 必须是非空字符串。`);
  }
  return value;
}

function nullableString(value, field) {
  return value === null ? null : string(value, field);
}

function boolean(value, field) {
  if (typeof value !== "boolean") {
    throw invalid(`${field} 必须是布尔值。`);
  }
  return value;
}

function array(value, field) {
  if (!Array.isArray(value)) {
    throw invalid(`${field} 必须是数组。`);
  }
  return value;
}

function publication(value, field, budget) {
  budget.count += 1;
  if (budget.count > MAX_ITEMS) {
    throw invalid(`检查结果最多包含 ${MAX_ITEMS} 个条目。`);
  }
  const item = object(value, field);
  const exports = array(item.exports, `${field}.exports`).map((entry, index) => {
    const exported = object(entry, `${field}.exports[${index}]`);
    return {
      kind: string(exported.kind, `${field}.exports[${index}].kind`),
      name: string(exported.name, `${field}.exports[${index}].name`),
    };
  });
  return {
    contract: string(item.contract, `${field}.contract`),
    exportRoot: nullableString(item.exportRoot, `${field}.exportRoot`),
    exports,
    exportsTruncated: boolean(item.exportsTruncated, `${field}.exportsTruncated`),
    message: nullableString(item.message, `${field}.message`),
    provider: string(item.provider, `${field}.provider`),
    ready: boolean(item.ready, `${field}.ready`),
    statePath: nullableString(item.statePath, `${field}.statePath`),
    status: string(item.status, `${field}.status`),
  };
}

function dependency(value, field, budget, depth = 0) {
  if (depth > 32) {
    throw invalid("依赖层级过深。");
  }
  budget.count += 1;
  if (budget.count > MAX_ITEMS) {
    throw invalid(`检查结果最多包含 ${MAX_ITEMS} 个条目。`);
  }
  const item = object(value, field);
  return {
    contract: string(item.contract, `${field}.contract`),
    dependencies: array(item.dependencies, `${field}.dependencies`).map((child, index) => (
      dependency(child, `${field}.dependencies[${index}]`, budget, depth + 1)
    )),
    message: nullableString(item.message, `${field}.message`),
    provider: string(item.provider, `${field}.provider`),
    publication: item.publication === null
      ? null
      : publication(item.publication, `${field}.publication`, budget),
    ready: boolean(item.ready, `${field}.ready`),
    status: string(item.status, `${field}.status`),
  };
}

export function createModuleCheckProjection(value, subject) {
  const document_ = object(value, "check");
  if (document_.protocol !== MODULE_CHECK_PROTOCOL) {
    throw invalid(`protocol 必须是 ${MODULE_CHECK_PROTOCOL}。`);
  }
  const command = object(document_.command, "command");
  const address = string(command.address, "command.address");
  if (address !== subject.address) {
    throw invalid("command.address 与选中的命令不一致。");
  }
  if (!COMMAND_SOURCES.has(command.source) || command.source !== subject.source) {
    throw invalid("command.source 与选中的 Kernel 或 Action 命令不一致。");
  }
  const normalizedCommand = {
    adapter: nullableString(command.adapter, "command.adapter"),
    address,
    diagnostic: nullableString(command.diagnostic, "command.diagnostic"),
    runnable: boolean(command.runnable, "command.runnable"),
    source: command.source,
  };
  const budget = { count: 0 };
  const guards = array(document_.guards, "guards").map((guard, index) => {
    budget.count += 1;
    if (budget.count > MAX_ITEMS) {
      throw invalid(`检查结果最多包含 ${MAX_ITEMS} 个条目。`);
    }
    const item = object(guard, `guards[${index}]`);
    return {
      entry: string(item.entry, `guards[${index}].entry`),
      scope: string(item.scope, `guards[${index}].scope`),
    };
  });
  const dependencies = array(document_.dependencies, "dependencies").map((item, index) => (
    dependency(item, `dependencies[${index}]`, budget)
  ));
  const publications = array(document_.publications, "publications").map((item, index) => (
    publication(item, `publications[${index}]`, budget)
  ));
  const ok = boolean(document_.ok, "ok");
  const expectedOk = normalizedCommand.runnable
    && dependencies.every((item) => item.ready)
    && publications.every((item) => item.ready);
  if (ok !== expectedOk) {
    throw invalid("ok 与命令、依赖及产物状态不一致。");
  }
  return {
    command: normalizedCommand,
    dependencies,
    guards,
    ok,
    protocol: MODULE_CHECK_PROTOCOL,
    publications,
  };
}
