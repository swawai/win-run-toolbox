import { lstat, stat } from "node:fs/promises";
import { isAbsolute, join, resolve } from "node:path";
import {
  controlledPath,
  ensureControlledDirectory,
  publishBuildReleaseSet,
  RUNTIME_ARTIFACT_NAMES,
} from "../_lib/release-set.ts";
import { acquireExclusiveFileLock } from "../_lib/windows-filesystem.ts";

if (Bun.argv.length !== 2) {
  throw new Error("proj.build.app does not accept dynamic arguments.");
}

const commandDataRoot = requiredAbsolute("SWAWKIT_PROJ_CORE_COMMAND_DATA_ROOT");
const swawkitHome = requiredAbsolute("SWAWKIT_HOME");
const cargoHome = requiredAbsolute("CARGO_HOME");
const appRoot = resolve(swawkitHome, "_lib", "proj", "_app");
const manifest = join(appRoot, "Cargo.toml");
const cargo = join(cargoHome, "bin", "cargo.exe");
await regularFile(manifest, "Cargo manifest");
await executableFile(cargo, "managed Cargo executable");

const locks = await ensureControlledDirectory(commandDataRoot, ["locks"], "build locks");
const work = await ensureControlledDirectory(commandDataRoot, ["work", "cargo"], "Cargo work");
using lock = await acquireExclusiveFileLock(join(locks, "build.lock"), 30 * 60 * 1000);

const build = Bun.spawn([
  cargo,
  "build",
  "--locked",
  "--release",
  "--manifest-path",
  manifest,
  "--target-dir",
  work,
], {
  cwd: appRoot,
  stdin: "inherit",
  stdout: "inherit",
  stderr: "inherit",
});
const exitCode = await build.exited;
if (exitCode !== 0) {
  throw new Error(`Cargo failed with exit code ${exitCode}.`);
}

const release = await ensureControlledDirectory(
  commandDataRoot,
  ["work", "cargo", "release"],
  "Cargo release output",
);
const candidates = Object.fromEntries(
  RUNTIME_ARTIFACT_NAMES
    .map((name) => [name, controlledPath(commandDataRoot, join(release, name), "build candidate")]),
) as Record<typeof RUNTIME_ARTIFACT_NAMES[number], string>;
for (const [name, path] of Object.entries(candidates)) {
  const metadata = await lstat(path);
  if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size <= 0) {
    throw new Error(`Cargo reported success but ${name} is missing or invalid: ${path}`);
  }
  console.log(`[BUILT] ${path} (${metadata.size} bytes)`);
}
const id = await publishBuildReleaseSet(commandDataRoot, candidates);
console.log(`[READY] proj.build.app release ${id}`);

function requiredAbsolute(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`required environment variable is missing: ${name}`);
  if (!isAbsolute(value)) throw new Error(`${name} must be absolute: ${value}`);
  return resolve(value);
}

async function regularFile(path: string, label: string): Promise<void> {
  const metadata = await lstat(path);
  if (!metadata.isFile() || metadata.isSymbolicLink()) {
    throw new Error(`${label} is not a regular file: ${path}`);
  }
}

async function executableFile(path: string, label: string): Promise<void> {
  const metadata = await stat(path);
  if (!metadata.isFile() || metadata.size <= 0) {
    throw new Error(`${label} is not a file: ${path}`);
  }
}
