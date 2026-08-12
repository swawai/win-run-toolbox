import { createHash, randomUUID } from "node:crypto";
import {
  copyFile,
  lstat,
  mkdir,
  open,
  readFile,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";
import { moveFileReplace } from "./windows-filesystem.ts";

const BUILD_SCHEMA = "swawkit.proj-build-release-set/v1";
const RUNTIME_SCHEMA = "swawkit.proj-release-set/v1";
const STATE_SCHEMA = "swawkit.command-provider-state/v1";
const MAX_MANIFEST_BYTES = 1024 * 1024;
export const PRODUCER_CONTRACT = "swawkit.proj-build-app/v3";
export const RUNTIME_ARTIFACT_NAMES = [
  "swawkit-proj.exe",
  "swawkit-proj-host.exe",
  "swawkit-proj-toolchain.exe",
] as const;
type RuntimeArtifactName = typeof RUNTIME_ARTIFACT_NAMES[number];

export type Artifact = {
  name: string;
  path: string;
  length: number;
  sha256: string;
};

export type BuildReleaseSet = {
  releaseId: string;
  root: string;
  artifacts: Artifact[];
};

function sha256(content: string | Buffer): string {
  return createHash("sha256").update(content).digest("hex");
}

async function fileRecord(name: string, path: string): Promise<Artifact> {
  if (!/^[A-Za-z0-9][A-Za-z0-9._+-]*$/.test(name)) {
    throw new Error(`invalid Release Set artifact name: '${name}'`);
  }
  const metadata = await lstat(path);
  if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size <= 0) {
    throw new Error(`Release Set build candidate is invalid: ${path}`);
  }
  return {
    name,
    path,
    length: metadata.size,
    sha256: sha256(await readFile(path)),
  };
}

function releaseId(artifacts: Artifact[]): string {
  const records = new Map(artifacts.map((artifact) => [artifact.name, artifact]));
  if (records.size !== RUNTIME_ARTIFACT_NAMES.length) {
    throw new Error("the App build Release Set has the wrong artifact membership");
  }
  const identity = [RUNTIME_SCHEMA];
  for (const name of RUNTIME_ARTIFACT_NAMES) {
    const artifact = records.get(name);
    if (!artifact) {
      throw new Error("the App build Release Set has the wrong artifact membership");
    }
    identity.push(artifact.name, String(artifact.length), artifact.sha256);
  }
  return sha256(identity.join("\n"));
}

async function regularDirectory(path: string, label: string): Promise<void> {
  const metadata = await lstat(path);
  if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
    throw new Error(`${label} must be a regular directory: ${path}`);
  }
}

export async function ensureControlledDirectory(
  root: string,
  segments: string[],
  label: string,
): Promise<string> {
  if (!isAbsolute(root)) {
    throw new Error(`${label} root must be absolute: ${root}`);
  }
  await regularDirectory(root, label);
  let current = resolve(root);
  for (const segment of segments) {
    if (!segment || segment === "." || segment === ".." || /[\\/]/.test(segment)) {
      throw new Error(`unsafe ${label} segment: '${segment}'`);
    }
    current = join(current, segment);
    await mkdir(current).catch((error: NodeJS.ErrnoException) => {
      if (error.code !== "EEXIST") throw error;
    });
    await regularDirectory(current, label);
  }
  return current;
}

export async function requireControlledDirectory(
  root: string,
  segments: string[],
  label: string,
): Promise<string> {
  if (!isAbsolute(root)) throw new Error(`${label} root must be absolute: ${root}`);
  await regularDirectory(root, label);
  let current = resolve(root);
  for (const segment of segments) {
    if (!segment || segment === "." || segment === ".." || /[\\/]/.test(segment)) {
      throw new Error(`unsafe ${label} segment: '${segment}'`);
    }
    current = join(current, segment);
    await regularDirectory(current, label);
  }
  return current;
}

export function controlledPath(root: string, path: string, label: string): string {
  const rootPath = resolve(root);
  const result = resolve(path);
  const child = relative(rootPath, result);
  if (!child || child.startsWith("..") || isAbsolute(child)) {
    throw new Error(`${label} escapes the command data root: ${result}`);
  }
  return result;
}

async function writeAtomic(path: string, content: string): Promise<void> {
  try {
    const current = await lstat(path);
    if (!current.isFile() || current.isSymbolicLink()) {
      throw new Error(`atomic publication target must be a regular file: ${path}`);
    }
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
  const temporary = join(
    dirname(path),
    `.${basename(path)}.${randomUUID().replaceAll("-", "")}.tmp`,
  );
  const file = await open(temporary, "wx");
  try {
    await file.writeFile(content, { encoding: "utf8" });
    await file.sync();
  } finally {
    await file.close();
  }
  moveFileReplace(temporary, path);
}

function json(value: unknown): string {
  return `${JSON.stringify(value, null, 2).replaceAll("\n", "\r\n")}\r\n`;
}

export async function readBuildReleaseDirectory(
  root: string,
  expectedId: string,
  expectedNames: readonly string[],
): Promise<Artifact[]> {
  await regularDirectory(root, "build Release Set");
  const manifestPath = join(root, "manifest.json");
  const manifestMetadata = await lstat(manifestPath);
  if (
    !manifestMetadata.isFile()
    || manifestMetadata.isSymbolicLink()
    || manifestMetadata.size <= 0
    || manifestMetadata.size > MAX_MANIFEST_BYTES
  ) {
    throw new Error(`build Release Set manifest is invalid: ${manifestPath}`);
  }
  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  if (
    !manifest || typeof manifest !== "object" || Array.isArray(manifest)
    || Object.keys(manifest).sort().join("\n")
      !== ["artifacts", "releaseId", "runtimeSchema", "schema"].sort().join("\n")
    || manifest.schema !== BUILD_SCHEMA
    || manifest.runtimeSchema !== RUNTIME_SCHEMA
    || manifest.releaseId !== expectedId
    || !Array.isArray(manifest.artifacts)
  ) {
    throw new Error(`build Release Set manifest is invalid: ${root}`);
  }
  const records: Artifact[] = [];
  for (const value of manifest.artifacts) {
    if (
      !value || typeof value !== "object" || Array.isArray(value)
      || Object.keys(value).sort().join("\n") !== ["length", "name", "sha256"].join("\n")
      || typeof value.name !== "string"
      || !Number.isSafeInteger(value.length) || value.length <= 0
      || typeof value.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(value.sha256)
    ) {
      throw new Error(`build Release Set manifest is invalid: ${root}`);
    }
    const record = await fileRecord(value.name, join(root, value.name));
    if (record.length !== value.length || record.sha256 !== value.sha256) {
      throw new Error(`build Release Set artifact is corrupt: ${record.path}`);
    }
    records.push(record);
  }
  const names = records.map(({ name }) => name).sort();
  if (names.join("\n") !== [...expectedNames].sort().join("\n") || releaseId(records) !== expectedId) {
    throw new Error(`build Release Set identity is invalid: ${root}`);
  }
  return records;
}

export async function publishBuildReleaseSet(
  commandDataRoot: string,
  candidates: Record<RuntimeArtifactName, string>,
): Promise<string> {
  const names = Object.keys(candidates).sort();
  const expectedNames = [...RUNTIME_ARTIFACT_NAMES].sort();
  if (names.length === 0) {
    throw new Error("a build Release Set must contain at least one artifact");
  }
  if (names.join("\n") !== expectedNames.join("\n")) {
    throw new Error("the App build Release Set has the wrong artifact membership");
  }
  const artifacts = await Promise.all(
    Object.entries(candidates).map(([name, path]) =>
      fileRecord(name, controlledPath(commandDataRoot, path, "build candidate"))
    ),
  );
  const id = releaseId(artifacts);
  const inputRevision = `sha256-${id}`;
  const token = randomUUID().replaceAll("-", "").toLowerCase();
  const statePath = join(commandDataRoot, "_state.json");
  await writeAtomic(statePath, json({
    schema: STATE_SCHEMA,
    status: "unavailable",
    inputRevision,
    token,
  }));

  const exportRoot = await ensureControlledDirectory(commandDataRoot, ["export"], "build export");
  const releasesRoot = await ensureControlledDirectory(exportRoot, ["releases"], "build releases");
  const releaseRoot = join(releasesRoot, id);
  try {
    await regularDirectory(releaseRoot, "build Release Set");
    await readBuildReleaseDirectory(releaseRoot, id, expectedNames);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    const stage = join(exportRoot, `.release.${token}.tmp`);
    await mkdir(stage);
    let committed = false;
    try {
      for (const artifact of artifacts) {
        const destination = join(stage, artifact.name);
        await copyFile(artifact.path, destination);
        const copied = await fileRecord(artifact.name, destination);
        if (copied.length !== artifact.length || copied.sha256 !== artifact.sha256) {
          throw new Error(`staged Release Set artifact is corrupt: ${destination}`);
        }
      }
      await writeFile(join(stage, "manifest.json"), json({
        schema: BUILD_SCHEMA,
        runtimeSchema: RUNTIME_SCHEMA,
        releaseId: id,
        artifacts: artifacts.map(({ name, length, sha256 }) => ({ name, length, sha256 })),
      }), { encoding: "utf8", flag: "wx" });
      try {
        await rename(stage, releaseRoot);
        committed = true;
      } catch (error) {
        const code = (error as NodeJS.ErrnoException).code;
        if (code !== "EEXIST" && code !== "EPERM") throw error;
        await readBuildReleaseDirectory(releaseRoot, id, expectedNames);
      }
    } finally {
      if (!committed) await rm(stage, { recursive: true, force: true });
    }
  }
  await readBuildReleaseDirectory(releaseRoot, id, expectedNames);
  await writeAtomic(join(exportRoot, "current"), `${id}\r\n`);
  await writeAtomic(statePath, json({
    schema: STATE_SCHEMA,
    status: "ready",
    inputRevision,
    token,
    producerContract: PRODUCER_CONTRACT,
  }));
  return id;
}
