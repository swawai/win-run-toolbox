import { createHash, randomUUID } from "node:crypto";
import { copyFile, lstat, mkdir, open, readFile, rename, rm, writeFile } from "node:fs/promises";
import { basename, join } from "node:path";
import {
  type Artifact,
  type BuildReleaseSet,
  ensureControlledDirectory,
  requireControlledDirectory,
  RUNTIME_ARTIFACT_NAMES,
} from "../../build/_lib/release-set.ts";
import {
  acquireExclusiveFileLock,
  moveFileReplace,
} from "../../build/_lib/windows-filesystem.ts";

const RUNTIME_SCHEMA = "swawkit.proj-release-set/v1";
const MAX_MANIFEST_BYTES = 1024 * 1024;

export async function publishRuntimeReleaseSet(
  projHome: string,
  cacheDataRoot: string,
  release: BuildReleaseSet,
): Promise<string> {
  if (releaseIdentity(release.artifacts) !== release.releaseId) {
    throw new Error("the application Release Set ID does not match its artifacts");
  }
  const kernelRoot = await requireControlledDirectory(
    projHome,
    ["_lib", "proj"],
    "Proj kernel",
  );
  const runtimeRoot = await ensureControlledDirectory(kernelRoot, ["_bin"], "runtime root");
  const cacheRoot = await ensureControlledDirectory(cacheDataRoot, [], "shared cache");
  const locks = await ensureControlledDirectory(cacheRoot, ["locks"], "runtime locks");
  using lock = await acquireExclusiveFileLock(join(locks, "release-publish.lock"), 120_000);
  const releasesRoot = await ensureControlledDirectory(runtimeRoot, ["releases"], "runtime releases");
  const target = join(releasesRoot, release.releaseId);
  if (await missing(target)) {
    await publishDirectory(releasesRoot, target, release);
  }
  await validateRuntimeRelease(target, release);
  await publishSelector(runtimeRoot, release.releaseId);
  return release.releaseId;
}

function releaseIdentity(artifacts: Artifact[]): string {
  const records = new Map(artifacts.map((artifact) => [artifact.name, artifact]));
  if (records.size !== RUNTIME_ARTIFACT_NAMES.length) {
    throw new Error("the application Release Set has invalid membership");
  }
  const identity = [RUNTIME_SCHEMA];
  for (const name of RUNTIME_ARTIFACT_NAMES) {
    const artifact = records.get(name);
    if (!artifact) throw new Error("the application Release Set has invalid membership");
    identity.push(name, String(artifact.length), artifact.sha256);
  }
  return createHash("sha256").update(identity.join("\n")).digest("hex");
}

async function publishDirectory(
  releasesRoot: string,
  target: string,
  release: BuildReleaseSet,
): Promise<void> {
  const stage = join(releasesRoot, `.${release.releaseId}.${randomUUID().replaceAll("-", "")}.tmp`);
  await mkdir(stage);
  let committed = false;
  try {
    for (const artifact of release.artifacts) {
      await copyFile(artifact.path, join(stage, artifact.name));
    }
    await writeFile(join(stage, "manifest.json"), json({
      schema: RUNTIME_SCHEMA,
      releaseId: release.releaseId,
      artifacts: release.artifacts.map(({ name, length, sha256 }) => ({ name, length, sha256 })),
    }), { flag: "wx" });
    await validateRuntimeRelease(stage, release, false);
    try {
      await rename(stage, target);
      committed = true;
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (code !== "EEXIST" && code !== "EPERM") throw error;
      await validateRuntimeRelease(target, release);
    }
  } finally {
    if (!committed) await rm(stage, { recursive: true, force: true });
  }
}

async function validateRuntimeRelease(
  root: string,
  expected: BuildReleaseSet,
  requireReleaseLeaf = true,
): Promise<void> {
  const metadata = await lstat(root);
  if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
    throw new Error(`runtime Release Set directory is unsafe: ${root}`);
  }
  if (requireReleaseLeaf && basename(root) !== expected.releaseId) {
    throw new Error(`runtime Release Set path is invalid: ${root}`);
  }
  const manifestPath = join(root, "manifest.json");
  const manifestMetadata = await lstat(manifestPath);
  if (
    !manifestMetadata.isFile()
    || manifestMetadata.isSymbolicLink()
    || manifestMetadata.size <= 0
    || manifestMetadata.size > MAX_MANIFEST_BYTES
  ) {
    throw new Error(`runtime Release Set manifest is invalid: ${manifestPath}`);
  }
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  if (
    !manifest || typeof manifest !== "object" || Array.isArray(manifest)
    || Object.keys(manifest).sort().join("\n") !== ["artifacts", "releaseId", "schema"].sort().join("\n")
    || manifest.schema !== RUNTIME_SCHEMA
    || manifest.releaseId !== expected.releaseId
    || !Array.isArray(manifest.artifacts)
  ) {
    throw new Error(`runtime Release Set manifest is invalid: ${manifestPath}`);
  }
  const records = new Map<string, Artifact>();
  for (const record of manifest.artifacts) {
    if (
      !record || typeof record !== "object" || Array.isArray(record)
      || Object.keys(record).sort().join("\n") !== ["length", "name", "sha256"].join("\n")
      || typeof record.name !== "string"
      || !Number.isSafeInteger(record.length) || record.length <= 0
      || typeof record.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(record.sha256)
      || records.has(record.name)
    ) throw new Error(`runtime Release Set manifest is invalid: ${manifestPath}`);
    records.set(record.name, record);
  }
  for (const artifact of expected.artifacts) {
    const record = records.get(artifact.name);
    const path = join(root, artifact.name);
    const item = await lstat(path);
    const bytes = await readFile(path);
    const actual = new Bun.CryptoHasher("sha256").update(bytes).digest("hex");
    if (
      !record || !item.isFile() || item.isSymbolicLink()
      || item.size !== artifact.length || record.length !== artifact.length
      || actual !== artifact.sha256 || record.sha256 !== artifact.sha256
    ) throw new Error(`runtime Release Set artifact is corrupt: ${path}`);
  }
  if (records.size !== RUNTIME_ARTIFACT_NAMES.length) {
    throw new Error(`runtime Release Set has invalid membership: ${manifestPath}`);
  }
}

async function publishSelector(runtimeRoot: string, id: string): Promise<void> {
  const current = join(runtimeRoot, "current");
  try {
    const metadata = await lstat(current);
    if (!metadata.isFile() || metadata.isSymbolicLink()) {
      throw new Error(`runtime Release Set selector is unsafe: ${current}`);
    }
    if (metadata.size <= 128 && (await readFile(current, "utf8")) === `${id}\n`) return;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
  const stage = join(runtimeRoot, `.current.${randomUUID().replaceAll("-", "")}.tmp`);
  const file = await open(stage, "wx");
  try {
    await file.writeFile(`${id}\n`);
    await file.sync();
  } finally {
    await file.close();
  }
  moveFileReplace(stage, current);
}

async function missing(path: string): Promise<boolean> {
  try {
    await lstat(path);
    return false;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return true;
    throw error;
  }
}

function json(value: unknown): string {
  return `${JSON.stringify(value, null, 2).replaceAll("\n", "\r\n")}\r\n`;
}
