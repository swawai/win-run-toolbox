const COMMAND_RUNS_URL = "/api/v2/command-runs";
const COMMAND_RUN_PROTOCOL = "swawkit.command-run/v1";
const COMMAND_RUN_STATES = new Set([
  "running",
  "canceling",
  "exited",
  "canceled",
  "failed",
]);

export class CommandRunError extends Error {
  constructor(message, status = 0) {
    super(message);
    this.name = "CommandRunError";
    this.status = status;
  }
}

function contractError(message) {
  return new CommandRunError(`命令执行协议无效：${message}`);
}

function requireObject(value, field) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw contractError(`${field} 必须是对象。`);
  }
  return value;
}

function requireString(value, field, { allowEmpty = false } = {}) {
  if (typeof value !== "string" || (!allowEmpty && value.length === 0)) {
    throw contractError(`${field} 必须是${allowEmpty ? "" : "非空"}字符串。`);
  }
  return value;
}

export function normalizeCommandRunSnapshot(value) {
  const snapshot = requireObject(value, "snapshot");
  if (snapshot.protocol !== COMMAND_RUN_PROTOCOL) {
    throw contractError(`protocol 必须是 ${COMMAND_RUN_PROTOCOL}。`);
  }
  const id = requireString(snapshot.id, "id");
  const address = requireString(snapshot.address, "address");
  if (!COMMAND_RUN_STATES.has(snapshot.state)) {
    throw contractError("state 不是受支持的状态。");
  }
  if (
    snapshot.exitCode !== null
    && !Number.isSafeInteger(snapshot.exitCode)
  ) {
    throw contractError("exitCode 必须是整数或 null。");
  }
  if (snapshot.state === "exited" && !Number.isSafeInteger(snapshot.exitCode)) {
    throw contractError("exited 状态必须提供 exitCode。");
  }
  if (snapshot.error !== null && typeof snapshot.error !== "string") {
    throw contractError("error 必须是字符串或 null。");
  }
  if (snapshot.state !== "exited" && snapshot.exitCode !== null) {
    throw contractError("只有 exited 状态可以提供 exitCode。");
  }
  if (snapshot.state === "failed") {
    if (typeof snapshot.error !== "string" || snapshot.error.length === 0) {
      throw contractError("failed 状态必须提供非空 error。");
    }
  } else if (snapshot.error !== null) {
    throw contractError("只有 failed 状态可以提供 error。");
  }
  if (!Number.isSafeInteger(snapshot.nextCursor) || snapshot.nextCursor < 0) {
    throw contractError("nextCursor 必须是非负整数。");
  }
  if (!Array.isArray(snapshot.events)) {
    throw contractError("events 必须是数组。");
  }

  let previousSequence = 0;
  const events = snapshot.events.map((value, index) => {
    const event = requireObject(value, `events[${index}]`);
    if (!Number.isSafeInteger(event.sequence) || event.sequence <= previousSequence) {
      throw contractError("events 必须按正整数 sequence 严格递增。");
    }
    previousSequence = event.sequence;
    if (event.stream !== "stdout" && event.stream !== "stderr") {
      throw contractError(`events[${index}].stream 必须是 stdout 或 stderr。`);
    }
    return {
      sequence: event.sequence,
      stream: event.stream,
      text: requireString(event.text, `events[${index}].text`, { allowEmpty: true }),
    };
  });
  if (previousSequence > snapshot.nextCursor) {
    throw contractError("nextCursor 不能早于最后一个事件。");
  }
  if (typeof snapshot.truncated !== "boolean") {
    throw contractError("truncated 必须是布尔值。");
  }

  return {
    protocol: COMMAND_RUN_PROTOCOL,
    id,
    address,
    state: snapshot.state,
    exitCode: snapshot.exitCode,
    error: snapshot.error,
    nextCursor: snapshot.nextCursor,
    events,
    truncated: snapshot.truncated,
  };
}

async function readApiError(response, fallback) {
  try {
    const document = await response.json();
    if (typeof document?.error === "string" && document.error) {
      return document.error;
    }
  } catch {
    // The HTTP status and fallback remain sufficient for a non-JSON error.
  }
  return fallback;
}

export async function startCommandRun(address, arguments_, fetchRun = fetch) {
  if (typeof address !== "string" || address.length === 0) {
    throw new CommandRunError("命令地址不能为空。");
  }
  if (!Array.isArray(arguments_) || arguments_.some((value) => typeof value !== "string")) {
    throw new CommandRunError("命令参数必须是字符串数组。");
  }
  const response = await fetchRun(COMMAND_RUNS_URL, {
    method: "POST",
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ address, arguments: arguments_ }),
  });
  if (response.status !== 201) {
    throw new CommandRunError(
      await readApiError(response, `Host 返回 HTTP ${response.status}`),
      response.status,
    );
  }
  const location = response.headers.get("location");
  if (!location) {
    throw contractError("创建响应缺少 Location。");
  }
  const snapshot = normalizeCommandRunSnapshot(await response.json());
  const expectedLocation = `${COMMAND_RUNS_URL}/${encodeURIComponent(snapshot.id)}`;
  if (location !== expectedLocation) {
    throw contractError("创建响应的 Location 与 run id 不一致。");
  }
  return snapshot;
}

export async function readCommandRun(id, after, fetchRun = fetch) {
  requireString(id, "run id");
  if (!Number.isSafeInteger(after) || after < 0) {
    throw new CommandRunError("命令输出 cursor 必须是非负整数。");
  }
  const response = await fetchRun(
    `${COMMAND_RUNS_URL}/${encodeURIComponent(id)}?after=${after}`,
    { cache: "no-store", headers: { Accept: "application/json" } },
  );
  if (response.status !== 200) {
    throw new CommandRunError(
      await readApiError(response, `Host 返回 HTTP ${response.status}`),
      response.status,
    );
  }
  const snapshot = normalizeCommandRunSnapshot(await response.json());
  if (snapshot.id !== id) {
    throw contractError("轮询响应返回了不同的 run id。");
  }
  if (
    snapshot.nextCursor < after
    || snapshot.events.some((event) => event.sequence <= after)
  ) {
    throw contractError("轮询响应没有从请求的 cursor 之后继续。");
  }
  return snapshot;
}

export async function cancelCommandRun(id, fetchRun = fetch) {
  requireString(id, "run id");
  const response = await fetchRun(
    `${COMMAND_RUNS_URL}/${encodeURIComponent(id)}`,
    { method: "DELETE", headers: { Accept: "application/json" } },
  );
  if (response.status !== 204) {
    throw new CommandRunError(
      await readApiError(response, `Host 返回 HTTP ${response.status}`),
      response.status,
    );
  }
}
