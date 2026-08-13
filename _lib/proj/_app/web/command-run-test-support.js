export class FakeElement {
  constructor(tagName = "div") {
    this.tagName = tagName;
    this.children = [];
    this.parentElement = null;
    this.listeners = new Map();
    this.attributes = new Map();
    this.dataset = {};
    this.className = "";
    this.textContent = "";
    this.value = "";
    this.hidden = false;
    this.disabled = false;
    this.scrollHeight = 0;
    this.scrollTop = 0;
    this.clientHeight = 0;
  }

  append(...children) {
    for (const child of children) {
      child.parentElement = this;
      this.children.push(child);
      this.scrollHeight += child.textContent.length;
    }
  }

  replaceChildren(...children) {
    for (const child of this.children) {
      child.parentElement = null;
    }
    this.children = [];
    this.scrollHeight = 0;
    this.append(...children);
  }

  remove() {
    if (!this.parentElement) {
      return;
    }
    this.parentElement.children = this.parentElement.children
      .filter((child) => child !== this);
    this.parentElement = null;
  }

  querySelectorAll(selector) {
    const className = selector.startsWith(".") ? selector.slice(1) : "";
    const result = [];
    for (const child of this.children) {
      if (child.className.split(/\s+/).includes(className)) {
        result.push(child);
      }
      result.push(...child.querySelectorAll(selector));
    }
    return result;
  }

  addEventListener(type, listener) {
    this.listeners.set(type, listener);
  }

  dispatch(type) {
    this.listeners.get(type)?.({ preventDefault() {} });
  }

  setAttribute(name, value) {
    this.attributes.set(name, String(value));
  }

  removeAttribute(name) {
    this.attributes.delete(name);
  }

  focus() {
    this.focused = true;
  }

  get nextElementSibling() {
    if (!this.parentElement) {
      return null;
    }
    const index = this.parentElement.children.indexOf(this);
    return this.parentElement.children[index + 1] ?? null;
  }
}

export function elements() {
  const element = () => new FakeElement();
  return {
    commandRunAdd: element(),
    commandRunAddress: element(),
    commandRunActions: element(),
    commandRunArguments: element(),
    commandRunCancel: element(),
    commandRunConfirm: element(),
    commandRunConfirmation: element(),
    commandRunConfirmationText: element(),
    commandRunConfirmDismiss: element(),
    commandRunEditor: element(),
    commandRunEmpty: element(),
    commandRunExitCode: element(),
    commandRunFeedback: element(),
    commandRunForm: element(),
    commandRunOutput: element(),
    commandRunOperationList: element(),
    commandRunOperations: element(),
    commandRunResult: element(),
    commandRunSection: element(),
    commandRunState: element(),
    commandRunSubmit: element(),
    commandRunTruncated: element(),
  };
}

export function documentObject() {
  return { createElement: (tagName) => new FakeElement(tagName) };
}

export function storage(initial = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (name) => values.get(name) ?? null,
    setItem: (name, value) => values.set(name, value),
    removeItem: (name) => values.delete(name),
  };
}

export function scheduler() {
  let nextId = 1;
  const pending = new Map();
  return {
    clearTimer(id) {
      pending.delete(id);
    },
    setTimer(callback, delay) {
      const id = nextId++;
      pending.set(id, { callback, delay });
      return id;
    },
    take() {
      const [id, task] = pending.entries().next().value ?? [];
      if (id !== undefined) {
        pending.delete(id);
      }
      return task;
    },
  };
}

export function snapshot(overrides = {}) {
  return {
    protocol: "swawkit.command-run/v1",
    id: "run-1",
    address: ".dev.pwsh",
    state: "running",
    exitCode: null,
    error: null,
    nextCursor: 0,
    events: [],
    truncated: false,
    ...overrides,
  };
}

export function outputEvent(sequence, stream, text) {
  return {
    sequence,
    timestampUnixMs: 1000 + sequence,
    phase: "worker",
    kind: "output",
    stream,
    text,
  };
}

export function progressEvent(sequence, state = "running", overrides = {}) {
  return {
    sequence,
    timestampUnixMs: 1000 + sequence,
    phase: "worker",
    kind: "progress",
    id: "download:fixture.zip",
    state,
    current: null,
    total: null,
    unit: "bytes",
    message: "Downloading fixture.zip",
    ...overrides,
  };
}

export function response(status, document = null, location = null) {
  return {
    status,
    headers: { get: () => location },
    async json() { return document; },
  };
}

export async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

export function deferred() {
  let resolve;
  const promise = new Promise((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}
