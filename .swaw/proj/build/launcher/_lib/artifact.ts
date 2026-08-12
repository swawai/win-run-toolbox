import { createHash, randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { copyFile, lstat, open, readFile, rm } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import {
  ensureControlledDirectory,
  requireControlledDirectory,
} from "../../_lib/release-set.ts";
import { moveFileReplace } from "../../_lib/windows-filesystem.ts";

const MANIFEST_SCHEMA = "swawkit.proj-build-artifact/v1";
export const PRODUCER_ADDRESS = "proj.build.launcher";
export const PRODUCER_CONTRACT = "swawkit.proj-build-launcher/v1";
export const ARTIFACT_NAME = "template.proj1.exe";
const STATE_SCHEMA = "swawkit.command-provider-state/v1";
const MAX_DOCUMENT_BYTES = 1024 * 1024;

export type BuildArtifact = {
  path: string;
  length: number;
  sha256: string;
};

export async function publishBuildArtifact(
  commandRoot: string,
): Promise<BuildArtifact> {
  await requireControlledDirectory(commandRoot, [], "Launcher command data");
  const release = await requireControlledDirectory(
    commandRoot,
    ["work", "launcher", "release"],
    "Launcher build release",
  );
  const source = join(release, ARTIFACT_NAME);
  const artifact = await inspectArtifact(source, ARTIFACT_NAME);
  const token = randomUUID().replaceAll("-", "").toLowerCase();
  const inputRevision = `sha256-${artifact.sha256}`;
  await writeState(commandRoot, { status: "unavailable", inputRevision, token });
  const exportRoot = await ensureControlledDirectory(commandRoot, ["export"], "Launcher export");
  const destination = join(exportRoot, ARTIFACT_NAME);
  await publishFile(source, destination, artifact);
  const published = await inspectArtifact(destination, ARTIFACT_NAME);
  await writeAtomic(join(exportRoot, "manifest.json"), json({
    schema: MANIFEST_SCHEMA,
    producerAddress: PRODUCER_ADDRESS,
    producerContract: PRODUCER_CONTRACT,
    inputRevision,
    token,
    artifact: { name: ARTIFACT_NAME, length: published.length, sha256: published.sha256 },
  }));
  await writeState(commandRoot, {
    status: "ready",
    inputRevision,
    token,
    producerContract: PRODUCER_CONTRACT,
  });
  return published;
}

export async function readReadyBuildArtifact(
  dataRoot: string,
  entryCommand: string,
): Promise<BuildArtifact> {
  try {
    const provider = await requireControlledDirectory(
      dataRoot,
      ["modules", "action", "proj", "build", "launcher"],
      "proj.build.launcher provider",
    );
    const exportRoot = await requireControlledDirectory(provider, ["export"], "Launcher export");
    const initial = await readState(join(provider, "_state.json"));
    const manifestPath = join(exportRoot, "manifest.json");
    const manifest = await readJson(manifestPath) as Record<string, unknown>;
    if (
      keys(manifest) !== keysOf("artifact", "inputRevision", "producerAddress", "producerContract", "schema", "token")
      || manifest.schema !== MANIFEST_SCHEMA || manifest.producerAddress !== PRODUCER_ADDRESS
      || manifest.producerContract !== PRODUCER_CONTRACT
      || manifest.inputRevision !== initial.inputRevision || manifest.token !== initial.token
      || !manifest.artifact || typeof manifest.artifact !== "object" || Array.isArray(manifest.artifact)
    ) throw new Error("its artifact manifest is invalid");
    const record = manifest.artifact as Record<string, unknown>;
    if (
      keys(record) !== keysOf("length", "name", "sha256") || record.name !== ARTIFACT_NAME
      || !Number.isSafeInteger(record.length) || (record.length as number) <= 0
      || typeof record.sha256 !== "string" || !/^[a-f0-9]{64}$/.test(record.sha256)
      || initial.inputRevision !== `sha256-${record.sha256}`
    ) throw new Error("its artifact manifest is invalid");
    const artifact = await inspectArtifact(join(exportRoot, ARTIFACT_NAME), ARTIFACT_NAME);
    if (artifact.length !== record.length || artifact.sha256 !== record.sha256) {
      throw new Error("its artifact does not match the manifest");
    }
    const final = await readState(join(provider, "_state.json"));
    if (JSON.stringify(final) !== JSON.stringify(initial)) throw new Error("it changed while being read");
    return artifact;
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    throw new Error(
      `required export from '${PRODUCER_ADDRESS}' is invalid because ${reason}; run '${entryCommand} ${PRODUCER_ADDRESS}'`,
    );
  }
}

async function readState(path: string) {
  const value = await readJson(path) as Record<string, unknown>;
  if (
    keys(value) !== keysOf("inputRevision", "producerContract", "schema", "status", "token")
    || value.schema !== STATE_SCHEMA || value.status !== "ready"
    || value.producerContract !== PRODUCER_CONTRACT
    || typeof value.inputRevision !== "string" || !/^sha256-[a-f0-9]{64}$/.test(value.inputRevision)
    || typeof value.token !== "string" || !/^[a-f0-9]{32}$/.test(value.token)
  ) throw new Error("its Provider State is not Ready for the expected contract");
  return value;
}

async function writeState(commandRoot: string, value: Record<string, unknown>): Promise<void> {
  await writeAtomic(join(commandRoot, "_state.json"), json({ schema: STATE_SCHEMA, ...value }));
}

async function publishFile(source: string, destination: string, expected: BuildArtifact): Promise<void> {
  await requireReplaceableFile(destination, "Launcher export target");
  const temporary = temporarySibling(destination);
  let commitAttempted = false;
  try {
    await copyFile(source, temporary, constants.COPYFILE_EXCL);
    const copied = await inspectArtifact(temporary, ARTIFACT_NAME);
    if (copied.length !== expected.length || copied.sha256 !== expected.sha256) {
      throw new Error(`staged Launcher artifact is corrupt: ${temporary}`);
    }
    commitAttempted = true;
    moveFileReplace(temporary, destination);
  } catch (error) {
    if (commitAttempted) {
      throw new Error(
        `atomic Launcher artifact publication failed; recovery temporary: '${temporary}'; ${error}`,
      );
    }
    await rm(temporary, { force: true });
    throw error;
  }
}

async function inspectArtifact(path: string, label: string): Promise<BuildArtifact> {
  const metadata = await lstat(path);
  if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size <= 0) {
    throw new Error(`${label} must be a non-empty regular file: ${path}`);
  }
  const sha256 = createHash("sha256").update(await readFile(path)).digest("hex");
  return { path, length: metadata.size, sha256 };
}

async function readJson(path: string): Promise<unknown> {
  const metadata = await lstat(path);
  if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size <= 0 || metadata.size > MAX_DOCUMENT_BYTES) {
    throw new Error(`replaceable JSON document is invalid: ${path}`);
  }
  return JSON.parse(await readFile(path, "utf8"));
}

async function writeAtomic(path: string, content: string): Promise<void> {
  await requireReplaceableFile(path, "Launcher atomic publication target");
  const temporary = temporarySibling(path);
  let commitAttempted = false;
  try {
    const file = await open(temporary, "wx");
    try {
      await file.writeFile(content);
      await file.sync();
    } finally {
      await file.close();
    }
    commitAttempted = true;
    moveFileReplace(temporary, path);
  } catch (error) {
    if (commitAttempted) {
      throw new Error(`atomic publication failed; recovery temporary: '${temporary}'; ${error}`);
    }
    await rm(temporary, { force: true });
    throw error;
  }
}

async function requireReplaceableFile(path: string, label: string): Promise<void> {
  try {
    const metadata = await lstat(path);
    if (!metadata.isFile() || metadata.isSymbolicLink()) {
      throw new Error(`${label} is unsafe: ${path}`);
    }
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
}

function temporarySibling(path: string): string {
  return join(dirname(path), `.${basename(path)}.${randomUUID().replaceAll("-", "")}.tmp`);
}

function keys(value: Record<string, unknown>): string { return Object.keys(value).sort().join("\n"); }
function keysOf(...value: string[]): string { return value.sort().join("\n"); }
function json(value: unknown): string {
  return `${JSON.stringify(value, null, 2).replaceAll("\n", "\r\n")}\r\n`;
}
