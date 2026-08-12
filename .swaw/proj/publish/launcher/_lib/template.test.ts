import { afterEach, expect, test } from "bun:test";
import { lstat, mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { BuildArtifact } from "../../../build/launcher/_lib/artifact.ts";
import { publishLauncherTemplate } from "./template.ts";

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

test("publishes atomically and is idempotent", async () => {
  const fixture = await makeFixture();
  const artifact = await makeArtifact(fixture.root, "launcher-v1");
  const first = await publishLauncherTemplate(fixture.home, fixture.cache, artifact);
  expect(first.changed).toBeTrue();
  expect(await readFile(first.path, "utf8")).toBe("launcher-v1");
  const before = (await lstat(first.path)).birthtimeMs;

  const second = await publishLauncherTemplate(fixture.home, fixture.cache, artifact);
  expect(second.changed).toBeFalse();
  expect((await lstat(second.path)).birthtimeMs).toBe(before);
  expect((await readdir(join(fixture.home, "Favorites"))).filter((name) => name.startsWith(".")))
    .toEqual([]);
});

test("rejects a target reparse point without changing its destination", async () => {
  const fixture = await makeFixture();
  const artifact = await makeArtifact(fixture.root, "launcher-v2");
  const external = join(fixture.root, "external.exe");
  await writeFile(external, "external");
  await symlink(external, join(fixture.home, "Favorites", "template.proj1.exe"), "file");

  await expect(publishLauncherTemplate(fixture.home, fixture.cache, artifact)).rejects.toThrow(
    "target is unsafe",
  );
  expect(await readFile(external, "utf8")).toBe("external");
});

async function makeFixture() {
  const root = await mkdtemp(join(tmpdir(), "swawkit-launcher-template-"));
  roots.push(root);
  const home = join(root, "home");
  const cache = join(home, "data", "proj_cache");
  await mkdir(join(home, "Favorites"), { recursive: true });
  await mkdir(cache, { recursive: true });
  return { root, home, cache };
}

async function makeArtifact(root: string, content: string): Promise<BuildArtifact> {
  const path = join(root, "candidate.exe");
  await writeFile(path, content);
  return {
    path,
    length: Buffer.byteLength(content),
    sha256: new Bun.CryptoHasher("sha256").update(content).digest("hex"),
  };
}
