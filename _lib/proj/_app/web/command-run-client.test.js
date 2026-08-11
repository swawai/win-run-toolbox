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

function outputEvent(sequence, stream, text) {
  return {
    sequence,
    timestampUnixMs: 1000 + sequence,
    phase: "worker",
    kind: "output",
    stream,
    text,
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
        outputEvent(1, "stdout", "out\n"),
        outputEvent(2, "stderr", "err\n"),
      ],
      truncated: true,
    });

    expect(normalizeCommandRunSnapshot(document)).toEqual(document);
  });

  test("normalizes structured progress beside ordinary output", () => {
    const progress = {
      sequence: 2,
      timestampUnixMs: 1002,
      phase: "worker",
      kind: "progress",
      id: "download:fixture.zip",
      state: "completed",
      current: 42,
      total: 42,
      unit: "bytes",
      message: "Downloaded fixture.zip",
    };
    const document = snapshot({
      nextCursor: 2,
      events: [outputEvent(1, "stdout", "prepare\n"), progress],
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
        outputEvent(2, "stdout", "two"),
        outputEvent(1, "stdout", "one"),
      ],
    }))).toThrow("严格递增");
    expect(() => normalizeCommandRunSnapshot(snapshot({
      nextCursor: 1,
      events: [{ ...outputEvent(1, "stdout", "one"), kind: "progress" }],
    }))).toThrow("进度标识");
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
