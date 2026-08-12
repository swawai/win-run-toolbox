import { isAbsolute, join, resolve } from "node:path";
import { readReadyBuildReleaseSet } from "../../build/_lib/provider-release.ts";
import { requireControlledDirectory } from "../../build/_lib/release-set.ts";
import { acquireExclusiveFileLock } from "../../build/_lib/windows-filesystem.ts";
import { publishRuntimeReleaseSet } from "../_lib/runtime-release.ts";

if (Bun.argv.length !== 2) {
  throw new Error("proj.publish.app does not accept dynamic arguments.");
}

const projHome = requiredAbsolute("SWAWKIT_HOME");
const dataRoot = requiredAbsolute("SWAWKIT_PROJ_DATA_ROOT");
const entryCommand = process.env.SWAWKIT_PROJ_ENTRY_COMMAND;
if (!entryCommand) throw new Error("required environment variable is missing: SWAWKIT_PROJ_ENTRY_COMMAND");

const locks = await requireControlledDirectory(
  dataRoot,
  ["modules", "action", "proj", "build", "app", "locks"],
  "proj.build.app locks",
);
using providerLock = await acquireExclusiveFileLock(join(locks, "build.lock"), 120_000);
const release = await readReadyBuildReleaseSet(dataRoot, entryCommand);
const id = await publishRuntimeReleaseSet(projHome, join(projHome, "data", "proj_cache"), release);
console.log(`[PUBLISHED] Release Set ${id}`);

function requiredAbsolute(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`required environment variable is missing: ${name}`);
  if (!isAbsolute(value)) throw new Error(`${name} must be absolute: ${value}`);
  return resolve(value);
}
