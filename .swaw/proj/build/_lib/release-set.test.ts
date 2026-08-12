import { afterEach, expect, test } from "bun:test";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { publishBuildReleaseSet } from "./release-set.ts";
import { acquireExclusiveFileLock } from "./windows-filesystem.ts";

const temporaryRoots: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })),
  );
});

test("publishes the PowerShell-compatible immutable Release Set identity", async () => {
  const root = await temporaryRoot();
  const commandDataRoot = join(root, "command");
  const work = join(commandDataRoot, "work");
  await mkdir(work, { recursive: true });
  const candidates = {
    "swawkit-proj.exe": await candidate(work, "swawkit-proj.exe", "core"),
    "swawkit-proj-host.exe": await candidate(work, "swawkit-proj-host.exe", "host"),
    "swawkit-proj-toolchain.exe": await candidate(work, "swawkit-proj-toolchain.exe", "toolchain"),
  };

  const id = await publishBuildReleaseSet(commandDataRoot, candidates);
  expect(id).toBe("414ab4df81e05a8b7ee16525c259949fc40234049f811a54b2a70194a8415bcc");
  expect((await readFile(join(commandDataRoot, "export", "current"), "utf8")).trim()).toBe(id);
  const state = JSON.parse(await readFile(join(commandDataRoot, "_state.json"), "utf8"));
  expect(state).toMatchObject({
    schema: "swawkit.command-provider-state/v1",
    status: "ready",
    inputRevision: `sha256-${id}`,
    producerContract: "swawkit.proj-build-app/v3",
  });
  expect(await publishBuildReleaseSet(commandDataRoot, candidates)).toBe(id);
});

test("rejects corruption in an existing immutable release", async () => {
  const root = await temporaryRoot();
  const commandDataRoot = join(root, "command");
  const work = join(commandDataRoot, "work");
  await mkdir(work, { recursive: true });
  const candidates = {
    "swawkit-proj.exe": await candidate(work, "swawkit-proj.exe", "core"),
    "swawkit-proj-host.exe": await candidate(work, "swawkit-proj-host.exe", "host"),
    "swawkit-proj-toolchain.exe": await candidate(work, "swawkit-proj-toolchain.exe", "toolchain"),
  };
  const id = await publishBuildReleaseSet(commandDataRoot, candidates);
  await writeFile(
    join(commandDataRoot, "export", "releases", id, "swawkit-proj.exe"),
    "evil",
  );
  expect(publishBuildReleaseSet(commandDataRoot, candidates)).rejects.toThrow(
    "artifact is corrupt",
  );
  const state = JSON.parse(await readFile(join(commandDataRoot, "_state.json"), "utf8"));
  expect(state.status).toBe("unavailable");
});

test("rejects an empty Release Set", async () => {
  const root = await temporaryRoot();
  const commandDataRoot = join(root, "command");
  await mkdir(commandDataRoot, { recursive: true });
  expect(publishBuildReleaseSet(commandDataRoot, {} as never)).rejects.toThrow(
    "at least one artifact",
  );
});

test("build lock is exclusive and reusable by the same process", async () => {
  const root = await temporaryRoot();
  const path = join(root, "build.lock");
  const first = await acquireExclusiveFileLock(path, 100);
  try {
    expect(acquireExclusiveFileLock(path, 100)).rejects.toThrow("cannot acquire build lock");
  } finally {
    first[Symbol.dispose]();
  }
  const second = await acquireExclusiveFileLock(path, 100);
  second[Symbol.dispose]();
});

async function temporaryRoot(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "swawkit-proj-build-"));
  temporaryRoots.push(root);
  return root;
}

async function candidate(root: string, name: string, content: string): Promise<string> {
  const path = join(root, name);
  await writeFile(path, content);
  return path;
}
