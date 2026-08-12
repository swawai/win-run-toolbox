import { isAbsolute, join, resolve } from "node:path";
import { ensureControlledDirectory, requireControlledDirectory } from "../../build/_lib/release-set.ts";
import { acquireExclusiveFileLock } from "../../build/_lib/windows-filesystem.ts";
import { readReadyBuildArtifact } from "../../build/launcher/_lib/artifact.ts";
import { publishLauncherTemplate } from "./_lib/template.ts";

if (Bun.argv.length !== 2) throw new Error("proj.publish.launcher does not accept dynamic arguments.");
const projHome = requiredAbsolute("SWAWKIT_HOME");
const dataRoot = requiredAbsolute("SWAWKIT_PROJ_DATA_ROOT");
const entryCommand = process.env.SWAWKIT_PROJ_ENTRY_COMMAND;
if (!entryCommand) throw new Error("required environment variable is missing: SWAWKIT_PROJ_ENTRY_COMMAND");

const providerLocks = await requireControlledDirectory(
  dataRoot,
  ["modules", "action", "proj", "build", "launcher", "locks"],
  "proj.build.launcher locks",
);
using providerLock = await acquireExclusiveFileLock(join(providerLocks, "build.lock"), 120_000);
const artifact = await readReadyBuildArtifact(dataRoot, entryCommand);
const cache = await ensureControlledDirectory(join(projHome, "data", "proj_cache"), [], "shared cache");
const locks = await ensureControlledDirectory(cache, ["locks"], "Launcher publish locks");
using publishLock = await acquireExclusiveFileLock(join(locks, "launcher-template-publish.lock"), 120_000);
const published = await publishLauncherTemplate(projHome, cache, artifact);
console.log(`[${published.changed ? "PUBLISHED" : "CURRENT"}] ${published.path} (${artifact.sha256})`);

function requiredAbsolute(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`required environment variable is missing: ${name}`);
  if (!isAbsolute(value)) throw new Error(`${name} must be absolute: ${value}`);
  return resolve(value);
}
