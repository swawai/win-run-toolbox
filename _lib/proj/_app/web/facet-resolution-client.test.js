import { describe, expect, test } from "bun:test";

import {
  createCollectionResolutionLoader,
  resolveFacet,
} from "./facet-resolution-client.js";

function response(document) {
  return { json: async () => document, ok: true, status: 200 };
}

describe("Facet resolution client", () => {
  test("posts a command Subject and selected Facet", async () => {
    let request = null;
    const command = { address: ".check", source: "kernel" };
    const facet = {
      id: "status",
      kind: "projection",
      resolver: { returns: "swawkit.module-check/v1", type: "command" },
    };
    const document = { protocol: "swawkit.module-check/v1" };
    const result = await resolveFacet({}, command, facet, {
      fetchImpl: async (url, options) => {
        request = { options, url };
        return response(document);
      },
    });

    expect(result).toBe(document);
    expect(request.url).toBe("/api/v2/facet-resolutions");
    expect(request.options.method).toBe("POST");
    expect(JSON.parse(request.options.body)).toEqual({
      facet: "status",
      subject: { address: ".check", source: "kernel", type: "command" },
    });
  });

  test("includes collection provenance for an instance Subject", async () => {
    let body = null;
    const subject = { ref: { type: "instance", kind: "run", id: "run-01" } };
    const facet = {
      id: "overview",
      kind: "projection",
      resolver: { returns: "swawkit.run/v1", type: "command" },
    };
    const via = {
      facet: "runs",
      subject: { address: ".logs", source: "kernel", type: "command" },
    };
    await resolveFacet({}, subject, facet, {
      fetchImpl: async (_url, options) => {
        body = JSON.parse(options.body);
        return response({ protocol: "swawkit.run/v1" });
      },
      via,
    });
    expect(body).toEqual({ facet: "overview", subject: subject.ref, via });
  });

  test("rejects instance-owned collections instead of exposing half-usable children", async () => {
    const subject = { ref: { type: "instance", kind: "run", id: "run-01" } };
    await expect(resolveFacet({}, subject, {
      id: "artifacts",
      kind: "collection",
    }, {
      fetchImpl: async () => { throw new Error("must not fetch"); },
      via: {
        facet: "runs",
        subject: { address: ".logs", source: "kernel", type: "command" },
      },
    })).rejects.toThrow("recursive provenance");
  });

  test("does not let stale collection responses or errors replace the latest state", async () => {
    const pending = [];
    const resolved = [];
    const errors = [];
    const loader = createCollectionResolutionLoader({
      onError(_owner, _facet, error) { errors.push(error.message); },
      onLoading() {},
      onResolved(collection) { resolved.push(collection.value); },
      resolveCollection() {
        return new Promise((resolve, reject) => pending.push({ reject, resolve }));
      },
    });

    const staleResponse = loader.load(".logs", "runs");
    const currentResponse = loader.load(".logs", "runs");
    pending[1].resolve({ value: "current" });
    expect(await currentResponse).toEqual({ value: "current" });
    pending[0].resolve({ value: "stale" });
    expect(await staleResponse).toBeNull();

    const staleError = loader.load(".logs", "runs");
    const newestResponse = loader.load(".logs", "runs");
    pending[3].resolve({ value: "newest" });
    await newestResponse;
    pending[2].reject(new Error("stale failure"));
    expect(await staleError).toBeNull();

    expect(resolved).toEqual(["current", "newest"]);
    expect(errors).toEqual([]);
  });
});
