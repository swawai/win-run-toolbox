const COMMAND_ROUTE_ROOT = "/commands";
const COMMAND_SOURCES = new Set(["action", "kernel", "control"]);
const COMMAND_VIEWS = new Set(["children", "edit", "overview", "help", "run", "logs"]);
const NORMAL_SEGMENT = /^[a-z][a-z0-9-]*$/;
const ENVIRONMENT_SEGMENT = /^SWAWKIT_PROJ_[A-Z0-9_]+$/;

function isCommandSegment(segment) {
  return NORMAL_SEGMENT.test(segment) || ENVIRONMENT_SEGMENT.test(segment);
}

function addressSegments(command) {
  if (command.source === "action") {
    return command.address.split(".");
  }
  if (command.source === "kernel") {
    return command.address === "" ? [] : command.address.slice(1).split(".");
  }
  if (command.source === "control") {
    return command.address.slice(2).split(".");
  }
  throw new Error(`不支持的命令来源：${command.source}`);
}

export function commandPath(command) {
  const segments = addressSegments(command);
  const suffix = segments.length === 0
    ? ""
    : `/${segments.map(encodeURIComponent).join("/")}`;
  return `${COMMAND_ROUTE_ROOT}/${command.source}${suffix}`;
}

export function parseCommandPath(pathname) {
  if (pathname === "/" || pathname === COMMAND_ROUTE_ROOT || pathname === `${COMMAND_ROUTE_ROOT}/`) {
    return null;
  }
  const parts = pathname.split("/");
  if (parts.length < 3 || parts[0] !== "" || parts[1] !== "commands") {
    throw new Error("当前 URL 不是有效的命令地址。");
  }
  const source = parts[2];
  if (!COMMAND_SOURCES.has(source)) {
    throw new Error(`URL 包含未知的命令来源：${source || "<empty>"}。`);
  }
  let segments;
  try {
    segments = parts.slice(3).map((segment) => decodeURIComponent(segment));
  } catch {
    throw new Error("URL 包含无效的转义字符。");
  }
  if (segments.some((segment) => !isCommandSegment(segment))) {
    throw new Error("URL 包含无效的命令路径段。");
  }
  if (source !== "kernel" && segments.length === 0) {
    throw new Error(`URL 缺少 ${source} 命令地址。`);
  }
  const joined = segments.join(".");
  return {
    source,
    address: source === "action"
      ? joined
      : source === "kernel"
        ? segments.length === 0 ? "" : `.${joined}`
        : `..${joined}`,
  };
}

export function commandAtPath(
  catalog,
  pathname,
  { allowMissing = false } = {},
) {
  const route = parseCommandPath(pathname);
  if (route === null) {
    return null;
  }
  const command = catalog.commandByAddress.get(route.address);
  if (!command || command.source !== route.source) {
    if (allowMissing) {
      return null;
    }
    throw new Error(`URL 指向不存在的命令：${route.address || "<root>"}。`);
  }
  return command;
}

export function parseCommandView(search = "") {
  const params = new URLSearchParams(search);
  const unknown = [...params.keys()].find((name) => name !== "view");
  if (unknown) {
    throw new Error(`URL 包含未知的命令视图参数：${unknown}。`);
  }
  const values = params.getAll("view");
  if (values.length > 1) {
    throw new Error("URL 只能声明一个命令视图。");
  }
  const view = values[0] ?? null;
  if (view !== null && !COMMAND_VIEWS.has(view)) {
    throw new Error(`URL 包含未知的命令视图：${view || "<empty>"}。`);
  }
  return view;
}

export function updateCommandPath(
  history,
  location,
  command,
  { defaultView = null, mode = "push", view = null } = {},
) {
  if (mode === "none") {
    return;
  }
  const query = view && view !== defaultView
    ? `?view=${encodeURIComponent(view)}`
    : "";
  const path = `${commandPath(command)}${query}`;
  const current = `${location.pathname}${location.search ?? ""}`;
  if (mode === "push" && current === path) {
    return;
  }
  if (mode === "replace") {
    history.replaceState(null, "", path);
  } else {
    history.pushState(null, "", path);
  }
}
