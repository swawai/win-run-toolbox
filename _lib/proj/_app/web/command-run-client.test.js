import { describe, expect, test } from "bun:test";

import {
  cancelCommandRun,
  CommandRunError,
  normalizeCommandRunSnapshot,
  readCommandRun,
  startCommandRun,
} from "./command-run-client.js";

function snapshot(overrides = {}) {
  return {
    protocol: "swawkit.command-run/v1",
    id: "run-17",
    address: ".dev.status",
    state: "running",
    exitCode: null,
    error: null,
    nextCursor: 0,
    events: [],
    truncated: false,
    ...overrides,
  };
}

function response(status, document = null, location = null) {
  return {
    status,
    headers: {
      get(name) {
        return name.toLowerCase() === "location" ? location : null;
      },
    },
    async json() {
      return document;
    },
  };
}

describe("command run protocol client", () => {
  test("normalizes ordered stdout and stderr events", () => {
    const document = snapshot({
      state: "exited",
      exitCode: 7,
      nextCursor: 2,
      events: [
        { sequence: 1, stream: "stdout", text: "out\n" },
        { sequence: 2, stream: "stderr", text: "err\n" },
      ],
      truncated: true,
    });

    expect(normalizeCommandRunSnapshot(document)).toEqual(document);
  });

  test("rejects unknown states, invalid exit codes, and unordered events", () => {
    expect(() => normalizeCommandRunSnapshot(snapshot({ state: "queued" })))
      .toThrow("state 不是受支持的状态");
    expect(() => normalizeCommandRunSnapshot(snapshot({
      state: "exited",
      exitCode: null,
    }))).toThrow("exited 状态必须提供 exitCode");
    expect(() => normalizeCommandRunSnapshot(snapshot({
      nextCursor: 2,
      events: [
        { sequence: 2, stream: "stdout", text: "two" },
        { sequence: 1, stream: "stdout", text: "one" },
      ],
    }))).toThrow("严格递增");
  });

  test("enforces state-specific exit and error fields", () => {
    expect(() => normalizeCommandRunSnapshot(snapshot({ exitCode: 3 }))).toThrow();
    expect(() => normalizeCommandRunSnapshot(snapshot({
      state: "failed",
      error: null,
    }))).toThrow();
    expect(() => normalizeCommandRunSnapshot(snapshot({
      state: "exited",
      exitCode: 0,
      error: "unexpected",
    }))).toThrow();
    expect(normalizeCommandRunSnapshot(snapshot({
      state: "failed",
      error: "worker failed",
    })).error).toBe("worker failed");
  });

  test("starts one exact argv invocation and requires Location", async () => {
    let request;
    const created = snapshot();
    const result = await startCommandRun(
      ".dev.ps",
      ["-Command", "Write-Host 'A B'", ""],
      async (url, options) => {
        request = { url, options };
        return response(201, created, "/api/v2/command-runs/run-17");
      },
    );

    expect(request.url).toBe("/api/v2/command-runs");
    expect(request.options.method).toBe("POST");
    expect(JSON.parse(request.options.body)).toEqual({
      address: ".dev.ps",
      arguments: ["-Command", "Write-Host 'A B'", ""],
    });
    expect(result).toEqual(created);

    await expect(startCommandRun(".dev.status", [], async () => (
      response(201, created)
    ))).rejects.toThrow("缺少 Location");

    await expect(startCommandRun(".dev.status", [], async () => (
      response(201, created, "/api/v2/command-runs/run-other")
    ))).rejects.toThrow("Location 与 run id 不一致");
  });

  test("rejects an empty address before making a request", async () => {
    let requested = false;
    await expect(startCommandRun("", [], async () => {
      requested = true;
    })).rejects.toThrow("命令地址不能为空");
    expect(requested).toBe(false);
  });

  test("polls from the declared cursor and URL-encodes the run id", async () => {
    let request;
    const exited = snapshot({
      id: "run/17",
      state: "exited",
      exitCode: 0,
      nextCursor: 8,
    });
    const result = await readCommandRun("run/17", 8, async (url, options) => {
      request = { url, options };
      return response(200, exited);
    });

    expect(request.url).toBe("/api/v2/command-runs/run%2F17?after=8");
    expect(request.options.cache).toBe("no-store");
    expect(result.state).toBe("exited");
  });

  test("rejects a mismatched run or a cursor regression", async () => {
    await expect(readCommandRun("run-17", 8, async () => (
      response(200, snapshot({ id: "run-other", nextCursor: 8 }))
    ))).rejects.toThrow("不同的 run id");

    await expect(readCommandRun("run-17", 8, async () => (
      response(200, snapshot({ nextCursor: 7 }))
    ))).rejects.toThrow("cursor 之后继续");
  });

  test("DELETE requires the idempotent 204 contract", async () => {
    let request;
    await cancelCommandRun("run-17", async (url, options) => {
      request = { url, options };
      return response(204);
    });
    expect(request).toEqual({
      url: "/api/v2/command-runs/run-17",
      options: {
        method: "DELETE",
        headers: { Accept: "application/json" },
      },
    });

    await expect(cancelCommandRun("run-17", async () => (
      response(409, { error: "cannot cancel" })
    ))).rejects.toEqual(new CommandRunError("cannot cancel", 409));
  });
});
