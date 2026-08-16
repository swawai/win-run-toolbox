import { normalizeCommandEvents } from "./command-event-client.js";

export const RUN_JOURNAL_PROTOCOL = "swawkit.command-run-journal/v1";

const SOURCES = new Set(["cli", "web"]);
const STATES = new Set(["running", "exited", "canceled", "failed"]);

function invalid(message) {
  return new Error(`运行投影协议无效：${message}`);
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

function integer(value, field) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw invalid(`${field} 必须是非负安全整数。`);
  }
  return value;
}

function nullableInteger(value, field) {
  return value === null ? null : integer(value, field);
}

function outcome(document_) {
  if (!STATES.has(document_.state)) {
    throw invalid("state 不受支持。");
  }
  const finishedAtUnixMs = nullableInteger(document_.finishedAtUnixMs, "finishedAtUnixMs");
  const exitCode = document_.exitCode;
  if (exitCode !== null && !Number.isSafeInteger(exitCode)) {
    throw invalid("exitCode 必须是整数或 null。");
  }
  if (document_.error !== null && typeof document_.error !== "string") {
    throw invalid("error 必须是字符串或 null。");
  }
  const valid = document_.state === "running"
    ? finishedAtUnixMs === null && exitCode === null && document_.error === null
    : document_.state === "exited"
      ? finishedAtUnixMs !== null && Number.isSafeInteger(exitCode) && document_.error === null
      : document_.state === "canceled"
        ? finishedAtUnixMs !== null && exitCode === null && document_.error === null
        : finishedAtUnixMs !== null
          && exitCode === null
          && typeof document_.error === "string"
          && document_.error.length > 0;
  if (!valid) {
    throw invalid("终态字段与 state 不一致。");
  }
  return { error: document_.error, exitCode, finishedAtUnixMs, state: document_.state };
}

export function createRunProjection(value, expectedId) {
  const document_ = object(value, "run");
  if (document_.protocol !== RUN_JOURNAL_PROTOCOL) {
    throw invalid(`protocol 必须是 ${RUN_JOURNAL_PROTOCOL}。`);
  }
  const id = string(document_.id, "id");
  if (id !== expectedId) {
    throw invalid("id 与选中的 Run 不一致。");
  }
  if (!SOURCES.has(document_.source)) {
    throw invalid("source 必须是 cli 或 web。");
  }
  const { events, lastSequence } = normalizeCommandEvents(document_.events, invalid);
  const nextCursor = integer(document_.nextCursor, "nextCursor");
  if (lastSequence > nextCursor) {
    throw invalid("nextCursor 早于最后一个事件。");
  }
  if (typeof document_.truncated !== "boolean") {
    throw invalid("truncated 必须是布尔值。");
  }
  return {
    address: string(document_.address, "address"),
    argumentCount: integer(document_.argumentCount, "argumentCount"),
    events,
    id,
    nextCursor,
    profileRevision: string(document_.profileRevision, "profileRevision"),
    protocol: RUN_JOURNAL_PROTOCOL,
    source: document_.source,
    startedAtUnixMs: integer(document_.startedAtUnixMs, "startedAtUnixMs"),
    truncated: document_.truncated,
    ...outcome(document_),
  };
}
