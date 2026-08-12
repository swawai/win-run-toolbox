import { afterEach, expect, test } from "bun:test";
import {
  copyFile,
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  rm,
  symlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readReadyBuildReleaseSet } from "../../build/_lib/provider-release.ts";
import { publishBuildReleaseSet } from "../../build/_lib/release-set.ts";
import { publishRuntimeReleaseSet } from "./runtime-release.ts";

const temporaryRoots: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryRoots.splice(0).map((root) => rm(root, { recursive: true, force: true })),
  );
});

test("atomically selects a new runtime while the previous release remains mapped", async () => {
  const fixture = await runtimeFixture();
  const previous = await publishFixtureRelease(fixture, {
    "swawkit-proj.exe": systemExecutable("cmd.exe"),
    "swawkit-proj-host.exe": systemExecutable("where.exe"),
    "swawkit-proj-toolchain.exe": systemExecutable("whoami.exe"),
  });
  const running = Bun.spawn(
    [
      join(fixture.runtimeRoot, "releases", previous, "swawkit-proj.exe"),
      "/d",
      "/c",
      "ping -n 8 127.0.0.1 >nul",
    ],
    { stdout: "ignore", stderr: "ignore", windowsHide: true },
  );
  try {
    await Bun.sleep(400);
    expect(running.exitCode).toBeNull();

    const current = await publishFixtureRelease(fixture, {
      "swawkit-proj.exe": systemExecutable("where.exe"),
      "swawkit-proj-host.exe": systemExecutable("whoami.exe"),
      "swawkit-proj-toolchain.exe": systemExecutable("hostname.exe"),
    });
    expect(current).not.toBe(previous);
    expect(running.exitCode).toBeNull();
    expect(await readFile(join(fixture.runtimeRoot, "current"), "utf8")).toBe(`${current}\n`);
    expect(await readFile(join(fixture.runtimeRoot, "releases", current, "manifest.json"), "utf8"))
      .toContain(current);
    for (const name of ["swawkit-proj.exe", "swawkit-proj-host.exe", "swawkit-proj-toolchain.exe"]) {
      await expect(lstat(join(fixture.runtimeRoot, name))).rejects.toMatchObject({ code: "ENOENT" });
    }

    const releaseRoot = join(fixture.runtimeRoot, "releases", current);
    const before = (await lstat(releaseRoot)).birthtimeMs;
    const selected = await readReadyBuildReleaseSet(fixture.dataRoot, "fixture");
    await writeFile(join(fixture.runtimeRoot, "current"), "invalid".repeat(1024));
    expect(await publishRuntimeReleaseSet(fixture.home, fixture.cacheRoot, selected)).toBe(current);
    expect(await readFile(join(fixture.runtimeRoot, "current"), "utf8")).toBe(`${current}\n`);
    expect((await lstat(releaseRoot)).birthtimeMs).toBe(before);
    expect((await readdir(fixture.runtimeRoot)).filter((name) => name.startsWith("."))).toEqual([]);

    await writeFile(join(releaseRoot, "swawkit-proj-host.exe"), "coherently-tampered-host");
    expect(publishRuntimeReleaseSet(fixture.home, fixture.cacheRoot, selected)).rejects.toThrow(
      "artifact is corrupt",
    );
  } finally {
    running.kill();
    await running.exited;
  }
});

test("rejects a runtime releases parent junction", async () => {
  const fixture = await runtimeFixture();
  const external = join(fixture.root, "external-releases");
  await mkdir(fixture.runtimeRoot, { recursive: true });
  await mkdir(external);
  await symlink(external, join(fixture.runtimeRoot, "releases"), "junction");
  const release = await buildFixtureRelease(fixture, {
    "swawkit-proj.exe": systemExecutable("where.exe"),
    "swawkit-proj-host.exe": systemExecutable("whoami.exe"),
    "swawkit-proj-toolchain.exe": systemExecutable("hostname.exe"),
  });

  expect(publishRuntimeReleaseSet(fixture.home, fixture.cacheRoot, release)).rejects.toThrow(
    "runtime releases must be a regular directory",
  );
  expect(await readdir(external)).toEqual([]);
});

type Fixture = Awaited<ReturnType<typeof runtimeFixture>>;
type Candidates = Record<
  "swawkit-proj.exe" | "swawkit-proj-host.exe" | "swawkit-proj-toolchain.exe",
  string
>;

async function runtimeFixture() {
  const root = await mkdtemp(join(tmpdir(), "swawkit-proj-publish-"));
  temporaryRoots.push(root);
  const home = join(root, "home");
  const dataRoot = join(root, "data");
  const commandRoot = join(dataRoot, "modules", "action", "proj", "build", "app");
  const cacheRoot = join(home, "data", "proj_cache");
  const runtimeRoot = join(home, "_lib", "proj", "_bin");
  await mkdir(join(home, "_lib", "proj"), { recursive: true });
  await mkdir(cacheRoot, { recursive: true });
  return { root, home, dataRoot, commandRoot, cacheRoot, runtimeRoot, generation: 0 };
}

async function publishFixtureRelease(fixture: Fixture, sources: Candidates): Promise<string> {
  const release = await buildFixtureRelease(fixture, sources);
  return publishRuntimeReleaseSet(fixture.home, fixture.cacheRoot, release);
}

async function buildFixtureRelease(fixture: Fixture, sources: Candidates) {
  const work = join(fixture.commandRoot, "work", String(++fixture.generation));
  await mkdir(work, { recursive: true });
  const candidates = {} as Candidates;
  for (const [name, source] of Object.entries(sources)) {
    const destination = join(work, name);
    await copyFile(source, destination);
    candidates[name as keyof Candidates] = destination;
  }
  await publishBuildReleaseSet(fixture.commandRoot, candidates);
  return readReadyBuildReleaseSet(fixture.dataRoot, "fixture");
}

function systemExecutable(name: string): string {
  const root = process.env.SystemRoot;
  if (!root) throw new Error("SystemRoot is unavailable");
  return join(root, "System32", name);
}
