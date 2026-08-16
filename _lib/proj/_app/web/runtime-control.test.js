import { describe, expect, test } from "bun:test";

import {
  RuntimeControlError,
  createRuntimeControlView,
  readRuntimeStatus,
  requestHostRestart,
  requestHostShutdown,
  requestRuntimeCleanup,
  runtimeRootPresentation,
} from "./runtime-control.js";

function hostStatus(updateAvailable = true) {
  return {
    protocol: "swawkit.host-status/v1",
    entryKeySha256: "0".repeat(64),
    bootId: "123-boot",
    pid: 123,
    url: "http://127.0.0.1:43127/",
    runningReleaseId: "1".repeat(64),
    selectedReleaseId: updateAvailable ? "2".repeat(64) : "1".repeat(64),
    updateAvailable,
  };
}

function runtimeElements() {
  const element = () => ({
    dataset: {},
    disabled: false,
    hidden: false,
    textContent: "",
    addEventListener() {},
    replaceChildren() {},
  });
  return {
    genericCommandOverview: element(),
    runtimeCleanupApply: element(),
    runtimeCleanupFeedback: element(),
    runtimeCleanupList: element(),
    runtimeCleanupPreview: element(),
    runtimeCleanupResult: element(),
    runtimeCleanupSection: element(),
    runtimeCleanupSummary: element(),
    runtimeControl: element(),
    runtimeDescription: element(),
    runtimeHostActions: element(),
    runtimeHostConnection: element(),
    runtimeHostExit: element(),
    runtimeHostFeedback: element(),
    runtimeHostPid: element(),
    runtimeHostProperties: element(),
    runtimeHostRestart: element(),
    runtimeHostSection: element(),
    runtimeHostStatus: element(),
    runtimeReleaseCount: element(),
    runtimeRunningRelease: element(),
    runtimeSelectedRelease: element(),
    runtimeTitle: element(),
  };
}

describe("Runtime control client", () => {
  test("reads and validates the aggregate Runtime protocol", async () => {
    const document = {
      protocol: "swawkit.runtime-status/v1",
      selectedReleaseId: "2".repeat(64),
      releaseCount: 3,
      host: hostStatus(true),
    };
    const result = await readRuntimeStatus(async (url, options) => {
      expect(url).toBe("/api/v2/runtime");
      expect(options.cache).toBe("no-store");
      return { ok: true, json: async () => document };
    });
    expect(result).toEqual(document);
    expect(runtimeRootPresentation(result)).toEqual({
      icon: "●",
      summary: "新版本待重启",
      tone: "warning",
    });
  });

  test("rejects inconsistent Runtime state instead of guessing", async () => {
    await expect(readRuntimeStatus(async () => ({
      ok: true,
      json: async () => ({
        protocol: "swawkit.runtime-status/v1",
        selectedReleaseId: "3".repeat(64),
        releaseCount: 2,
        host: hostStatus(true),
      }),
    }))).rejects.toBeInstanceOf(RuntimeControlError);
  });

  test("uses exact non-form headers for Host controls", async () => {
    const requests = [];
    const fetchImpl = async (url, options) => {
      requests.push([url, options]);
      return { status: 202 };
    };
    await requestHostShutdown(fetchImpl);
    await requestHostRestart(fetchImpl);
    expect(requests).toEqual([
      ["/api/v2/host/shutdown", {
        method: "POST",
        headers: { "X-SwawKit-Control": "shutdown" },
      }],
      ["/api/v2/host/restart", {
        method: "POST",
        headers: { "X-SwawKit-Control": "restart" },
      }],
    ]);
  });

  test("keeps the Runtime root read-only and actions local to their subcommands", async () => {
    const elements = runtimeElements();
    const document = {
      protocol: "swawkit.runtime-status/v1",
      selectedReleaseId: "1".repeat(64),
      releaseCount: 3,
      host: hostStatus(false),
    };
    const view = createRuntimeControlView(elements, {
      fetchImpl: async () => ({ ok: true, json: async () => document }),
    });

    expect(view.select({ handler: "runtime.status" })).toBe(true);
    await view.load();
    expect(elements.runtimeHostSection.hidden).toBe(false);
    expect(elements.runtimeHostActions.hidden).toBe(true);
    expect(elements.runtimeHostFeedback.hidden).toBe(true);
    expect(elements.runtimeHostExit.hidden).toBe(true);
    expect(elements.runtimeHostRestart.hidden).toBe(true);
    expect(elements.runtimeCleanupSection.hidden).toBe(true);

    expect(view.select({ handler: "host.restart" })).toBe(true);
    await view.load();
    expect(elements.runtimeHostActions.hidden).toBe(false);
    expect(elements.runtimeHostExit.hidden).toBe(true);
    expect(elements.runtimeHostRestart.hidden).toBe(false);
    expect(elements.runtimeHostRestart.disabled).toBe(true);
    expect(elements.runtimeHostRestart.dataset.busy).not.toBe("true");

    expect(view.select({ handler: "runtime.cleanup" })).toBe(true);
    expect(elements.runtimeHostSection.hidden).toBe(true);
    expect(elements.runtimeCleanupSection.hidden).toBe(false);
  });

  test("keeps cleanup preview and apply explicit in the protocol", async () => {
    for (const apply of [false, true]) {
      const state = apply ? "removed" : "removable";
      const document = {
        protocol: "swawkit.runtime-cleanup/v1",
        action: apply ? "apply" : "preview",
        items: [
          {
            releaseId: "2".repeat(64),
            state: "selected",
            pids: [],
            reason: null,
          },
          {
            releaseId: "3".repeat(64),
            state,
            pids: [],
            reason: null,
          },
        ],
        summary: {
          selected: 1,
          inUse: 0,
          removable: apply ? 0 : 1,
          removed: apply ? 1 : 0,
          retained: 0,
        },
      };
      const result = await requestRuntimeCleanup(apply, async (url, options) => {
        expect(url).toBe("/api/v2/runtime/cleanup");
        expect(options.headers["X-SwawKit-Control"]).toBe(
          apply ? "runtime-cleanup-apply" : "runtime-cleanup-preview",
        );
        return { ok: true, json: async () => document };
      });
      expect(result).toEqual(document);
    }
  });

  test("rejects cleanup documents with impossible item semantics", async () => {
    const invalid = {
      protocol: "swawkit.runtime-cleanup/v1",
      action: "preview",
      items: [
        {
          releaseId: "2".repeat(64),
          state: "selected",
          pids: [],
          reason: null,
        },
        {
          releaseId: "3".repeat(64),
          state: "removed",
          pids: [],
          reason: null,
        },
      ],
      summary: {
        selected: 1,
        inUse: 0,
        removable: 0,
        removed: 1,
        retained: 0,
      },
    };

    await expect(requestRuntimeCleanup(false, async () => ({
      ok: true,
      json: async () => invalid,
    }))).rejects.toBeInstanceOf(RuntimeControlError);
  });
});
