import { t } from "./i18n.js";

const COMMAND_ROUTE_ROOT = "/commands";
const COMMAND_SOURCES = new Set(["action", "kernel", "control"]);
const FACET_ID = /^[a-z][a-z0-9-]{0,31}$/;
const SUBJECT_REF = /^::[a-z][a-z0-9-]{0,31}\/[a-z0-9][a-z0-9-]{0,127}$/;
const NORMAL_SEGMENT = /^[a-z][a-z0-9-]*$/;

function isCommandSegment(segment) {
  return NORMAL_SEGMENT.test(segment);
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
  throw new Error(t(
    `不支持的命令来源：${command.source}`,
    `Unsupported command source: ${command.source}`,
  ));
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
    throw new Error(t("当前 URL 不是有效的命令地址。", "The current URL is not a valid command address."));
  }
  const source = parts[2];
  if (!COMMAND_SOURCES.has(source)) {
    throw new Error(t(
      `URL 包含未知的命令来源：${source || "<empty>"}。`,
      `The URL contains an unknown command source: ${source || "<empty>"}.`,
    ));
  }
  let segments;
  try {
    segments = parts.slice(3).map((segment) => decodeURIComponent(segment));
  } catch {
    throw new Error(t("URL 包含无效的转义字符。", "The URL contains invalid escape characters."));
  }
  if (segments.some((segment) => !isCommandSegment(segment))) {
    throw new Error(t("URL 包含无效的命令路径段。", "The URL contains an invalid command path segment."));
  }
  if (source !== "kernel" && segments.length === 0) {
    throw new Error(t(
      `URL 缺少 ${source} 命令地址。`,
      `The URL is missing a ${source} command address.`,
    ));
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
    throw new Error(t(
      `URL 指向不存在的命令：${route.address || "<root>"}。`,
      `The URL points to a missing command: ${route.address || "<root>"}.`,
    ));
  }
  return command;
}

export function parseCommandSelection(search = "") {
  const params = new URLSearchParams(search);
  const allowed = new Set(["facet", "subject", "subject-facet"]);
  const unknown = [...params.keys()].find((name) => !allowed.has(name));
  if (unknown) {
    throw new Error(t(
      `URL 包含未知的 Subject 选择参数：${unknown}。`,
      `The URL contains an unknown Subject-selection parameter: ${unknown}.`,
    ));
  }
  const facets = params.getAll("facet");
  if (facets.length > 1) {
    throw new Error(t("URL 只能声明一个命令 Facet。", "The URL may declare only one command Facet."));
  }
  const facet = facets[0] ?? null;
  if (facet !== null && !FACET_ID.test(facet)) {
    throw new Error(t(
      `URL 包含无效的命令 Facet 标识：${facet || "<empty>"}。`,
      `The URL contains an invalid command-Facet identifier: ${facet || "<empty>"}.`,
    ));
  }
  const subjects = params.getAll("subject");
  if (subjects.length > 1) {
    throw new Error(t("URL 只能声明一个 Subject。", "The URL may declare only one Subject."));
  }
  const subject = subjects[0] ?? null;
  if (subject !== null && !SUBJECT_REF.test(subject)) {
    throw new Error(t(
      `URL 包含无效的 Subject 引用：${subject || "<empty>"}。`,
      `The URL contains an invalid Subject reference: ${subject || "<empty>"}.`,
    ));
  }
  const subjectFacets = params.getAll("subject-facet");
  if (subjectFacets.length > 1) {
    throw new Error(t("URL 只能声明一个 Subject Facet。", "The URL may declare only one Subject Facet."));
  }
  const subjectFacet = subjectFacets[0] ?? null;
  if (subjectFacet !== null && !FACET_ID.test(subjectFacet)) {
    throw new Error(t(
      `URL 包含无效的 Subject Facet 标识：${subjectFacet || "<empty>"}。`,
      `The URL contains an invalid Subject-Facet identifier: ${subjectFacet || "<empty>"}.`,
    ));
  }
  if (subject !== null && facet === null) {
    throw new Error(t("Subject 深链必须声明其集合 Facet。", "A Subject deep link must declare its collection Facet."));
  }
  if (subjectFacet !== null && subject === null) {
    throw new Error(t("Subject Facet 缺少 Subject。", "A Subject Facet requires a Subject."));
  }
  return { facet, subject, subjectFacet };
}

export function parseCommandFacet(search = "") {
  return parseCommandSelection(search).facet;
}

export async function restoreCommandSelection({
  collectionFacet,
  loadCollection,
  ownerAddress,
  selectOwner,
  selectSubject,
  subjectFacet,
  subjectRef,
}) {
  const ownerSelected = selectOwner();
  if (!subjectRef || ownerSelected === false) {
    return ownerSelected !== false;
  }
  const collection = await loadCollection(ownerAddress, collectionFacet);
  if (!collection) {
    return null;
  }
  return selectSubject(ownerAddress, collectionFacet, subjectRef, {
    facet: subjectFacet,
  });
}

export function updateCommandPath(
  history,
  location,
  command,
  {
    defaultFacet = null,
    defaultSubjectFacet = null,
    facet = null,
    mode = "push",
    subject = null,
    subjectFacet = null,
  } = {},
) {
  if (mode === "none") {
    return;
  }
  const params = new URLSearchParams();
  if (facet && (subject || facet !== defaultFacet)) {
    params.set("facet", facet);
  }
  if (subject) {
    params.set("subject", subject);
  }
  if (subjectFacet && subjectFacet !== defaultSubjectFacet) {
    params.set("subject-facet", subjectFacet);
  }
  const query = params.size > 0 ? `?${params}` : "";
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
