import { normalizeCommandEvents } from "./command-event-client.js";

const JOURNALS_URL = "/api/v2/command-run-journals";
const HISTORY_PROTOCOL = "swawkit.command-run-history/v1";
const JOURNAL_PROTOCOL = "swawkit.command-run-journal/v1";
const SOURCES = new Set(["cli", "web"]);
const STATES = new Set(["running", "exited", "canceled", "failed"]);

export class CommandJournalError extends Error {
  constructor(message, status = 0) {
    super(message);
    this.name = "CommandJournalError";
    this.status = status;
  }
}

function contractError(message) {
  return new CommandJournalError(`命令日志协议无效：${message}`);
}

function object(value, field) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw contractError(`${field} 必须是对象。`);
  }
  return value;
}

function string(value, field) {
  if (typeof value !== "string" || value.length === 0) {
    throw contractError(`${field} 必须是非空字符串。`);
  }
  return value;
}

function integer(value, field) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw contractError(`${field} 必须是非负安全整数。`);
  }
  return value;
}

function nullableInteger(value, field) {
  return value === null ? null : integer(value, field);
}

function normalizeOutcome(value, field) {
  if (!STATES.has(value.state)) {
    throw contractError(`${field}.state 不是受支持的状态。`);
  }
  const finishedAtUnixMs = nullableInteger(
    value.finishedAtUnixMs,
    `${field}.finishedAtUnixMs`,
  );
  const exitCode = value.exitCode;
  if (exitCode !== null && !Number.isSafeInteger(exitCode)) {
    throw contractError(`${field}.exitCode 必须是整数或 null。`);
  }
  if (value.error !== null && typeof value.error !== "string") {
    throw contractError(`${field}.error 必须是字符串或 null。`);
  }
  const valid = value.state === "running"
    ? finishedAtUnixMs === null && exitCode === null && value.error === null
    : value.state === "exited"
      ? finishedAtUnixMs !== null && Number.isSafeInteger(exitCode) && value.error === null
      : value.state === "canceled"
        ? finishedAtUnixMs !== null && exitCode === null && value.error === null
        : finishedAtUnixMs !== null
          && exitCode === null
          && typeof value.error === "string"
          && value.error.length > 0;
  if (!valid) {
    throw contractError(`${field} 的状态与终态字段不一致。`);
  }
  return { state: value.state, finishedAtUnixMs, exitCode, error: value.error };
}

function normalizeSummary(value, index) {
  const summary = object(value, `runs[${index}]`);
  const field = `runs[${index}]`;
  if (!SOURCES.has(summary.source)) {
    throw contractError(`${field}.source 不是 cli 或 web。`);
  }
  if (typeof summary.truncated !== "boolean") {
    throw contractError(`${field}.truncated 必须是布尔值。`);
  }
  return {
    id: string(summary.id, `${field}.id`),
    source: summary.source,
    ...normalizeOutcome(summary, field),
    startedAtUnixMs: integer(summary.startedAtUnixMs, `${field}.startedAtUnixMs`),
    argumentCount: integer(summary.argumentCount, `${field}.argumentCount`),
    eventCount: integer(summary.eventCount, `${field}.eventCount`),
    truncated: summary.truncated,
  };
}

export function normalizeCommandJournalHistory(value) {
  const history = object(value, "history");
  if (history.protocol !== HISTORY_PROTOCOL) {
    throw contractError(`protocol 必须是 ${HISTORY_PROTOCOL}。`);
  }
  if (!Array.isArray(history.runs)) {
    throw contractError("runs 必须是数组。");
  }
  return {
    protocol: HISTORY_PROTOCOL,
    address: string(history.address, "address"),
    runs: history.runs.map(normalizeSummary),
  };
}

export function normalizeCommandJournal(value) {
  const journal = object(value, "journal");
  if (journal.protocol !== JOURNAL_PROTOCOL) {
    throw contractError(`protocol 必须是 ${JOURNAL_PROTOCOL}。`);
  }
  if (!SOURCES.has(journal.source)) {
    throw contractError("source 不是 cli 或 web。");
  }
  const { events, lastSequence } = normalizeCommandEvents(journal.events, contractError);
  const nextCursor = integer(journal.nextCursor, "nextCursor");
  if (lastSequence > nextCursor) {
    throw contractError("nextCursor 不能早于最后一个事件。");
  }
  if (typeof journal.truncated !== "boolean") {
    throw contractError("truncated 必须是布尔值。");
  }
  return {
    protocol: JOURNAL_PROTOCOL,
    id: string(journal.id, "id"),
    address: string(journal.address, "address"),
    source: journal.source,
    ...normalizeOutcome(journal, "journal"),
    startedAtUnixMs: integer(journal.startedAtUnixMs, "startedAtUnixMs"),
    argumentCount: integer(journal.argumentCount, "argumentCount"),
    profileRevision: string(journal.profileRevision, "profileRevision"),
    nextCursor,
    events,
    truncated: journal.truncated,
  };
}

async function apiError(response, fallback) {
  try {
    const document = await response.json();
    return typeof document?.error === "string" && document.error
      ? document.error
      : fallback;
  } catch {
    return fallback;
  }
}

function locatorParts(locator) {
  string(locator, "command locator");
  const separator = locator.indexOf("/");
  if (separator <= 0 || separator === locator.length - 1 || locator.indexOf("/", separator + 1) >= 0) {
    throw contractError("命令定位值必须使用 <source>/<address> 格式。");
  }
  const source = locator.slice(0, separator);
  const address = locator.slice(separator + 1);
  return {
    address,
    encoded: `${encodeURIComponent(source)}/${encodeURIComponent(address)}`,
  };
}

export async function readCommandJournalHistory(locator, fetchJournal = fetch) {
  const { address, encoded } = locatorParts(locator);
  const response = await fetchJournal(
    `${JOURNALS_URL}?command=${encoded}`,
    { cache: "no-store", headers: { Accept: "application/json" } },
  );
  if (response.status !== 200) {
    throw new CommandJournalError(
      await apiError(response, `Host 返回 HTTP ${response.status}`),
      response.status,
    );
  }
  const history = normalizeCommandJournalHistory(await response.json());
  if (history.address !== address) {
    throw contractError("历史响应返回了不同的命令地址。");
  }
  return history;
}

export async function readCommandJournal(locator, id, after = 0, fetchJournal = fetch) {
  const { address, encoded } = locatorParts(locator);
  string(id, "run id");
  integer(after, "after");
  const response = await fetchJournal(
    `${JOURNALS_URL}/${encodeURIComponent(id)}?command=${encoded}&after=${after}`,
    { cache: "no-store", headers: { Accept: "application/json" } },
  );
  if (response.status !== 200) {
    throw new CommandJournalError(
      await apiError(response, `Host 返回 HTTP ${response.status}`),
      response.status,
    );
  }
  const journal = normalizeCommandJournal(await response.json());
  if (journal.id !== id || journal.address !== address) {
    throw contractError("日志响应与请求的命令运行不一致。");
  }
  if (journal.nextCursor < after || journal.events.some((event) => event.sequence <= after)) {
    throw contractError("日志响应没有从请求的 cursor 之后继续。");
  }
  return journal;
}

export async function openCommandJournalDirectory(locator, id, fetchJournal = fetch) {
  const { encoded } = locatorParts(locator);
  string(id, "run id");
  const response = await fetchJournal(
    `${JOURNALS_URL}/${encodeURIComponent(id)}/open-directory?command=${encoded}`,
    {
      method: "POST",
      headers: {
        Accept: "application/json",
        "X-SwawKit-Control": "open-journal-directory",
      },
    },
  );
  if (response.status !== 204) {
    throw new CommandJournalError(
      await apiError(response, `Host 返回 HTTP ${response.status}`),
      response.status,
    );
  }
}
