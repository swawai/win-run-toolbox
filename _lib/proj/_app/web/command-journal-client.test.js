import { describe, expect, test } from "bun:test";

import {
  normalizeCommandJournal,
  normalizeCommandJournalHistory,
  readCommandJournal,
  readCommandJournalHistory,
} from "./command-journal-client.js";

function summary(overrides = {}) {
  return {
    id: "run-17",
    source: "cli",
    state: "exited",
    startedAtUnixMs: 1000,
    finishedAtUnixMs: 1200,
    exitCode: 0,
    error: null,
    argumentCount: 2,
    eventCount: 1,
    truncated: false,
    ...overrides,
  };
}

function journal(overrides = {}) {
  return {
    protocol: "swawkit.command-run-journal/v1",
    id: "run-17",
    address: ".dev.status",
    source: "cli",
    state: "exited",
    startedAtUnixMs: 1000,
    finishedAtUnixMs: 1200,
    exitCode: 0,
    error: null,
    argumentCount: 2,
    profileRevision: "sha256-fixture",
    nextCursor: 1,
    events: [{
      sequence: 1,
      timestampUnixMs: 1100,
      phase: "run",
      stream: "stdout",
      text: "ready\n",
    }],
    truncated: false,
    ...overrides,
  };
}

function response(status, document) {
  return { status, async json() { return document; } };
}

describe("command journal protocol client", () => {
  test("normalizes history and phase-aware JSONL events", () => {
    const history = {
      protocol: "swawkit.command-run-history/v1",
      address: ".dev.status",
      runs: [summary()],
    };
    expect(normalizeCommandJournalHistory(history)).toEqual(history);
    expect(normalizeCommandJournal(journal())).toEqual(journal());
  });

  test("rejects inconsistent terminal fields and event order", () => {
    expect(() => normalizeCommandJournalHistory({
      protocol: "swawkit.command-run-history/v1",
      address: ".dev.status",
      runs: [summary({ state: "failed", error: null, exitCode: null })],
    })).toThrow("终态字段不一致");
    expect(() => normalizeCommandJournal(journal({
      nextCursor: 2,
      events: [
        journal().events[0],
        { ...journal().events[0], sequence: 1 },
      ],
    }))).toThrow("严格递增");
    expect(() => normalizeCommandJournal(journal({
      events: [{ ...journal().events[0], phase: "setup" }],
    }))).toThrow("phase 不受支持");
  });

  test("reads exact command history and incremental journal URLs", async () => {
    const requests = [];
    const historyDocument = {
      protocol: "swawkit.command-run-history/v1",
      address: ".dev.status",
      runs: [summary()],
    };
    await readCommandJournalHistory("kernel/.dev.status", async (url, options) => {
      requests.push({ url, options });
      return response(200, historyDocument);
    });
    await readCommandJournal("kernel/.dev.status", "run/17", 1, async (url, options) => {
      requests.push({ url, options });
      return response(200, journal({
        id: "run/17",
        nextCursor: 2,
        events: [{ ...journal().events[0], sequence: 2 }],
      }));
    });

    expect(requests[0].url)
      .toBe("/api/v2/command-run-journals?command=kernel%2F.dev.status");
    expect(requests[1].url)
      .toBe("/api/v2/command-run-journals/run%2F17?command=kernel%2F.dev.status&after=1");
    expect(requests.every(({ options }) => options.cache === "no-store")).toBeTrue();
  });

  test("surfaces API errors and rejects mismatched identities", async () => {
    await expect(readCommandJournalHistory("kernel/.dev.status", async () => (
      response(500, { error: "journal unavailable" })
    ))).rejects.toThrow("journal unavailable");

    await expect(readCommandJournal("kernel/.dev.status", "run-17", 0, async () => (
      response(200, journal({ address: ".other" }))
    ))).rejects.toThrow("不一致");
  });
});
