const CONTEXT_SCHEMA = "swawkit.context/v1";

function invalid(message) {
  return new Error(`Invalid Context projection protocol: ${message}`);
}

function object(value, field) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw invalid(`${field} must be an object.`);
  }
  return value;
}

export function createContextProjection(document, expectedId) {
  const payload = object(document, "Context");
  if (payload.schema !== CONTEXT_SCHEMA || payload.id !== expectedId) {
    throw invalid("Context schema or ID does not match the selected Subject.");
  }
  if (!Array.isArray(payload.commands) || !Array.isArray(payload.notes)) {
    throw invalid("Context commands and notes must be arrays.");
  }
  const commands = payload.commands.map((raw, index) => {
    const command = object(raw, `commands[${index}]`);
    if (
      !new Set(["kernel", "action", "control"]).has(command.source)
      || typeof command.address !== "string"
      || command.address.length === 0
    ) {
      throw invalid(`commands[${index}] is invalid.`);
    }
    return { address: command.address, source: command.source };
  });
  if (payload.notes.some((note) => typeof note !== "string")) {
    throw invalid("Context notes must contain strings.");
  }
  if (typeof payload.prompt !== "string") {
    throw invalid("Context prompt must be a string.");
  }
  return {
    commands,
    id: payload.id,
    notes: [...payload.notes],
    prompt: payload.prompt,
    schema: CONTEXT_SCHEMA,
  };
}
