import { afterEach, expect, test } from "bun:test";
import { lstat, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { publishBuildArtifact, readReadyBuildArtifact } from "./artifact.ts";

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

test("publishes and reads one coherent Launcher Provider snapshot", async () => {
  const fixture = await makeFixture();
  const source = fixture.candidate;
  await mkdir(join(fixture.commandRoot, "work", "launcher", "release"), { recursive: true });
  await writeFile(source, "launcher-fixture");
  const published = await publishBuildArtifact(fixture.commandRoot);

  const resolved = await readReadyBuildArtifact(fixture.dataRoot, "fixture");
  expect(resolved).toEqual(published);
  expect(await readFile(resolved.path, "utf8")).toBe("launcher-fixture");
  const state = JSON.parse(await readFile(join(fixture.commandRoot, "_state.json"), "utf8"));
  expect(state).toMatchObject({
    schema: "swawkit.command-provider-state/v1",
    status: "ready",
    inputRevision: `sha256-${published.sha256}`,
    producerContract: "swawkit.proj-build-launcher/v1",
  });

  await writeFile(resolved.path, "tampered-fixture");
  await expect(readReadyBuildArtifact(fixture.dataRoot, "fixture")).rejects.toThrow(
    "run 'fixture proj.build.launcher'",
  );
});

test("a failed publication revokes Ready without replacing the previous export", async () => {
  const fixture = await makeFixture();
  const work = join(fixture.commandRoot, "work", "launcher", "release");
  await mkdir(work, { recursive: true });
  const source = fixture.candidate;
  await writeFile(source, "known-good");
  const published = await publishBuildArtifact(fixture.commandRoot);
  const previous = await readFile(published.path);

  await rm(source);
  await mkdir(source);
  await expect(publishBuildArtifact(fixture.commandRoot)).rejects.toThrow("regular file");
  expect(await readFile(published.path)).toEqual(previous);
  const state = JSON.parse(await readFile(join(fixture.commandRoot, "_state.json"), "utf8"));
  expect(state.status).toBe("ready");

  await rm(source, { recursive: true });
  await writeFile(source, "new-candidate");
  await rm(published.path);
  await mkdir(published.path);
  await expect(publishBuildArtifact(fixture.commandRoot)).rejects.toThrow(
    "export target is unsafe",
  );
  const failed = JSON.parse(await readFile(join(fixture.commandRoot, "_state.json"), "utf8"));
  expect(failed.status).toBe("unavailable");
});

async function makeFixture() {
  const root = await mkdtemp(join(tmpdir(), "swawkit-launcher-artifact-"));
  roots.push(root);
  const dataRoot = join(root, "data");
  const commandRoot = join(dataRoot, "modules", "action", "proj", "build", "launcher");
  await mkdir(commandRoot, { recursive: true });
  return {
    dataRoot,
    commandRoot,
    candidate: join(commandRoot, "work", "launcher", "release", "template.proj1.exe"),
  };
}
