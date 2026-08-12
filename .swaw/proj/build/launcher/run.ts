import { randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { copyFile, lstat, readFile, rm } from "node:fs/promises";
import { isAbsolute, join, resolve } from "node:path";
import { ensureControlledDirectory } from "../_lib/release-set.ts";
import { acquireExclusiveFileLock, moveFileReplace } from "../_lib/windows-filesystem.ts";
import { publishBuildArtifact } from "./_lib/artifact.ts";

if (Bun.argv.length !== 2) throw new Error("proj.build.launcher does not accept dynamic arguments.");

const commandRoot = requiredAbsolute("SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT");
const projHome = requiredAbsolute("SWAWKIT_HOME");
const launcherRoot = join(projHome, "_lib", "proj", "_launcher");
const source = await regularFile(join(launcherRoot, "launcher.c"), "Launcher source");
const contract = await readContract(join(launcherRoot, "build.json"));
const tools = join(requiredAbsolute("VCToolsInstallDir"), "bin", "Hostx64", "x64");
const compiler = await regularFile(join(tools, "cl.exe"), "managed C compiler");
const linker = await regularFile(join(tools, "link.exe"), "managed linker");
const locks = await ensureControlledDirectory(commandRoot, ["locks"], "Launcher build locks");
const work = await ensureControlledDirectory(commandRoot, ["work", "launcher"], "Launcher build work");
const release = await ensureControlledDirectory(work, ["release"], "Launcher build release");
using lock = await acquireExclusiveFileLock(join(locks, "build.lock"), 30 * 60 * 1000);

const object = join(work, "launcher.obj");
const staged = join(work, "template.proj1.exe");
await requireReplaceableFile(object, "Launcher object target");
await requireReplaceableFile(staged, "Launcher staged executable target");
await run(compiler, [...contract.compileArguments, `/Fo${object}`, source]);
await run(linker, [
  ...contract.linkArguments,
  `/OUT:${staged}`,
  object,
  ...contract.libraries,
]);
const candidate = join(release, "template.proj1.exe");
await requireReplaceableFile(candidate, "Launcher candidate target");
const metadata = await lstat(staged);
if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size <= 0 || metadata.size > contract.maximumBytes) {
  throw new Error(`unexpected Launcher size ${metadata.size}; expected 1-${contract.maximumBytes} bytes`);
}
const candidateStage = join(release, `.template.proj1.${randomUUID().replaceAll("-", "")}.tmp`);
await copyFile(staged, candidateStage, constants.COPYFILE_EXCL);
if (!await sameFile(staged, candidateStage)) {
  await rm(candidateStage, { force: true });
  throw new Error(`staged Launcher candidate copy is corrupt: ${candidateStage}`);
}
try {
  moveFileReplace(candidateStage, candidate);
} catch (error) {
  throw new Error(`cannot publish Launcher candidate; recovery temporary: '${candidateStage}'; ${error}`);
}
await rm(candidateStage, { force: true });
const published = await publishBuildArtifact(commandRoot);
console.log(`[BUILT] ${candidate} (${published.length} bytes)`);
console.log(`[READY] proj.build.launcher (sha256-${published.sha256})`);

async function run(executable: string, arguments_: string[]): Promise<void> {
  const child = Bun.spawn([executable, ...arguments_], {
    cwd: launcherRoot,
    stdin: "inherit",
    stdout: "inherit",
    stderr: "inherit",
    windowsHide: true,
  });
  const code = await child.exited;
  if (code !== 0) throw new Error(`${executable} failed with exit code ${code}`);
}

async function readContract(path: string) {
  const value = JSON.parse(await readFile(await regularFile(path, "Launcher build contract"), "utf8"));
  if (
    !value || typeof value !== "object" || Array.isArray(value)
    || Object.keys(value).sort().join("\n")
      !== ["compileArguments", "libraries", "linkArguments", "maximumBytes", "schema"].sort().join("\n")
    || value.schema !== "swawkit.proj-launcher-build/v1"
    || !stringArray(value.compileArguments) || !stringArray(value.linkArguments)
    || !stringArray(value.libraries)
    || !Number.isSafeInteger(value.maximumBytes) || value.maximumBytes <= 0
  ) throw new Error(`Launcher build contract is invalid: ${path}`);
  return value as {
    compileArguments: string[]; linkArguments: string[]; libraries: string[]; maximumBytes: number;
  };
}

function stringArray(value: unknown): value is string[] {
  return Array.isArray(value) && value.length > 0
    && value.every((item) => typeof item === "string" && item.length > 0);
}

async function regularFile(path: string, label: string): Promise<string> {
  const metadata = await lstat(path);
  if (!metadata.isFile() || metadata.isSymbolicLink()) throw new Error(`${label} is invalid: ${path}`);
  return path;
}

async function sameFile(left: string, right: string): Promise<boolean> {
  const [leftMetadata, rightMetadata, leftBytes, rightBytes] = await Promise.all([
    lstat(left),
    lstat(right),
    readFile(left),
    readFile(right),
  ]);
  return leftMetadata.isFile() && !leftMetadata.isSymbolicLink()
    && rightMetadata.isFile() && !rightMetadata.isSymbolicLink()
    && leftMetadata.size === rightMetadata.size
    && new Bun.CryptoHasher("sha256").update(leftBytes).digest("hex")
      === new Bun.CryptoHasher("sha256").update(rightBytes).digest("hex");
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

function requiredAbsolute(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`required environment variable is missing: ${name}`);
  if (!isAbsolute(value)) throw new Error(`${name} must be absolute: ${value}`);
  return resolve(value);
}
