const PHASES = new Set(["guard-global", "guard-command", "run", "worker"]);
const PROGRESS_STATES = new Set(["running", "completed", "failed"]);
const PROGRESS_UNITS = new Set(["bytes", "items", "percent"]);
const PROGRESS_ID = /^[A-Za-z0-9._:-]{1,128}$/;

function object(value, field, contractError) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw contractError(`${field} 必须是对象。`);
  }
  return value;
}

function nonNegativeInteger(value, field, contractError) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw contractError(`${field} 必须是非负安全整数。`);
  }
  return value;
}

function nullableInteger(value, field, contractError) {
  return value === null ? null : nonNegativeInteger(value, field, contractError);
}

export function normalizeCommandEvents(values, contractError) {
  if (!Array.isArray(values)) {
    throw contractError("events 必须是数组。");
  }
  let previousSequence = 0;
  const events = values.map((value, index) => {
    const field = `events[${index}]`;
    const event = object(value, field, contractError);
    const sequence = nonNegativeInteger(event.sequence, `${field}.sequence`, contractError);
    if (sequence === 0 || sequence <= previousSequence) {
      throw contractError("events 必须按正整数 sequence 严格递增。");
    }
    previousSequence = sequence;
    const timestampUnixMs = nonNegativeInteger(
      event.timestampUnixMs,
      `${field}.timestampUnixMs`,
      contractError,
    );
    if (!PHASES.has(event.phase)) {
      throw contractError(`${field}.phase 不受支持。`);
    }
    const common = { sequence, timestampUnixMs, phase: event.phase, kind: event.kind };
    if (event.kind === "output") {
      if (event.stream !== "stdout" && event.stream !== "stderr") {
        throw contractError(`${field}.stream 必须是 stdout 或 stderr。`);
      }
      if (typeof event.text !== "string") {
        throw contractError(`${field}.text 必须是字符串。`);
      }
      return { ...common, stream: event.stream, text: event.text };
    }
    if (event.kind !== "progress") {
      throw contractError(`${field}.kind 不受支持。`);
    }
    if (typeof event.id !== "string" || !PROGRESS_ID.test(event.id)) {
      throw contractError(`${field}.id 不是有效的进度标识。`);
    }
    if (!PROGRESS_STATES.has(event.state)) {
      throw contractError(`${field}.state 不是有效的进度状态。`);
    }
    if (!PROGRESS_UNITS.has(event.unit)) {
      throw contractError(`${field}.unit 不是有效的进度单位。`);
    }
    if (
      typeof event.message !== "string"
      || event.message.length === 0
      || event.message.length > 512
      || /[\u0000-\u0008\u000a-\u001f\u007f]/u.test(event.message)
    ) {
      throw contractError(`${field}.message 不是有效的进度消息。`);
    }
    const current = nullableInteger(event.current, `${field}.current`, contractError);
    const total = nullableInteger(event.total, `${field}.total`, contractError);
    if (total !== null && (total === 0 || current === null || current > total)) {
      throw contractError(`${field} 的进度数值不一致。`);
    }
    return {
      ...common,
      id: event.id,
      state: event.state,
      current,
      total,
      unit: event.unit,
      message: event.message,
    };
  });
  return { events, lastSequence: previousSequence };
}
