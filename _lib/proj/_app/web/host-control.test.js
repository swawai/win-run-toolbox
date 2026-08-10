import { describe, expect, test } from "bun:test";

import {
  HostControlError,
  readHostStatus,
  requestHostShutdown,
} from "./host-control.js";

describe("Host control client", () => {
  test("reads the exact runtime protocol", async () => {
    const document = {
      protocol: "swawkit.host-runtime/v1",
      entryKeySha256: "0".repeat(64),
      bootId: "123-boot",
      pid: 123,
      url: "http://127.0.0.1:43127/",
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
        protocol: "swawkit.host-runtime/v1",
        entryKeySha256: "0".repeat(64),
        bootId: "boot",
        pid: 1,
        url: "http://localhost:43127/",
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
});
