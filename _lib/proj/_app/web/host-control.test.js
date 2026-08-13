import { describe, expect, test } from "bun:test";

import {
  HostControlError,
  createHostControlView,
  readHostStatus,
  requestHostRestart,
  requestHostShutdown,
} from "./host-control.js";

function controlElement() {
  return {
    addEventListener() {},
    dataset: {},
    disabled: false,
    hidden: false,
    textContent: "",
  };
}

describe("Host control client", () => {
  test("reads the exact runtime protocol", async () => {
    const document = {
      protocol: "swawkit.host-status/v1",
      entryKeySha256: "0".repeat(64),
      bootId: "123-boot",
      pid: 123,
      url: "http://127.0.0.1:43127/",
      runningReleaseId: "1".repeat(64),
      selectedReleaseId: "2".repeat(64),
      updateAvailable: true,
    };
    const result = await readHostStatus(async (url, options) => {
      expect(url).toBe("/api/v2/host");
      expect(options.cache).toBe("no-store");
      return { ok: true, json: async () => document };
    });
    expect(result).toEqual(document);
  });

  test("rejects malformed status instead of guessing", async () => {
    await expect(readHostStatus(async () => ({
      ok: true,
      json: async () => ({ protocol: "legacy", pid: 1, bootId: "boot" }),
    }))).rejects.toBeInstanceOf(HostControlError);
    await expect(readHostStatus(async () => ({
      ok: true,
      json: async () => ({
        protocol: "swawkit.host-status/v1",
        entryKeySha256: "0".repeat(64),
        bootId: "boot",
        pid: 1,
        url: "http://localhost:43127/",
        runningReleaseId: "1".repeat(64),
        selectedReleaseId: "1".repeat(64),
        updateAvailable: false,
      }),
    }))).rejects.toBeInstanceOf(HostControlError);
    await expect(readHostStatus(async () => ({
      ok: true,
      json: async () => ({
        protocol: "swawkit.host-status/v1",
        entryKeySha256: "0".repeat(64),
        bootId: "boot",
        pid: 1,
        url: "http://127.0.0.1:43127/",
        runningReleaseId: "1".repeat(64),
        selectedReleaseId: "2".repeat(64),
        updateAvailable: false,
      }),
    }))).rejects.toBeInstanceOf(HostControlError);
  });

  test("uses an explicit non-form header for shutdown", async () => {
    await requestHostShutdown(async (url, options) => {
      expect(url).toBe("/api/v2/host/shutdown");
      expect(options).toEqual({
        method: "POST",
        headers: { "X-SwawKit-Control": "shutdown" },
      });
      return { status: 202 };
    });
  });

  test("uses an explicit non-form header for restart", async () => {
    await requestHostRestart(async (url, options) => {
      expect(url).toBe("/api/v2/host/restart");
      expect(options).toEqual({
        method: "POST",
        headers: { "X-SwawKit-Control": "restart" },
      });
      return { status: 202 };
    });
  });

  test("reveals and submits restart only for a pending release", async () => {
    const elements = {
      hostIndicator: controlElement(),
      hostQuit: controlElement(),
      hostRestart: controlElement(),
      hostStatus: controlElement(),
    };
    const requests = [];
    const view = createHostControlView(elements, {
      confirmRestart: () => true,
      fetchImpl: async (url, options) => {
        requests.push([url, options]);
        if (url === "/api/v2/host") {
          return {
            ok: true,
            json: async () => ({
              protocol: "swawkit.host-status/v1",
              entryKeySha256: "0".repeat(64),
              bootId: "boot",
              pid: 17,
              url: "http://127.0.0.1:43127/",
              runningReleaseId: "1".repeat(64),
              selectedReleaseId: "2".repeat(64),
              updateAvailable: true,
            }),
          };
        }
        return { status: 202 };
      },
    });

    await view.load();
    expect(elements.hostRestart.hidden).toBe(false);
    expect(elements.hostStatus.textContent).toContain("新版本待重启");
    await view.restart();
    expect(requests[1]).toEqual([
      "/api/v2/host/restart",
      {
        method: "POST",
        headers: { "X-SwawKit-Control": "restart" },
      },
    ]);
    expect(elements.hostRestart.disabled).toBe(true);
  });
});
